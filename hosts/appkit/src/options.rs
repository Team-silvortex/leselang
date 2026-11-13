//! Host CLI ingress and scalar display; independent of AppKit and its main thread.
use leselang_vm::native::core::ScalarValue;
use std::{
    ffi::OsString,
    io::Read,
    path::{Path, PathBuf},
};

pub const MAX_SOURCE_BYTES: usize = 16 * 1024;
pub const HELP: &str = "LeseLang AppKit host\n  --demo [--hold]         Discover controls, fill input, submit and verify (default)\n  --button-demo [--hold]  Run the original button-only demo\n  --run FILE [--hold]     Run a bounded UTF-8 LeseLang source file\n  --self-test            Run real GUI conformance on the main thread\nRequires macOS 14+ and a logged-in graphical desktop.";
#[derive(Debug, Eq, PartialEq)]
pub enum Mode {
    Form,
    Buttons,
    Run(PathBuf),
    SelfTest,
    Help,
}
#[derive(Debug, Eq, PartialEq)]
pub struct Options {
    pub mode: Mode,
    pub hold: bool,
}
impl Options {
    pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Self, String> {
        let mut args = args.into_iter();
        let mut mode = None;
        let mut hold = false;
        while let Some(arg) = args.next() {
            if arg == "--hold" {
                if hold {
                    return Err("duplicate --hold".into());
                }
                hold = true;
                continue;
            }
            let selected = if arg == "--run" {
                Mode::Run(args.next().ok_or("--run requires a source file")?.into())
            } else if arg == "--demo" {
                Mode::Form
            } else if arg == "--button-demo" {
                Mode::Buttons
            } else if arg == "--self-test" {
                Mode::SelfTest
            } else if arg == "--help" || arg == "-h" {
                Mode::Help
            } else {
                return Err(format!("unknown argument {arg:?}; use --help"));
            };
            if mode.replace(selected).is_some() {
                return Err(
                    "choose one of --demo, --button-demo, --run, --self-test or --help".into(),
                );
            }
        }
        let mode = mode.unwrap_or(Mode::Form);
        if hold && matches!(mode, Mode::Help | Mode::SelfTest) {
            return Err("--hold is only available for a program run".into());
        }
        Ok(Self { mode, hold })
    }
}
pub fn load_source(path: &Path) -> Result<String, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("cannot open {path:?}: {e}"))?;
    read_source(file)
}
fn read_source(input: impl Read) -> Result<String, String> {
    let mut bytes = Vec::new();
    input
        .take((MAX_SOURCE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("cannot read source: {e}"))?;
    if bytes.len() > MAX_SOURCE_BYTES {
        return Err("source exceeds 16384 UTF-8 bytes".into());
    }
    String::from_utf8(bytes).map_err(|_| "source is not valid UTF-8".into())
}
// Bounded human-readable scalar output, not a wire codec or native handle format.
pub fn render_value(value: &ScalarValue) -> String {
    match value {
        ScalarValue::Integer(value) => value.to_string(),
        ScalarValue::Boolean(value) => value.to_string(),
        ScalarValue::None => "none".into(),
        ScalarValue::String(value) => format!("{value:?}"),
        ScalarValue::OptionalString(value) => value
            .0
            .as_ref()
            .map_or_else(|| "none".into(), |value| format!("{value:?}")),
        ScalarValue::StringList(value) => format!("{:?}", value.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parse(args: &[&str]) -> Result<Options, String> {
        Options::parse(args.iter().map(OsString::from))
    }
    #[test]
    fn modes_and_conflicts() {
        assert_eq!(parse(&[]).unwrap().mode, Mode::Form);
        assert_eq!(
            parse(&["--hold", "--run", "form.lese"]).unwrap(),
            Options {
                mode: Mode::Run("form.lese".into()),
                hold: true
            }
        );
        for args in [
            &["--run"][..],
            &["--demo", "--button-demo"],
            &["--self-test", "--hold"],
            &["--hold", "--hold"],
            &["--help", "--run", "file"],
            &["--invalid"],
        ] {
            assert!(parse(args).is_err(), "{args:?}");
        }
    }
    #[test]
    fn ingress_is_bounded_and_utf8_checked() {
        let bytes = vec![b'a'; MAX_SOURCE_BYTES];
        assert_eq!(
            read_source(bytes.as_slice()).unwrap().len(),
            MAX_SOURCE_BYTES
        );
        let too_large = vec![b'a'; MAX_SOURCE_BYTES + 2];
        let mut reader = too_large.as_slice();
        assert!(read_source(&mut reader).is_err());
        assert_eq!(reader.len(), 1);
        assert!(read_source(&[0xff][..]).is_err());
        assert_eq!(read_source("保存 ✓".as_bytes()).unwrap(), "保存 ✓");
    }
    #[cfg(unix)]
    #[test]
    fn non_utf8_file_paths_are_not_lossily_converted() {
        use std::os::unix::ffi::OsStringExt;
        let path = OsString::from_vec(b"/tmp/leselang-\xff.lese".to_vec());
        assert_eq!(
            Options::parse([OsString::from("--run"), path.clone()])
                .unwrap()
                .mode,
            Mode::Run(path.into())
        );
    }
    #[test]
    fn scalar_display_escapes_terminal_controls() {
        assert_eq!(
            render_value(&ScalarValue::String("a\n\u{1b}[31m".into())),
            "\"a\\n\\u{1b}[31m\""
        );
    }
}
