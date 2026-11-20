//! Delivery-only primitives shared by one-shot and interactive Tauri hosts.
use leselang_tauri_host::{Scope, Transport};
use serde_json::Value;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::Receiver,
    },
    time::{Duration, Instant},
};
use tauri::ipc::Channel;

// Generate the embedded macOS Info.plist exactly once for both launch modes.
pub(crate) fn context() -> tauri::Context<tauri::Wry> {
    tauri::generate_context!()
}

pub(crate) const WIRE_LIMIT: usize = 32 * 1024;
pub(crate) const REPORT_LIMIT: usize = 128 * 1024;
pub(crate) const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
pub(crate) const EXECUTION_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Correlation {
    transport_version: u64,
    window: String,
    epoch: String,
    session: String,
    request_id: u64,
}

impl Correlation {
    pub(crate) fn from_value(value: &Value) -> Option<Self> {
        Some(Self {
            transport_version: value.get("transport_version")?.as_u64()?,
            window: value.get("window")?.as_str()?.into(),
            epoch: value.get("epoch")?.as_str()?.into(),
            session: value.get("session")?.as_str()?.into(),
            request_id: value.get("request_id")?.as_u64()?,
        })
    }
}

pub(crate) struct Endpoint {
    pub(crate) scope: Scope,
    pub(crate) outgoing: Channel<Value>,
    pub(crate) incoming: Receiver<Value>,
    pub(crate) live: Arc<AtomicBool>,
    pub(crate) pending: Arc<Mutex<Option<Correlation>>>,
    pub(crate) started: Instant,
}

impl Transport for Endpoint {
    fn exchange(&mut self, request: Value) -> Result<Value, String> {
        if !self.is_live() {
            return Err("WebView bridge has been retired".into());
        }
        bounded(&request, WIRE_LIMIT)?;
        let expected =
            Correlation::from_value(&request).ok_or("request lacks correlation fields")?;
        {
            let mut pending = self
                .pending
                .lock()
                .map_err(|_| "pending reply lock failed")?;
            if pending.is_some() {
                self.live.store(false, Ordering::Release);
                return Err("another WebView request is already pending".into());
            }
            *pending = Some(expected);
        }
        self.outgoing.send(request).map_err(|_| {
            self.live.store(false, Ordering::Release);
            "cannot deliver WebView request; outcome may be partial".to_owned()
        })?;
        let remaining = EXECUTION_TIMEOUT.saturating_sub(self.started.elapsed());
        let result = match self.incoming.recv_timeout(REQUEST_TIMEOUT.min(remaining)) {
            Ok(reply) if self.is_live() => Ok(reply),
            Ok(_) => Err("WebView retired before reply acceptance".into()),
            Err(_) => {
                self.live.store(false, Ordering::Release);
                Err("WebView reply deadline or delivery failed; outcome may be partial".into())
            }
        };
        if let Ok(mut pending) = self.pending.lock() {
            pending.take();
        }
        result
    }

    fn is_live(&self) -> bool {
        self.live.load(Ordering::Acquire) && self.started.elapsed() < EXECUTION_TIMEOUT
    }
}

pub(crate) fn bounded(value: &Value, limit: usize) -> Result<(), String> {
    if serde_json::to_vec(value)
        .map_err(|_| "cannot encode bridge value")?
        .len()
        > limit
    {
        return Err("bridge value exceeds its encoded byte limit".into());
    }
    Ok(())
}

pub(crate) fn valid_nonce(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

pub(crate) fn accept_pending(
    pending: &Mutex<Option<Correlation>>,
    reply: &Value,
) -> Result<(), String> {
    let mut pending = pending.lock().map_err(|_| "pending reply lock failed")?;
    if pending
        .as_ref()
        .is_some_and(|expected| Correlation::from_value(reply).as_ref() == Some(expected))
    {
        pending.take();
        Ok(())
    } else {
        Err("reply does not belong to the one currently pending request".into())
    }
}
