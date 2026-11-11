//! A bounded command-line reference host using the public embedding API.
#![forbid(unsafe_code)]

use std::{
    cell::RefCell,
    ffi::OsString,
    fs::File,
    io::{self, Read, Write},
    path::PathBuf,
    process::ExitCode,
};

use leselang_vm::embedding::{
    Fuel, HostBuilder, HostFailure, Limits, Poll, ScalarType, ScalarValue, parameter,
};

mod diagnostics;

const DEFAULT_FUEL: u64 = 10_000;
const HELP: &str = "LeseLang — bounded local language runner

Usage:
  leselang check FILE [--allow stdio]
  leselang run FILE [--allow stdio] [--fuel N]
  leselang --help
  leselang --version

FILE may be - to read UTF-8 source from standard input. Use -- before a file
whose name starts with a hyphen. Options may appear before or after FILE.

check compiles all declarations without running any host operation.
run evaluates main; std.print(message: string) requires --allow stdio.
Non-none final values are printed as JSON literals; std.print writes text lines.

The source limit is 16384 bytes and the default fuel budget is 10000.
Fuel is shared across language slices and charged host output; it never refills.
Exit status: 0 success, 1 source/I/O/execution failure, 2 command-line error.
";

#[derive(Clone, Copy, Debug, PartialEq)]
enum Command {
    Check,
    Run,
}

#[derive(Debug)]
struct Options {
    command: Command,
    input: PathBuf,
    stdio: bool,
    fuel: u64,
}

enum Action {
    Help,
    Version,
    Execute(Options),
}

fn arguments(args: impl IntoIterator<Item = OsString>) -> Result<Action, String> {
    let mut args = args.into_iter();
    let first = args.next().ok_or("expected check or run; see --help")?;
    if first == "--help" || first == "-h" {
        if args.next().is_some() {
            return Err("--help does not take arguments".into());
        }
        return Ok(Action::Help);
    }
    if first == "--version" || first == "-V" {
        if args.next().is_some() {
            return Err("--version does not take arguments".into());
        }
        return Ok(Action::Version);
    }
    let command = match first.to_str() {
        Some("check") => Command::Check,
        Some("run") => Command::Run,
        _ => return Err("expected check or run; see --help".into()),
    };
    let mut input = None;
    let mut stdio = false;
    let mut fuel = None;
    let mut positional_only = false;
    while let Some(argument) = args.next() {
        if !positional_only {
            if argument == "--" {
                positional_only = true;
                continue;
            }
            if argument == "--help" || argument == "-h" {
                if input.is_some() || stdio || fuel.is_some() || args.next().is_some() {
                    return Err("use check --help or run --help without other arguments".into());
                }
                return Ok(Action::Help);
            }
            if argument == "--allow" {
                if stdio {
                    return Err("--allow may be supplied only once".into());
                }
                if args.next().as_deref() != Some(std::ffi::OsStr::new("stdio")) {
                    return Err("--allow requires the capability stdio".into());
                }
                stdio = true;
                continue;
            }
            if argument == "--fuel" {
                if command != Command::Run {
                    return Err("--fuel applies only to run".into());
                }
                if fuel.is_some() {
                    return Err("--fuel may be supplied only once".into());
                }
                let value = args.next().ok_or("--fuel requires a decimal u64 budget")?;
                let value = value
                    .to_str()
                    .ok_or("--fuel requires a decimal u64 budget")?;
                if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                    return Err("--fuel requires a decimal u64 budget".into());
                }
                fuel = Some(value.parse().map_err(|_| "--fuel exceeds the u64 range")?);
                continue;
            }
            if argument != "-" && argument.as_encoded_bytes().starts_with(b"-") {
                return Err(
                    "unknown option; see --help (use -- before a hyphenated filename)".into(),
                );
            }
        }
        if input.replace(PathBuf::from(argument)).is_some() {
            return Err("expected exactly one source file".into());
        }
    }
    Ok(Action::Execute(Options {
        command,
        input: input.ok_or("missing source file (use - for standard input)")?,
        stdio,
        fuel: fuel.unwrap_or(DEFAULT_FUEL),
    }))
}

fn read_source(input: &mut dyn Read, limit: usize) -> Result<String, String> {
    // Stop before allocating/reading unbounded input, including streams and pipes.
    let mut bytes = Vec::new();
    input
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read source: {error}"))?;
    if bytes.len() > limit {
        return Err(format!("source exceeds the {limit}-byte limit"));
    }
    String::from_utf8(bytes).map_err(|error| {
        format!(
            "source is not UTF-8 (invalid byte at offset {})",
            error.utf8_error().valid_up_to()
        )
    })
}

fn execute(options: &Options, output: &mut dyn Write) -> Result<(), String> {
    let source = if options.input == std::path::Path::new("-") {
        read_source(&mut io::stdin().lock(), Limits::default().max_source_bytes)
    } else {
        File::open(&options.input)
            .map_err(|error| format!("cannot open source: {error}"))
            .and_then(|mut file| read_source(&mut file, Limits::default().max_source_bytes))
    }
    .map_err(|error| {
        format!(
            "{}: error: {error}",
            diagnostics::display_path(&options.input.to_string_lossy())
        )
    })?;
    execute_source(options, &source, output)
}

fn execute_source(options: &Options, source: &str, output: &mut dyn Write) -> Result<(), String> {
    let output = RefCell::new(output);
    let mut builder = HostBuilder::new();
    builder
        .register(
            "std.print",
            vec![parameter("message", ScalarType::String)],
            ScalarType::None,
            "stdio",
            |arguments, fuel| {
                let Some(ScalarValue::String(message)) = arguments.get("message") else {
                    return Err(HostFailure::Failed(
                        "std.print requires a string message".into(),
                    ));
                };
                fuel.charge(message.len() as u64 + 1)
                    .map_err(|_| HostFailure::Failed("fuel exhausted before std.print".into()))?;
                let mut output = output.borrow_mut();
                output
                    .write_all(message.as_bytes())
                    .and_then(|()| output.write_all(b"\n"))
                    .and_then(|()| output.flush())
                    // A write/flush error can occur after visible output; never retry it.
                    .map_err(|error| {
                        HostFailure::Uncertain(format!("standard output failed: {error}"))
                    })?;
                Ok(ScalarValue::None)
            },
        )
        .map_err(|error| format!("cannot register CLI host: {error}"))?;
    let host = builder.build();
    let grants: &[&str] = if options.stdio { &["stdio"] } else { &[] };
    let path = options.input.to_string_lossy();
    let program = host
        .compile(source, grants)
        .map_err(|error| diagnostics::render_compile(&path, source, &error))?;
    if options.command == Command::Check {
        return Ok(());
    }
    let path = diagnostics::display_path(&path);
    let mut session = host
        .start(&program, grants, Fuel::new(options.fuel))
        .map_err(|error| diagnostics::render_runtime(&error))?;
    let outcome = (|| {
        loop {
            match session
                .poll()
                .map_err(|error| diagnostics::render_runtime(&error))?
            {
                Poll::Request(request) => {
                    // The CLI is the explicit synchronous host driver; the library
                    // does not schedule, dispatch, accept, or resume on its own.
                    let authorized = session
                        .authorize(request)
                        .map_err(|rejected| diagnostics::render_runtime(&rejected.error))?;
                    let receipt = session
                        .dispatch(authorized)
                        .map_err(|failure| diagnostics::render_runtime(&failure.error))?;
                    session
                        .accept(receipt)
                        .map_err(|rejected| diagnostics::render_runtime(&rejected.error))?;
                }
                Poll::Value(value) => {
                    if value != ScalarValue::None {
                        let mut rendered = render_value(&value);
                        rendered.push('\n');
                        let mut output = output.borrow_mut();
                        output
                            .write_all(rendered.as_bytes())
                            .and_then(|()| output.flush())
                            .map_err(|error| {
                                format!("cannot write result: {error}; output may be partial")
                            })?;
                    }
                    return Ok(());
                }
                Poll::Awaiting => return Err("CLI host lost its pending request or receipt".into()),
                Poll::Terminal(end) => return Err(format!("execution ended: {end:?}")),
            }
        }
    })();
    outcome.map_err(|message: String| {
        let message = message.trim_end();
        let prefix = if message.starts_with("error[") { "" } else { "error: " };
        format!(
            "{path}: {prefix}{message}\nnote: execution stopped with {} fuel remaining; completed output is not rolled back",
            session.fuel_remaining(),
        )
    })
}

// Bounded scalar presentation, not a new language serializer or protocol.
fn render_value(value: &ScalarValue) -> String {
    match value {
        ScalarValue::Integer(value) => value.to_string(),
        ScalarValue::Boolean(value) => value.to_string(),
        ScalarValue::String(value) => quoted(value),
        ScalarValue::None => "null".into(),
        ScalarValue::OptionalString(value) => {
            value.0.as_deref().map_or_else(|| "null".into(), quoted)
        }
        ScalarValue::StringList(value) => {
            format!(
                "[{}]",
                value
                    .0
                    .iter()
                    .map(|item| quoted(item))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    }
}

fn quoted(text: &str) -> String {
    use std::fmt::Write;
    let mut output = String::from("\"");
    for ch in text.chars() {
        match ch {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            '\u{8}' => output.push_str("\\b"),
            '\u{c}' => output.push_str("\\f"),
            ch if ch < ' ' => write!(output, "\\u{:04x}", ch as u32).expect("writing to String"),
            ch => output.push(ch),
        }
    }
    output.push('"');
    output
}

fn main() -> ExitCode {
    let action = match arguments(std::env::args_os().skip(1)) {
        Ok(action) => action,
        Err(message) => {
            let _ = writeln!(io::stderr().lock(), "error: {message}");
            return ExitCode::from(2);
        }
    };
    let mut output = io::stdout().lock();
    let result = match action {
        Action::Help => output
            .write_all(HELP.as_bytes())
            .and_then(|()| output.flush())
            .map_err(|error| format!("cannot write help: {error}")),
        Action::Version => writeln!(output, "leselang {}", env!("CARGO_PKG_VERSION"))
            .and_then(|()| output.flush())
            .map_err(|error| format!("cannot write version: {error}")),
        Action::Execute(options) => execute(&options, &mut output),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            let _ = writeln!(io::stderr().lock(), "{message}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct PartialWriter {
        bytes: Vec<u8>,
        remaining: usize,
        fail_flush: bool,
    }
    impl Write for PartialWriter {
        fn write(&mut self, input: &[u8]) -> io::Result<usize> {
            if self.remaining == 0 {
                return Err(io::Error::other("injected write failure"));
            }
            let count = input.len().min(self.remaining);
            self.bytes.extend_from_slice(&input[..count]);
            self.remaining -= count;
            Ok(count)
        }
        fn flush(&mut self) -> io::Result<()> {
            if self.fail_flush {
                Err(io::Error::other("injected flush failure"))
            } else {
                Ok(())
            }
        }
    }

    fn run_options() -> Options {
        Options {
            command: Command::Run,
            input: "test.lese".into(),
            stdio: true,
            fuel: DEFAULT_FUEL,
        }
    }

    #[test]
    fn partial_output_and_flush_failure_are_uncertain_and_never_retried() {
        let source = "fn main() = bind(first: std.print(message: \"first\"), body: std.print(message: \"second\"))";
        for remaining in 0..13 {
            let mut output = PartialWriter {
                bytes: vec![],
                remaining,
                fail_flush: false,
            };
            let error = execute_source(&run_options(), source, &mut output).unwrap_err();
            assert!(error.contains("uncertain"), "{error}");
            assert_eq!(output.bytes, b"first\nsecond\n"[..remaining]);
        }
        let mut output = PartialWriter {
            bytes: vec![],
            remaining: 100,
            fail_flush: true,
        };
        let error = execute_source(&run_options(), source, &mut output).unwrap_err();
        assert!(error.contains("uncertain"), "{error}");
        assert_eq!(output.bytes, b"first\n");
    }

    #[test]
    fn result_output_failure_does_not_replay_completed_effects() {
        let source = "fn main() = bind(first: std.print(message: \"done\"), body: 42)";
        let mut output = PartialWriter {
            bytes: vec![],
            remaining: 6,
            fail_flush: false,
        };
        let error = execute_source(&run_options(), source, &mut output).unwrap_err();
        assert!(error.contains("cannot write result"), "{error}");
        assert_eq!(output.bytes, b"done\n4");
    }

    #[test]
    fn source_reader_never_reads_past_limit_plus_one() {
        let mut bytes = io::Cursor::new(vec![b' '; 100_000]);
        assert!(read_source(&mut bytes, 100).is_err());
        assert_eq!(bytes.position(), 101);
    }

    #[test]
    fn string_presentation_is_valid_json_for_control_and_unicode_characters() {
        let input = "\"\\\n\r\t\u{8}\u{c}\0中文🙂";
        let rendered = quoted(input);
        assert_eq!(serde_json::from_str::<String>(&rendered).unwrap(), input);
    }
}
