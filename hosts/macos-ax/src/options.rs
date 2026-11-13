//! Bounded command-line ingress; no application or permission changes.
#[cfg(any(target_os = "macos", test))]
use leselang_vm::native::core::ScalarValue;
use std::{
    ffi::OsString,
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub const INITIAL: &str = "LeseLang AX initial\n";
#[cfg(target_os = "macos")]
pub const EXPECTED: &str = "LeseLang cross-process ✓\n";
pub const MAX_SOURCE_BYTES: usize = 16 * 1024;
pub const HELP: &str = "LeseLang macOS Accessibility host\n  --prepare FILE                  Create a new plain-text test document (never overwrite)\n  --check-permission              Check this process's Accessibility access without prompting\n  --document FILE [--run SOURCE]   Drive exactly that already-open TextEdit document\n  --help                         Show usage\nRequires macOS 14+, TextEdit and Accessibility permission. Open the prepared file\nin TextEdit before running. The default program only edits the exact initial test\ntext. A supplied source uses the same scoped catalog and 1000 initial Fuel.\nThis host does not open apps, grant permissions, save documents or retry writes.";

#[derive(Debug, Eq, PartialEq)]
pub enum Options {
    Help,
    Permission,
    Prepare(PathBuf),
    Run {
        document: PathBuf,
        source: Option<PathBuf>,
    },
}
impl Options {
    pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Self, String> {
        let mut args = args.into_iter();
        let mut document = None;
        let mut source = None;
        let mut mode = None;
        while let Some(arg) = args.next() {
            if arg == "--document" {
                let value = args.next().ok_or("--document requires a file")?;
                if document.replace(PathBuf::from(value)).is_some() {
                    return Err("duplicate --document".into());
                }
            } else if arg == "--run" {
                let value = args.next().ok_or("--run requires a source file")?;
                if source.replace(PathBuf::from(value)).is_some() {
                    return Err("duplicate --run".into());
                }
            } else {
                let selected = if arg == "--help" || arg == "-h" {
                    Self::Help
                } else if arg == "--check-permission" {
                    Self::Permission
                } else if arg == "--prepare" {
                    Self::Prepare(args.next().ok_or("--prepare requires a new file")?.into())
                } else {
                    return Err(format!("unknown argument {arg:?}; use --help"));
                };
                if mode.replace(selected).is_some() {
                    return Err("choose one action".into());
                }
            }
        }
        if let Some(mode) = mode {
            if document.is_some() || source.is_some() {
                return Err("choose one action".into());
            }
            return Ok(mode);
        }
        if let Some(document) = document {
            return Ok(Self::Run { document, source });
        }
        if source.is_some() {
            return Err("--run requires --document".into());
        }
        Ok(Self::Help)
    }
}

pub fn prepare(path: &Path) -> Result<(), String> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|error| format!("cannot create new file {path:?}: {error}"))?;
    file.write_all(INITIAL.as_bytes())
        .map_err(|error| format!("cannot write test document: {error}"))
}
pub fn load_source(path: &Path) -> Result<String, String> {
    read_source(
        std::fs::File::open(path)
            .map_err(|error| format!("cannot open source {path:?}: {error}"))?,
    )
}
fn read_source(reader: impl Read) -> Result<String, String> {
    let mut bytes = Vec::new();
    reader
        .take((MAX_SOURCE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read source: {error}"))?;
    if bytes.len() > MAX_SOURCE_BYTES {
        return Err("source exceeds 16384 UTF-8 bytes".into());
    }
    String::from_utf8(bytes).map_err(|_| "source is not valid UTF-8".into())
}
#[cfg(any(target_os = "macos", test))]
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
    #[test]
    fn explicit_scope_and_conflicting_modes() {
        let parse = |args: &[&str]| Options::parse(args.iter().map(OsString::from));
        assert_eq!(
            parse(&["--document", "a.txt", "--run", "a.lese"]).unwrap(),
            Options::Run {
                document: "a.txt".into(),
                source: Some("a.lese".into())
            }
        );
        for args in [
            &["--document"][..],
            &["--run", "a"],
            &["--prepare", "a", "--document", "b"],
            &["--check-permission", "--run", "a"],
            &["--document", "a", "--document", "b"],
            &["--invalid"],
        ] {
            assert!(parse(args).is_err(), "{args:?}");
        }
    }
    #[test]
    fn source_is_bounded_and_utf8_checked() {
        let bytes = vec![b'a'; MAX_SOURCE_BYTES + 2];
        let mut remaining = bytes.as_slice();
        assert!(read_source(&mut remaining).is_err());
        assert_eq!(remaining.len(), 1);
        assert_eq!(
            read_source(&bytes[..MAX_SOURCE_BYTES]).unwrap().len(),
            MAX_SOURCE_BYTES
        );
        assert!(read_source(&[0xff][..]).is_err());
        assert_eq!(read_source("保存 ✓".as_bytes()).unwrap(), "保存 ✓");
    }
    #[test]
    fn preparation_refuses_to_overwrite_existing_content() {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "leselang-ax-options-{}-{}.txt",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        prepare(&path).unwrap();
        std::fs::write(&path, "preserve this content").unwrap();
        assert!(prepare(&path).is_err());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "preserve this content"
        );
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn displayed_values_escape_control_characters() {
        assert_eq!(
            render_value(&ScalarValue::String("x\n\u{1b}[31m".into())),
            "\"x\\n\\u{1b}[31m\""
        );
    }
    #[cfg(unix)]
    #[test]
    fn document_paths_are_not_lossily_converted() {
        use std::os::unix::ffi::OsStringExt;
        let path = OsString::from_vec(b"/tmp/leselang-\xff.txt".to_vec());
        assert_eq!(
            Options::parse(["--document".into(), path.clone()]).unwrap(),
            Options::Run {
                document: path.into(),
                source: None
            }
        );
    }
}
