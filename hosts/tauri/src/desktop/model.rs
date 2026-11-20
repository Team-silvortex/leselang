//! Native, one-shot model transport. This module never executes application work.
use serde::Deserialize;
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

const RESPONSE_LIMIT: usize = 128 * 1024;
const CONTENT_LIMIT: usize = 16 * 1024;
const TIMEOUT: Duration = Duration::from_secs(45);
const REQUEST_LIMIT: usize = 64;

#[derive(Clone)]
pub(super) struct Config {
    endpoint: String,
    model: String,
    key: Option<String>,
}

pub(super) struct Settings(pub Result<Option<Config>, String>);

impl Settings {
    pub fn from_env() -> Self {
        let get = |name| {
            std::env::var(name).map(Some).or_else(|error| match error {
                std::env::VarError::NotPresent => Ok(None),
                _ => Err("model_config_invalid".to_owned()),
            })
        };
        Self((|| {
            Config::parse(
                get("LESE_MODEL_ENDPOINT")?,
                get("LESE_MODEL_NAME")?,
                get("LESE_MODEL_API_KEY")?,
            )
        })())
    }

    pub fn info(&self) -> Result<Value, String> {
        Ok(match self.0.as_ref().map_err(Clone::clone)? {
            Some(config) => json!({"configured":true,"model":config.model}),
            None => json!({"configured":false}),
        })
    }

    pub fn config(&self) -> Result<Config, String> {
        self.0
            .as_ref()
            .map_err(Clone::clone)?
            .clone()
            .ok_or_else(|| "model_not_configured".into())
    }
}

impl Config {
    fn parse(
        endpoint: Option<String>,
        model: Option<String>,
        key: Option<String>,
    ) -> Result<Option<Self>, String> {
        if endpoint.is_none() && model.is_none() && key.is_none() {
            return Ok(None);
        }
        let bad = || "model_config_invalid".to_owned();
        let endpoint = endpoint.ok_or_else(bad)?;
        let model = model.ok_or_else(bad)?;
        if endpoint.len() > 2048
            || model.trim().is_empty()
            || model.len() > 128
            || model.chars().any(char::is_control)
            || endpoint.chars().any(char::is_whitespace)
        {
            return Err(bad());
        }
        let url = tauri::Url::parse(&endpoint).map_err(|_| bad())?;
        let loopback = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
        if !(url.scheme() == "https" || url.scheme() == "http" && loopback)
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.path() == "/"
        {
            return Err(bad());
        }
        if key.as_ref().is_some_and(|key| {
            key.is_empty()
                || key.len() > 4096
                || !key.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
        }) || !loopback && key.is_none()
        {
            return Err(bad());
        }
        Ok(Some(Self {
            endpoint,
            model,
            key,
        }))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Request {
    system: String,
    user: String,
}

impl Request {
    pub fn validate(&self) -> Result<(), String> {
        if self.system.trim().is_empty()
            || self.system.len() > 16 * 1024
            || self.user.trim().is_empty()
            || self.user.len() > 2 * 1024 * 1024
            || self.system.contains('\0')
            || self.user.contains('\0')
        {
            Err("model_request_invalid".into())
        } else {
            Ok(())
        }
    }
}

pub(super) struct Run {
    id: String,
    cancelled: AtomicBool,
}

#[derive(Default)]
pub(super) struct Registry {
    used: Vec<String>,
    active: Option<Arc<Run>>,
}

impl Registry {
    pub fn busy(&self) -> bool {
        self.active.is_some()
    }

    pub fn begin(&mut self, id: String) -> Result<Arc<Run>, String> {
        if self.busy() {
            return Err("model_request_pending".into());
        }
        if self.used.contains(&id) {
            return Err("model_request_retired".into());
        }
        if self.used.len() == REQUEST_LIMIT {
            return Err("model_request_limit".into());
        }
        let run = Arc::new(Run {
            id: id.clone(),
            cancelled: AtomicBool::new(false),
        });
        self.used.push(id);
        self.active = Some(run.clone());
        Ok(run)
    }

    pub fn cancel(&mut self, id: &str) -> Result<(), String> {
        if !self.used.iter().any(|used| used == id) {
            // Cancellation can overtake an asynchronous IPC invocation. Retain
            // a tombstone so that invocation can never start after the cancel.
            if self.used.len() == REQUEST_LIMIT {
                return Err("model_request_limit".into());
            }
            self.used.push(id.to_owned());
        }
        if let Some(run) = &self.active
            && run.id == id
        {
            run.cancelled.store(true, Ordering::Release);
        }
        Ok(())
    }

    pub fn close(&self) {
        if let Some(run) = &self.active {
            run.cancelled.store(true, Ordering::Release);
        }
    }

    pub fn finish(
        &mut self,
        run: &Arc<Run>,
        result: Result<String, String>,
    ) -> Result<String, String> {
        let current = self
            .active
            .as_ref()
            .is_some_and(|active| Arc::ptr_eq(active, run));
        if current {
            self.active.take();
        }
        if !current || run.cancelled.load(Ordering::Acquire) {
            Err("model_request_cancelled".into())
        } else {
            result
        }
    }
}

fn quoted(value: &str) -> String {
    // curl config strings use these escapes; request JSON has already escaped
    // its other control characters. No shell, argv secret or temporary file.
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\r', "\\r")
            .replace('\n', "\\n")
            .replace('\t', "\\t")
    )
}

fn curl_config(config: &Config, request: &Request) -> String {
    let body = json!({"model":config.model,"stream":false,"messages":[
        {"role":"system","content":request.system}, {"role":"user","content":request.user}
    ],"response_format":{"type":"json_object"}})
    .to_string();
    let mut result = format!(
        "url = {}\nheader = \"Content-Type: application/json\"\ndata-binary = {}\n",
        quoted(&config.endpoint),
        quoted(&body)
    );
    if let Some(key) = &config.key {
        result.push_str(&format!(
            "header = {}\n",
            quoted(&format!("Authorization: Bearer {key}"))
        ));
    }
    result
}

pub(super) fn complete(config: Config, request: Request, run: Arc<Run>) -> Result<String, String> {
    exchange(&config, &request, &run, TIMEOUT)
}

fn exchange(
    config: &Config,
    request: &Request,
    run: &Run,
    timeout: Duration,
) -> Result<String, String> {
    request.validate()?;
    if run.cancelled.load(Ordering::Acquire) {
        return Err("model_request_cancelled".into());
    }
    let started = Instant::now();
    let mut child = Command::new("/usr/bin/curl")
        .args([
            "--disable",
            "--silent",
            "--globoff",
            "--proto",
            "=https,http",
            "--max-time",
            "45",
            "--connect-timeout",
            "10",
            "--max-filesize",
            "131072",
            "--write-out",
            "\n%{http_code}",
            "--config",
            "-",
        ])
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "model_transport_unavailable".to_owned())?;
    let mut input = child.stdin.take().expect("piped stdin");
    let output = child.stdout.take().expect("piped stdout");
    let configuration = curl_config(config, request);
    let writer = std::thread::spawn(move || input.write_all(configuration.as_bytes()));
    let excessive = Arc::new(AtomicBool::new(false));
    let read_excessive = excessive.clone();
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = output
            .take((RESPONSE_LIMIT + 5) as u64)
            .read_to_end(&mut bytes);
        if bytes.len() > RESPONSE_LIMIT + 4 {
            read_excessive.store(true, Ordering::Release);
        }
        result.map(|_| bytes)
    });
    let outcome = loop {
        let failure = if run.cancelled.load(Ordering::Acquire) {
            Some("model_request_cancelled")
        } else if started.elapsed() >= timeout {
            Some("model_request_timeout")
        } else if excessive.load(Ordering::Acquire) {
            Some("model_response_too_large")
        } else {
            None
        };
        if let Some(failure) = failure {
            let _ = child.kill();
            let _ = child.wait();
            break Err(failure.to_owned());
        }
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err("model_transport_failed".into());
            }
        }
    };
    // Keep the admission slot until both pipes and the child have settled.
    let written = writer.join();
    let received = reader.join();
    let status = outcome?;
    if run.cancelled.load(Ordering::Acquire) {
        return Err("model_request_cancelled".into());
    }
    if excessive.load(Ordering::Acquire) || status.code() == Some(63) {
        return Err("model_response_too_large".into());
    }
    if status.code() == Some(28) {
        return Err("model_request_timeout".into());
    }
    if !status.success() || !matches!(written, Ok(Ok(()))) {
        return Err("model_transport_failed".into());
    }
    let bytes = received
        .map_err(|_| "model_transport_failed")?
        .map_err(|_| "model_transport_failed")?;
    let text = std::str::from_utf8(&bytes).map_err(|_| "model_response_invalid")?;
    let (body, status) = text.rsplit_once('\n').ok_or("model_response_invalid")?;
    if status != "200" {
        return Err(match status {
            "401" | "403" => "model_auth_failed",
            "429" => "model_rate_limited",
            _ => "model_http_failed",
        }
        .into());
    }
    parse_response(body)
}

fn parse_response(body: &str) -> Result<String, String> {
    let bad = || "model_response_invalid".to_owned();
    if body.len() > RESPONSE_LIMIT {
        return Err("model_response_too_large".into());
    }
    let value: Value = serde_json::from_str(body).map_err(|_| bad())?;
    let choices = value
        .get("choices")
        .and_then(Value::as_array)
        .ok_or_else(bad)?;
    if choices.len() != 1 || choices[0]["finish_reason"] != "stop" {
        return Err(bad());
    }
    let message = choices[0]
        .get("message")
        .and_then(Value::as_object)
        .ok_or_else(bad)?;
    if message.get("role").and_then(Value::as_str) != Some("assistant")
        || message.get("refusal").is_some_and(|value| !value.is_null())
        || message
            .get("tool_calls")
            .is_some_and(|value| !value.is_null())
        || message
            .get("function_call")
            .is_some_and(|value| !value.is_null())
    {
        return Err(bad());
    }
    let content = message
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(bad)?;
    if content.trim().is_empty() || content.len() > CONTENT_LIMIT || content.contains('\0') {
        return Err(bad());
    }
    Ok(content.to_owned())
}

#[cfg(test)]
mod tests;
