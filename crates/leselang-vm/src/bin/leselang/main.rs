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
    CompileError, Error, Fuel, Host, HostBuilder, HostFailure, Limits, Poll, RegistrationError,
    ScalarType, ScalarValue, diagnostics as diagnostic_view, parameter,
};

mod diagnostics;

const DEFAULT_FUEL: u64 = 10_000;
const HELP: &str = "LeseLang — bounded local language runner

Usage:
  leselang describe [--diagnostic-format text|json]
  leselang check FILE [--entry NAME] [--text NAME VALUE]... [--allow stdio] [--diagnostic-format text|json]
  leselang run FILE [--entry NAME] [--text NAME VALUE]... [--allow stdio] [--fuel N] [--diagnostic-format text|json]
  leselang fmt FILE [--check] [--diagnostic-format text|json]
  leselang --help
  leselang --version

FILE may be - to read UTF-8 source from standard input. Use -- before a file
whose name starts with a hyphen. Options may appear before or after FILE.

check compiles all declarations without running any host operation.
describe writes the registered host catalog as versioned JSON to stdout.
It takes no source file or grants, and invokes no host operation.
run evaluates main by default; --entry NAME selects another function.
--text supplies required string task inputs without interpreting VALUE as source.
Without --text, the selected entry must take no parameters. Rust hosts also support
integer, boolean, none, optional_string and string_list task inputs.
All declarations are checked; no function may call the selected entry.
std.print(message: string) requires --allow stdio.
fmt preserves source comments and writes formatted source to stdout, never to FILE.
fmt --check is quiet on matching source and fails if formatting would change it.
Formatting checks syntax only; --entry, --text, --allow and --fuel do not apply to fmt.
Non-none final values are printed as JSON literals; std.print writes text lines.
Diagnostics go to stderr; json emits one schema-versioned object per line.
Place --diagnostic-format json immediately after describe/check/run/fmt for JSON usage errors.

Source and formatted output are limited to 16384 bytes; default run fuel is 10000.
Fuel is shared across language slices and charged host output; it never refills.
Exit status: 0 success, 1 source/I/O/execution/format failure, 2 command-line error.
";

#[derive(Clone, Copy, Debug, PartialEq)]
enum Command {
    Check,
    Run,
    Format,
}

#[derive(Debug)]
struct Options {
    command: Command,
    input: PathBuf,
    entry: String,
    text_arguments: Vec<(String, String)>,
    stdio: bool,
    fuel: u64,
    diagnostic_format: diagnostics::Format,
    format_check: bool,
}

enum Action {
    Help,
    Version,
    Describe(diagnostics::Format),
    Execute(Options),
}

fn arguments(
    args: impl IntoIterator<Item = OsString>,
    diagnostic_format: &mut diagnostics::Format,
) -> Result<Action, String> {
    let mut args = args.into_iter();
    let first = args
        .next()
        .ok_or("expected describe, check, run or fmt; see --help")?;
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
    if first == "describe" {
        return describe_arguments(args, diagnostic_format);
    }
    let command = match first.to_str() {
        Some("check") => Command::Check,
        Some("run") => Command::Run,
        Some("fmt") => Command::Format,
        _ => return Err("expected describe, check, run or fmt; see --help".into()),
    };
    let mut input = None;
    let mut entry = None;
    let mut text_arguments = Vec::new();
    let mut stdio = false;
    let mut fuel = None;
    let mut positional_only = false;
    let mut format_supplied = false;
    let mut format_check = false;
    while let Some(argument) = args.next() {
        if !positional_only {
            if argument == "--" {
                positional_only = true;
                continue;
            }
            if argument == "--help" || argument == "-h" {
                if input.is_some()
                    || entry.is_some()
                    || !text_arguments.is_empty()
                    || stdio
                    || fuel.is_some()
                    || format_supplied
                    || format_check
                    || args.next().is_some()
                {
                    return Err("use check/run/fmt --help without other arguments".into());
                }
                return Ok(Action::Help);
            }
            if argument == "--diagnostic-format" {
                if format_supplied {
                    return Err("--diagnostic-format may be supplied only once".into());
                }
                *diagnostic_format = match args.next().as_deref().and_then(|value| value.to_str()) {
                    Some("text") => diagnostics::Format::Text,
                    Some("json") => diagnostics::Format::Json,
                    _ => return Err("--diagnostic-format requires text or json".into()),
                };
                format_supplied = true;
                continue;
            }
            if argument == "--allow" {
                if command == Command::Format {
                    return Err("--allow does not apply to fmt".into());
                }
                if stdio {
                    return Err("--allow may be supplied only once".into());
                }
                if args.next().as_deref() != Some(std::ffi::OsStr::new("stdio")) {
                    return Err("--allow requires the capability stdio".into());
                }
                stdio = true;
                continue;
            }
            if argument == "--entry" {
                if command == Command::Format {
                    return Err("--entry does not apply to fmt".into());
                }
                if entry.is_some() {
                    return Err("--entry may be supplied only once".into());
                }
                let value = args.next().ok_or("--entry requires a function name")?;
                let name = value
                    .to_str()
                    .filter(|name| leselang_vm::native::hir::pure_typing::valid_local_name(name))
                    .ok_or("--entry requires a non-reserved ASCII identifier of 1 to 64 bytes")?;
                entry = Some(name.to_owned());
                continue;
            }
            if argument == "--text" {
                if command == Command::Format {
                    return Err("--text does not apply to fmt".into());
                }
                if text_arguments.len() >= leselang_vm::native::syntax::MAX_FUNCTION_PARAMETERS {
                    return Err("too many --text inputs (maximum 8)".into());
                }
                let name = args.next().ok_or("--text requires NAME VALUE")?;
                let name = name
                    .to_str()
                    .filter(|name| leselang_vm::native::hir::pure_typing::valid_local_name(name))
                    .ok_or(
                        "--text requires a non-reserved ASCII parameter name of 1 to 64 bytes",
                    )?;
                if text_arguments.iter().any(|(prior, _)| prior == name) {
                    return Err("--text parameter names must be unique".into());
                }
                let value = args.next().ok_or("--text requires NAME VALUE")?;
                let value = value.to_str().ok_or("--text VALUE must be UTF-8")?;
                if value.len() > leselang_vm::native::core::MAX_SCALAR_STRING_BYTES {
                    return Err("--text VALUE exceeds the 4096-byte string limit".into());
                }
                text_arguments.push((name.to_owned(), value.to_owned()));
                continue;
            }
            if argument == "--check" {
                if command != Command::Format {
                    return Err("--check applies only to fmt".into());
                }
                if format_check {
                    return Err("--check may be supplied only once".into());
                }
                format_check = true;
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
        entry: entry.unwrap_or_else(|| "main".into()),
        text_arguments,
        stdio,
        fuel: fuel.unwrap_or(DEFAULT_FUEL),
        diagnostic_format: *diagnostic_format,
        format_check,
    }))
}

fn describe_arguments(
    mut args: impl Iterator<Item = OsString>,
    format: &mut diagnostics::Format,
) -> Result<Action, String> {
    let mut supplied = false;
    while let Some(argument) = args.next() {
        if argument == "--help" || argument == "-h" {
            if supplied || args.next().is_some() {
                return Err("use describe --help without other arguments".into());
            }
            return Ok(Action::Help);
        }
        if argument != "--diagnostic-format" {
            return Err(
                "describe takes only --diagnostic-format text|json; no source or execution options"
                    .into(),
            );
        }
        if supplied {
            return Err("--diagnostic-format may be supplied only once".into());
        }
        *format = match args.next().as_deref().and_then(|value| value.to_str()) {
            Some("text") => diagnostics::Format::Text,
            Some("json") => diagnostics::Format::Json,
            _ => return Err("--diagnostic-format requires text or json".into()),
        };
        supplied = true;
    }
    Ok(Action::Describe(*format))
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
        options.diagnostic_format.other(
            Some(&options.input.to_string_lossy()),
            "input",
            "LSI0001",
            &error,
        )
    })?;
    execute_source(options, &source, output)
}

// Discovery and execution use the same registration, including its callback.
fn cli_host<'h>(output: &'h RefCell<&mut dyn Write>) -> Result<Host<'h>, RegistrationError> {
    let mut builder = HostBuilder::new();
    builder.register(
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
    )?;
    Ok(builder.build())
}

fn describe_catalog(format: diagnostics::Format, output: &mut dyn Write) -> Result<(), String> {
    let output = RefCell::new(output);
    let host = cli_host(&output).map_err(|error| {
        format.other(
            None,
            "discovery",
            "LSD0001",
            &format!("cannot register CLI host: {error}"),
        )
    })?;
    let json = host
        .catalog_description("leselang.scalar")
        .to_json()
        .map_err(|error| format.other(None, "discovery", "LSD0001", &error.to_string()))?;
    let mut output = output.borrow_mut();
    output
        .write_all(json.as_bytes())
        .and_then(|()| output.flush())
        .map_err(|error| {
            format.other(
                None,
                "discovery",
                "LSD0002",
                &format!("cannot write catalog: {error}; output may be partial"),
            )
        })
}

fn execute_source(options: &Options, source: &str, output: &mut dyn Write) -> Result<(), String> {
    if options.command == Command::Format {
        return format_source(options, source, output);
    }
    let output = RefCell::new(output);
    let host = cli_host(&output).map_err(|error| {
        options.diagnostic_format.other(
            Some(&options.input.to_string_lossy()),
            "runtime",
            "LSR0001",
            &format!("cannot register CLI host: {error}"),
        )
    })?;
    let grants: &[&str] = if options.stdio { &["stdio"] } else { &[] };
    let path = options.input.to_string_lossy();
    let program = if options.text_arguments.is_empty() {
        host.compile_entry(source, &options.entry, grants)
    } else {
        host.compile_task(source, &options.entry, grants)
    }
    .map_err(|error| options.diagnostic_format.compile(&path, source, &error))?;
    let input_arguments = options
        .text_arguments
        .iter()
        .map(|(name, value)| (name.clone(), ScalarValue::String(value.clone())))
        .collect();
    let input_failure = |message: String| {
        options
            .diagnostic_format
            .other(Some(&path), "input", "LSI0002", &message)
    };
    if options.command == Command::Check {
        leselang_vm::embedding::bind_entry_arguments(
            program.parameters(),
            input_arguments,
            Limits::default().execution.effects.pure.max_bindings,
        )
        .map_err(|error| input_failure(error.to_string()))?;
        return Ok(());
    }
    let mut session = host
        .start_with_arguments(&program, input_arguments, grants, Fuel::new(options.fuel))
        .map_err(|error| input_failure(error.to_string()))?;
    let outcome = (|| {
        loop {
            match session
                .poll()
                .map_err(|error| diagnostic_view::runtime(&error))?
            {
                Poll::Request(request) => {
                    // The CLI is the explicit synchronous host driver; the library
                    // does not schedule, dispatch, accept, or resume on its own.
                    let authorized = session
                        .authorize(request)
                        .map_err(|rejected| diagnostic_view::runtime(&rejected.error))?;
                    let receipt = session
                        .dispatch(authorized)
                        .map_err(|failure| diagnostic_view::runtime(&failure.error))?;
                    session
                        .accept(receipt)
                        .map_err(|rejected| diagnostic_view::runtime(&rejected.error))?;
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
                                diagnostic_view::runtime(&Error::Execution(format!(
                                    "cannot write result: {error}; output may be partial"
                                )))
                            })?;
                    }
                    return Ok(());
                }
                Poll::Awaiting => {
                    return Err(diagnostic_view::runtime(&Error::Execution(
                        "CLI host lost its pending request or receipt".into(),
                    )));
                }
                Poll::Terminal(end) => {
                    return Err(diagnostic_view::runtime(&Error::Execution(format!(
                        "execution ended: {end:?}"
                    ))));
                }
            }
        }
    })();
    outcome.map_err(|diagnostic| {
        options
            .diagnostic_format
            .runtime(&path, &diagnostic, Some(session.fuel_remaining()))
    })
}

fn format_source(options: &Options, source: &str, output: &mut dyn Write) -> Result<(), String> {
    let path = options.input.to_string_lossy();
    let formatted = leselang_vm::native::syntax::format_source(source).map_err(|errors| {
        options
            .diagnostic_format
            .formatting(&path, source, &CompileError::Syntax(errors))
    })?;
    // Keep generated source admissible by this CLI, and reject atomically before
    // either writing stdout or deciding that --check found a formatting change.
    let limit = Limits::default().max_source_bytes;
    if formatted.len() > limit {
        return Err(options.diagnostic_format.other(
            Some(&path),
            "format",
            "LSF0002",
            &format!("formatted source exceeds the {limit}-byte limit"),
        ));
    }
    if options.format_check {
        return if formatted == source {
            Ok(())
        } else {
            Err(options.diagnostic_format.other(
                Some(&path),
                "format",
                "LSF0001",
                "source is not formatted; run fmt to inspect the formatted source",
            ))
        };
    }
    output
        .write_all(formatted.as_bytes())
        .and_then(|()| output.flush())
        .map_err(|error| {
            options.diagnostic_format.other(
                Some(&path),
                "format",
                "LSF0003",
                &format!("cannot write formatted source: {error}; output may be partial"),
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
    let mut diagnostic_format = diagnostics::Format::Text;
    let action = match arguments(std::env::args_os().skip(1), &mut diagnostic_format) {
        Ok(action) => action,
        Err(message) => {
            let message = diagnostic_format.other(None, "usage", "LSU0001", &message);
            let _ = io::stderr().lock().write_all(message.as_bytes());
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
        Action::Describe(format) => describe_catalog(format, &mut output),
        Action::Execute(options) => execute(&options, &mut output),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            let mut stderr = io::stderr().lock();
            let _ = stderr.write_all(message.as_bytes());
            if !message.ends_with('\n') {
                let _ = stderr.write_all(b"\n");
            }
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
            entry: "main".into(),
            text_arguments: Vec::new(),
            stdio: true,
            fuel: DEFAULT_FUEL,
            diagnostic_format: diagnostics::Format::Text,
            format_check: false,
        }
    }

    #[test]
    fn catalog_output_failures_report_partial_delivery_without_retry() {
        let mut complete = Vec::new();
        describe_catalog(diagnostics::Format::Json, &mut complete).unwrap();
        for (remaining, fail_flush, delivered) in [
            (0, false, 0),
            (17, false, 17),
            (complete.len(), true, complete.len()),
        ] {
            let mut output = PartialWriter {
                bytes: Vec::new(),
                remaining,
                fail_flush,
            };
            let error = describe_catalog(diagnostics::Format::Json, &mut output).unwrap_err();
            assert_eq!(output.bytes, complete[..delivered]);
            let record: serde_json::Value = serde_json::from_str(&error).unwrap();
            assert_eq!(record["phase"], "discovery");
            assert_eq!(record["code"], "LSD0002");
            assert!(record["path"].is_null());
            assert!(record["span"].is_null());
            assert!(record["fuel_remaining"].is_null());
            assert!(record["message"].as_str().unwrap().contains("partial"));
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
    fn json_reports_uncertain_and_result_output_failures_without_replaying_effects() {
        let mut options = run_options();
        options.diagnostic_format = diagnostics::Format::Json;
        for (source, expected, kind) in [
            (
                "fn main() = bind(first: std.print(message: \"first\"), body: std.print(message: \"second\"))",
                b"first\n".as_slice(),
                "host_uncertain",
            ),
            (
                "fn main() = bind(first: std.print(message: \"done\"), body: 42)",
                b"done\n4".as_slice(),
                "runtime",
            ),
        ] {
            let mut output = PartialWriter {
                bytes: vec![],
                remaining: 6,
                fail_flush: false,
            };
            let error = execute_source(&options, source, &mut output).unwrap_err();
            let record: serde_json::Value = serde_json::from_str(&error).unwrap();
            assert_eq!(record["schema_version"], 1);
            assert_eq!(record["kind"], kind);
            assert!(record["fuel_remaining"].as_u64().unwrap() < DEFAULT_FUEL);
            assert!(record["span"].is_null());
            assert_eq!(output.bytes, expected);
        }
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

    #[test]
    fn formatting_write_failures_report_partial_output_and_never_retry() {
        let mut options = run_options();
        options.command = Command::Format;
        options.diagnostic_format = diagnostics::Format::Json;
        for (remaining, fail_flush, expected) in [
            (5, false, b"fn ma".as_slice()),
            (100, true, b"fn main() = 42\n".as_slice()),
        ] {
            let mut output = PartialWriter {
                bytes: vec![],
                remaining,
                fail_flush,
            };
            let error = execute_source(&options, "fn main()=42", &mut output).unwrap_err();
            let record: serde_json::Value = serde_json::from_str(&error).unwrap();
            assert_eq!(record["phase"], "format");
            assert_eq!(record["code"], "LSF0003");
            assert!(record["fuel_remaining"].is_null());
            assert!(record["message"].as_str().unwrap().contains("partial"));
            assert_eq!(output.bytes, expected);
        }
        options.format_check = true;
        let mut output = PartialWriter {
            bytes: vec![],
            remaining: 0,
            fail_flush: true,
        };
        execute_source(&options, "fn main() = 42\n", &mut output).unwrap();
        assert!(output.bytes.is_empty());
    }
}
