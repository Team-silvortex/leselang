//! One synchronous reference agent for declared tasks and legacy GUI contracts.
//! It consumes JSON through an executable boundary, without linking a GUI host.
#[path = "catalog_agent/plan.rs"]
mod plan;
#[path = "catalog_agent/process.rs"]
mod process;

use std::{
    ffi::OsString,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::ExitCode,
    time::Duration,
};

use leselang_runtime_core::discovery::semantics::ExecutionStyle;
use serde_json::json;

const HELP: &str = "LeseLang catalog agent — reference cross-application GUI workflows
Usage: catalog_agent --host EXECUTABLE [--host-arg SELECTOR] --output NEW_DIRECTORY
                     [--task TASK] [--text VALUE] [--execute]

Read the actual offline catalog and validate its declared semantic contract before planning.
Task defaults to fill_and_save; --text is required only when its declaration has an input.
Old catalogs without semantics retain the known form and Calculator strategies.
Repeat --host-arg for literal host selectors (for example --host-arg --calculator).
Selectors are forwarded unchanged to both description and execution, without a shell.
By default, only save the plan and source.
--execute runs that source once and verifies the result and native snapshots.
The output directory must not exist. Host paths are invoked directly, without a shell.
Text is data, limited to 4096 UTF-8 bytes; the host's original Fuel still applies.
Failures stop the workflow. Execution, output loss and timeout are never retried.
";
const HOST_TIMEOUT: Duration = Duration::from_secs(30);
const CATALOG_LIMIT: usize = 256 * 1024;
const REPORT_LIMIT: usize = 128 * 1024;

#[derive(Debug, PartialEq)]
struct Options {
    host: PathBuf,
    host_args: Vec<OsString>,
    output: PathBuf,
    task: String,
    text: Option<String>,
    execute: bool,
}

fn arguments(args: impl IntoIterator<Item = OsString>) -> Result<Option<Options>, String> {
    let mut args = args.into_iter().peekable();
    if args
        .peek()
        .is_some_and(|arg| arg == "--help" || arg == "-h")
    {
        args.next();
        return if args.next().is_none() {
            Ok(None)
        } else {
            Err("--help takes no other arguments".into())
        };
    }
    let mut host = None;
    let mut host_args = Vec::new();
    let mut output = None;
    let mut task = None;
    let mut text = None;
    let mut execute = false;
    while let Some(arg) = args.next() {
        if arg == "--host" || arg == "--output" {
            let value = PathBuf::from(args.next().ok_or("path option requires a value")?);
            if value.as_os_str().is_empty() {
                return Err("paths must not be empty".into());
            }
            let slot = if arg == "--host" {
                &mut host
            } else {
                &mut output
            };
            if slot.replace(value).is_some() {
                return Err("duplicate path option".into());
            }
        } else if arg == "--host-arg" {
            let value = args
                .next()
                .ok_or("--host-arg requires one literal selector argument")?
                .into_string()
                .map_err(|_| "host selector arguments must be UTF-8")?;
            if host_args.len() >= 8
                || value.is_empty()
                || value.len() > 1024
                || value.contains('\0')
                || value == "--"
                || [
                    "--describe",
                    "--run",
                    "--entry",
                    "--text",
                    "--report-json",
                    "--hold",
                    "--help",
                ]
                .iter()
                .any(|flag| value == *flag || value.starts_with(&format!("{flag}=")))
            {
                return Err(
                    "host selector exceeds bounds or collides with agent execution flags".into(),
                );
            }
            host_args.push(value.into());
        } else if arg == "--task" {
            let value = args.next().ok_or("--task requires a task name")?;
            let value = value.into_string().map_err(|_| "task must be UTF-8")?;
            if value.is_empty()
                || value.len() > 64
                || !value.as_bytes()[0].is_ascii_alphabetic() && value.as_bytes()[0] != b'_'
                || !value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                || matches!(value.as_str(), "fn" | "true" | "false" | "none" | "body")
                || value.starts_with("_lese_")
            {
                return Err("task must be a nonreserved identifier of at most 64 bytes".into());
            }
            if task.replace(value).is_some() {
                return Err("duplicate --task".into());
            }
        } else if arg == "--text" {
            let value = args
                .next()
                .ok_or("--text requires a value")?
                .into_string()
                .map_err(|_| "--text must be UTF-8")?;
            if value.len() > 4096 || value.contains('\0') {
                return Err(
                    "--text must fit 4096 UTF-8 bytes and contain no NUL for process arguments"
                        .into(),
                );
            }
            if text.replace(value).is_some() {
                return Err("duplicate --text".into());
            }
        } else if arg == "--execute" {
            if execute {
                return Err("duplicate --execute".into());
            }
            execute = true;
        } else {
            return Err("unknown argument; use --help".into());
        }
    }
    let task = task.unwrap_or_else(|| "fill_and_save".into());
    Ok(Some(Options {
        host: host.ok_or("--host is required")?,
        host_args,
        output: output.ok_or("--output is required")?,
        task,
        text,
        execute,
    }))
}

fn save(directory: &Path, name: &str, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory.join(name))
        .map_err(|error| format!("cannot create {name}: {error}"))?;
    file.write_all(bytes)
        .and_then(|()| file.flush())
        .map_err(|error| format!("cannot write {name}: {error}; artifact may be partial"))
}

fn save_json(directory: &Path, name: &str, value: serde_json::Value) -> Result<(), String> {
    let mut bytes = serde_json::to_vec_pretty(&value).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    save(directory, name, &bytes)
}

#[derive(Debug)]
struct Progress {
    phase: &'static str,
    execution_started: bool,
}

fn workflow(options: &Options, directory: &Path, progress: &mut Progress) -> Result<(), String> {
    progress.phase = "discovery";
    let mut description_args = options.host_args.clone();
    description_args.push("--describe".into());
    let discovery = process::capture(
        &options.host,
        &description_args,
        CATALOG_LIMIT,
        HOST_TIMEOUT,
    )?;
    save(directory, "catalog.json", &discovery.stdout)?;
    save(directory, "discovery.stderr", &discovery.stderr)?;
    discovery.require_success()?;

    progress.phase = "planning";
    let prepared = plan::prepare(&discovery.stdout, &options.task)?;
    if prepared.takes_text != options.text.is_some() {
        return Err(if prepared.takes_text {
            "the declared task requires --text"
        } else {
            "the declared task takes no --text argument"
        }
        .into());
    }
    let source = &prepared.source;
    // Syntax is checked locally too; the actual host compiler remains authoritative.
    if !leselang_syntax::parse(source).diagnostics.is_empty() {
        return Err("generated source failed syntax validation".into());
    }
    save(directory, "program.lese", source.as_bytes())?;
    save_json(
        directory,
        "plan.json",
        json!({
            "schema_version": 1,
            "profile": prepared.profile,
            "task": prepared.task,
            "entry": prepared.entry,
            "arguments": if let Some(name) = &prepared.input_name { json!({name: options.text}) } else { json!({}) },
            "planning_contract": if prepared.semantic { "catalog_semantics_v1" } else { "legacy_profile" },
            "host_arguments": options.host_args.iter().map(|arg| arg.to_str().expect("validated host selector")).collect::<Vec<_>>(),
            "required_capabilities": prepared.required_capabilities,
            "expected_effects": prepared.expected_effects,
            "source": "program.lese",
            "execution_requested": options.execute,
        }),
    )?;
    if !options.execute {
        return save_json(
            directory,
            "verification.json",
            json!({"schema_version":1,"status":"planned","phase":"planning",
                "execution_started":false,"possible_partial_effects":false}),
        );
    }

    progress.phase = "execution";
    // Once a run is launched, a missing report is not evidence of zero effects.
    progress.execution_started = true;
    let mut arguments = options.host_args.clone();
    arguments.extend([
        "--run".into(),
        directory.join("program.lese").into_os_string(),
    ]);
    match prepared.execution {
        ExecutionStyle::NamedText => arguments.extend([
            "--entry".into(),
            prepared.entry.clone().into(),
            "--text".into(),
            prepared
                .input_name
                .as_ref()
                .expect("validated named input")
                .into(),
            options.text.as_ref().expect("validated task text").into(),
        ]),
        ExecutionStyle::Text => arguments.extend([
            "--text".into(),
            options.text.as_ref().expect("validated task text").into(),
        ]),
        ExecutionStyle::NoText => {}
    }
    arguments.push("--report-json".into());
    let execution = process::capture(&options.host, &arguments, REPORT_LIMIT, HOST_TIMEOUT)?;
    save(directory, "run.json", &execution.stdout)?;
    save(directory, "run.stderr", &execution.stderr)?;
    execution.require_success()?;

    progress.phase = "verification";
    let verified = prepared.verify(&execution.stdout, options.text.as_deref().unwrap_or(""))?;
    save_json(
        directory,
        "verification.json",
        json!({
            "schema_version":1,"status":"verified","phase":"verification",
            "execution_started":true,"possible_partial_effects":false,
            "effects":verified.effects,"fuel_remaining":verified.fuel_remaining,
            "checks":prepared.checks,
        }),
    )
}

fn run(options: Options) -> Result<PathBuf, String> {
    // No overwrite, reuse or resume: a new invocation cannot replay an old run silently.
    fs::create_dir(&options.output)
        .map_err(|error| format!("cannot create new output directory: {error}"))?;
    let directory = options
        .output
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let mut progress = Progress {
        phase: "discovery",
        execution_started: false,
    };
    if let Err(error) = workflow(&options, &directory, &mut progress) {
        let failure = save_json(
            &directory,
            "verification.json",
            json!({
                "schema_version":1,"status":"failed","phase":progress.phase,
                "execution_started":progress.execution_started,
                "possible_partial_effects":progress.execution_started,
                "error":error,
            }),
        );
        let suffix = failure
            .err()
            .map(|error| format!("; failure record: {error}"))
            .unwrap_or_default();
        return Err(format!(
            "{} failed: {error}{suffix}; no retry; artifacts: {}",
            progress.phase,
            directory.display()
        ));
    }
    Ok(directory)
}

fn main() -> ExitCode {
    let options = match arguments(std::env::args_os().skip(1)) {
        Ok(None) => {
            print!("{HELP}");
            return ExitCode::SUCCESS;
        }
        Ok(Some(options)) => options,
        Err(error) => {
            eprintln!("catalog agent: {error}");
            return ExitCode::from(2);
        }
    };
    let execute = options.execute;
    match run(options) {
        Ok(directory) => {
            println!(
                "{}; artifacts: {}",
                if execute {
                    "Verified GUI workflow"
                } else {
                    "Plan generated; no execution requested"
                },
                directory.display()
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("catalog agent: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
#[path = "catalog_agent/tests.rs"]
mod tests;
