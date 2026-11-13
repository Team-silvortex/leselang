use std::{
    ffi::OsString,
    io::{Read, Write},
    path::PathBuf,
    process::ExitCode,
};

#[cfg(all(target_os = "macos", feature = "desktop"))]
mod agent_transport;
#[cfg(all(target_os = "macos", feature = "desktop"))]
mod transport;
#[cfg(all(target_os = "macos", feature = "desktop"))]
mod transport_core;

const HELP: &str = "LeseLang TypeScript/Tauri host
  --describe                 Print the offline registered capability catalog
  --describe-agent           Print the offline interactive task catalog
  --agent                    Open the interactive agent workflow application
  --run FILE                 Run bounded UTF-8 source (default: reference form)
  --text TEXT                Supply the typed main(text: string) input
  --report-json              Print one JSON result record
  --hold                     Keep the WebView open after the task finishes
GUI execution requires macOS and the desktop feature. No action is retried.
";

#[derive(Debug, PartialEq)]
enum Options {
    Help,
    Describe,
    DescribeAgent,
    Agent,
    Run {
        source: Option<PathBuf>,
        text: String,
        report_json: bool,
        hold: bool,
    },
}

fn options(args: impl IntoIterator<Item = OsString>) -> Result<Options, String> {
    let mut args = args.into_iter().peekable();
    if args.peek().is_some_and(|arg| {
        arg == "--help"
            || arg == "-h"
            || arg == "--describe"
            || arg == "--describe-agent"
            || arg == "--agent"
    }) {
        let mode = args.next().unwrap();
        if args.next().is_some() {
            return Err(
                "--help, --describe, --describe-agent and --agent must be used alone".into(),
            );
        }
        return Ok(if mode == "--describe" {
            Options::Describe
        } else if mode == "--describe-agent" {
            Options::DescribeAgent
        } else if mode == "--agent" {
            Options::Agent
        } else {
            Options::Help
        });
    }
    let mut source = None;
    let mut text = None;
    let mut report_json = false;
    let mut hold = false;
    while let Some(arg) = args.next() {
        if arg == "--run" {
            let path = PathBuf::from(args.next().ok_or("--run requires FILE")?);
            if source.replace(path).is_some() {
                return Err("duplicate --run".into());
            }
        } else if arg == "--text" {
            let value = args
                .next()
                .ok_or("--text requires TEXT")?
                .into_string()
                .map_err(|_| "--text requires UTF-8")?;
            if value.len() > 4096 || value.contains('\0') {
                return Err("--text exceeds 4096 UTF-8 bytes or contains NUL".into());
            }
            if text.replace(value).is_some() {
                return Err("duplicate --text".into());
            }
        } else if arg == "--report-json" {
            if report_json {
                return Err("duplicate --report-json".into());
            }
            report_json = true;
        } else if arg == "--hold" {
            if hold {
                return Err("duplicate --hold".into());
            }
            hold = true;
        } else {
            return Err("unknown argument; use --help".into());
        }
    }
    Ok(Options::Run {
        source,
        text: text.unwrap_or_else(|| "TypeScript ✓".into()),
        report_json,
        hold,
    })
}

fn source(path: Option<PathBuf>) -> Result<String, String> {
    if let Some(path) = path {
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .map_err(|error| format!("cannot open source: {error}"))?
            .take(16 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| format!("cannot read source: {error}"))?;
        if bytes.len() > 16 * 1024 {
            return Err("source exceeds 16384 UTF-8 bytes".into());
        }
        String::from_utf8(bytes).map_err(|_| "source is not UTF-8".into())
    } else {
        Ok(include_str!("../examples/form.lese").into())
    }
}

fn execute(options: Options) -> Result<(), String> {
    match options {
        Options::Help => {
            print!("{HELP}");
            Ok(())
        }
        mode @ (Options::Describe | Options::DescribeAgent) => {
            let description = if mode == Options::DescribeAgent {
                leselang_tauri_host::describe_agent()?
            } else {
                leselang_tauri_host::describe()?
            };
            let mut output = std::io::stdout().lock();
            output
                .write_all(description.as_bytes())
                .and_then(|()| output.flush())
                .map_err(|error| format!("cannot write catalog: {error}; output may be partial"))
        }
        Options::Agent => {
            #[cfg(all(target_os = "macos", feature = "desktop"))]
            return agent_transport::launch();
            #[cfg(not(all(target_os = "macos", feature = "desktop")))]
            Err("agent application requires macOS and the desktop feature".into())
        }
        Options::Run {
            source: path,
            text,
            report_json,
            hold,
        } => {
            let source = source(path)?;
            #[cfg(all(target_os = "macos", feature = "desktop"))]
            return transport::launch(source, text, hold, report_json);
            #[cfg(not(all(target_os = "macos", feature = "desktop")))]
            {
                let _ = (source, text, hold, report_json);
                Err("GUI execution requires macOS and the desktop feature".into())
            }
        }
    }
}

fn main() -> ExitCode {
    let options = match options(std::env::args_os().skip(1)) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("Tauri host: {error}");
            return ExitCode::from(2);
        }
    };
    match execute(options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Tauri host: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metadata_flags_never_accept_execution_options_and_text_is_literal() {
        let parse = |args: &[&str]| options(args.iter().map(OsString::from));
        assert_eq!(parse(&["--describe"]).unwrap(), Options::Describe);
        assert_eq!(
            parse(&["--describe-agent"]).unwrap(),
            Options::DescribeAgent
        );
        assert_eq!(parse(&["--agent"]).unwrap(), Options::Agent);
        for args in [
            vec!["--describe", "--hold"],
            vec!["--describe-agent", "--hold"],
            vec!["--describe-agent", "--text", "literal"],
            vec!["--describe-agent", "--run", "missing"],
            vec!["--describe-agent", "--describe"],
            vec!["--describe", "--describe-agent"],
            vec!["--agent", "--hold"],
            vec!["--agent", "--text", "value"],
            vec!["--agent", "--run", "missing"],
            vec!["--describe", "--run", "missing"],
            vec!["--run"],
            vec!["--text"],
            vec!["--hold", "--hold"],
            vec!["--report-json", "--report-json"],
        ] {
            assert!(parse(&args).is_err(), "{args:?}");
        }
        assert_eq!(
            parse(&["--text", "--hold"]).unwrap(),
            Options::Run {
                source: None,
                text: "--hold".into(),
                report_json: false,
                hold: false
            }
        );
        assert!(parse(&["--text", &"x".repeat(4097)]).is_err());
    }
}
