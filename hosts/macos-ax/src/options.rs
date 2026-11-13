//! Bounded command-line ingress; no application or permission changes.
use leselang_macos_ax_host::scope::{
    ApplicationSpec, AuxiliaryProcessSpec, MAX_AUXILIARY_PROCESSES, WindowSelector,
};
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
pub const HELP: &str = "LeseLang macOS Accessibility host\n  --prepare FILE                  Create a new plain-text test document (never overwrite)\n  --check-permission              Check this process's Accessibility access without prompting\n  --document FILE [--run SOURCE]   Drive exactly that already-open TextEdit document\n  --calculator [--run SOURCE]      Drive Calculator's supported basic-mode window\n  --app ABS_APP [--pid PID]       Select an already-running .app executable instance\n  --windows                      List windows of --app without reading controls\n  --window-title TITLE           Match an exact window title\n  --window-id IDENTIFIER         Match an exact window identifier\n  --window-document FILE         Match an exact local document path\n  --controls                     Use the per-control catalog in the selected scope\n  --widgets                      Opt in to common widgets (requires --app and window)\n  --auxiliary-processes          Inspect foreign process frontiers of an exact window (read only)\n  --allow-auxiliary PID START_SECONDS START_MICROS EXECUTABLE\n                                 Explicit widget grant; repeat for at most four processes\n  --discover                     Print bounded metadata (requires --controls or --widgets)\n  --help                         Show usage\nRequires macOS 14+, the selected application already open and Accessibility access.\nTextEdit uses 1000 Fuel; its default program requires the exact initial test text.\nCalculator uses 2000 Fuel; its default program requires display 0 before 1 + 2 =.\nThe legacy Calculator demo supports the observed English basic-mode interface.\nControl mode requires --discover or --run SOURCE and uses 10000 Fuel for source.\nWith --app, use --windows or --controls/--widgets with an exact window field.\nWindow fields combine; missing or multiple matches fail. No focused-window fallback.\nDiscovery keys are connection-local; use exact selectors in later runs.\nAuxiliary discovery does not authorize access; grants require complete reviewed identities.\nThis host does not open apps, clear Calculator, change OS permissions, save documents\nor retry uncertain actions. Source files are limited to 16384 UTF-8 bytes.";

#[derive(Debug, Eq, PartialEq)]
pub enum Options {
    Help,
    Permission,
    Prepare(PathBuf),
    Run {
        document: PathBuf,
        source: Option<PathBuf>,
    },
    Calculator {
        source: Option<PathBuf>,
    },
    Controls {
        scope: ControlScope,
        input: ControlInput,
    },
    Windows(ApplicationSpec),
    AuxiliaryProcesses {
        application: ApplicationSpec,
        window: WindowSelector,
    },
    Widgets {
        application: ApplicationSpec,
        window: WindowSelector,
        auxiliaries: Vec<AuxiliaryProcessSpec>,
        input: ControlInput,
    },
}
#[derive(Debug, Eq, PartialEq)]
pub enum ControlScope {
    Document(PathBuf),
    Calculator,
    Application {
        application: ApplicationSpec,
        window: WindowSelector,
    },
}
#[derive(Debug, Eq, PartialEq)]
pub enum ControlInput {
    Discover,
    Run(PathBuf),
}
impl Options {
    pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Self, String> {
        let mut args = args.into_iter();
        let mut document = None;
        let mut source = None;
        let mut mode = None;
        let mut calculator = false;
        let mut controls = false;
        let mut widgets = false;
        let mut discover = false;
        let mut application = None;
        let mut pid = None;
        let mut windows = false;
        let mut auxiliary_processes = false;
        let mut auxiliaries: Vec<AuxiliaryProcessSpec> = Vec::new();
        let mut window_title = None;
        let mut window_identifier = None;
        let mut window_document = None;
        while let Some(arg) = args.next() {
            if arg == "--app" {
                let value = args.next().ok_or("--app requires an absolute .app path")?;
                if application.replace(PathBuf::from(value)).is_some() {
                    return Err("duplicate --app".into());
                }
            } else if arg == "--pid" {
                let value = args.next().ok_or("--pid requires a process identifier")?;
                let value = value
                    .to_str()
                    .filter(|value| {
                        !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
                    })
                    .and_then(|value| value.parse::<u32>().ok())
                    .ok_or("invalid --pid")?;
                if pid.replace(value).is_some() {
                    return Err("duplicate --pid".into());
                }
            } else if arg == "--windows" {
                if windows {
                    return Err("duplicate --windows".into());
                }
                windows = true;
            } else if arg == "--auxiliary-processes" {
                if auxiliary_processes {
                    return Err("duplicate --auxiliary-processes".into());
                }
                auxiliary_processes = true;
            } else if arg == "--allow-auxiliary" {
                if auxiliaries.len() >= MAX_AUXILIARY_PROCESSES {
                    return Err("too many auxiliary process grants".into());
                }
                let pid = u32::try_from(decimal(args.next(), "auxiliary PID")?)
                    .map_err(|_| "invalid auxiliary PID")?;
                let start_seconds = decimal(args.next(), "auxiliary start seconds")?;
                let start_micros = u32::try_from(decimal(args.next(), "auxiliary start micros")?)
                    .map_err(|_| "invalid auxiliary start micros")?;
                let executable_path = PathBuf::from(
                    args.next()
                        .ok_or("--allow-auxiliary requires an executable path")?,
                );
                let spec = AuxiliaryProcessSpec {
                    pid,
                    executable_path,
                    start_seconds,
                    start_micros,
                };
                spec.validate().map_err(|error| error.to_string())?;
                if auxiliaries.iter().any(|other| other.pid == pid) {
                    return Err("duplicate auxiliary PID".into());
                }
                auxiliaries.push(spec);
            } else if arg == "--window-title" || arg == "--window-id" {
                let value = args
                    .next()
                    .ok_or("window selector requires a value")?
                    .into_string()
                    .map_err(|_| "window selector is not UTF-8")?;
                let slot = if arg == "--window-title" {
                    &mut window_title
                } else {
                    &mut window_identifier
                };
                if slot.replace(value).is_some() {
                    return Err("duplicate window selector field".into());
                }
            } else if arg == "--window-document" {
                let value = args
                    .next()
                    .ok_or("--window-document requires a local file path")?;
                if window_document.replace(PathBuf::from(value)).is_some() {
                    return Err("duplicate --window-document".into());
                }
            } else if arg == "--document" {
                let value = args.next().ok_or("--document requires a file")?;
                if document.replace(PathBuf::from(value)).is_some() {
                    return Err("duplicate --document".into());
                }
            } else if arg == "--calculator" {
                if calculator {
                    return Err("duplicate --calculator".into());
                }
                calculator = true;
            } else if arg == "--controls" {
                if controls {
                    return Err("duplicate --controls".into());
                }
                controls = true;
            } else if arg == "--widgets" {
                if widgets {
                    return Err("duplicate --widgets".into());
                }
                widgets = true;
            } else if arg == "--discover" {
                if discover {
                    return Err("duplicate --discover".into());
                }
                discover = true;
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
            if document.is_some()
                || source.is_some()
                || calculator
                || controls
                || widgets
                || discover
                || application.is_some()
                || pid.is_some()
                || windows
                || auxiliary_processes
                || !auxiliaries.is_empty()
                || window_title.is_some()
                || window_identifier.is_some()
                || window_document.is_some()
            {
                return Err("choose one action".into());
            }
            return Ok(mode);
        }
        if controls && widgets {
            return Err("choose --controls or --widgets".into());
        }
        if !auxiliaries.is_empty() && !widgets {
            return Err("--allow-auxiliary requires --widgets".into());
        }
        if let Some(bundle_path) = application {
            if document.is_some() || calculator {
                return Err("choose --app, --document or --calculator".into());
            }
            let application = ApplicationSpec { bundle_path, pid };
            application.validate().map_err(|error| error.to_string())?;
            if windows {
                if controls
                    || widgets
                    || discover
                    || source.is_some()
                    || window_title.is_some()
                    || window_identifier.is_some()
                    || window_document.is_some()
                    || auxiliary_processes
                    || !auxiliaries.is_empty()
                {
                    return Err("--windows is a standalone read-only application action".into());
                }
                return Ok(Self::Windows(application));
            }
            if !controls && !widgets && !auxiliary_processes {
                return Err(
                    "--app requires --windows, --auxiliary-processes, --controls or --widgets"
                        .into(),
                );
            }
            let window = WindowSelector::Exact {
                title: window_title,
                identifier: window_identifier,
                document: window_document,
            };
            window.validate().map_err(|error| error.to_string())?;
            if auxiliary_processes {
                if controls || widgets || discover || source.is_some() || !auxiliaries.is_empty() {
                    return Err(
                        "--auxiliary-processes is a standalone read-only window action".into(),
                    );
                }
                return Ok(Self::AuxiliaryProcesses {
                    application,
                    window,
                });
            }
            let input = match (discover, source) {
                (true, None) => ControlInput::Discover,
                (false, Some(path)) => ControlInput::Run(path),
                _ => {
                    return Err(
                        "--controls/--widgets requires exactly one --discover or --run SOURCE action".into(),
                    );
                }
            };
            if widgets {
                return Ok(Self::Widgets {
                    application,
                    window,
                    auxiliaries,
                    input,
                });
            }
            return Ok(Self::Controls {
                scope: ControlScope::Application {
                    application,
                    window,
                },
                input,
            });
        }
        if widgets
            || pid.is_some()
            || windows
            || auxiliary_processes
            || !auxiliaries.is_empty()
            || window_title.is_some()
            || window_identifier.is_some()
            || window_document.is_some()
        {
            return Err("application/window selection flags require --app".into());
        }
        if controls {
            let scope = match (document, calculator) {
                (Some(path), false) => ControlScope::Document(path),
                (None, true) => ControlScope::Calculator,
                _ => {
                    return Err(
                        "--controls requires exactly one --document FILE or --calculator scope"
                            .into(),
                    );
                }
            };
            let input = match (discover, source) {
                (true, None) => ControlInput::Discover,
                (false, Some(path)) => ControlInput::Run(path),
                _ => {
                    return Err(
                        "--controls requires exactly one --discover or --run SOURCE action".into(),
                    );
                }
            };
            return Ok(Self::Controls { scope, input });
        }
        if discover {
            return Err("--discover requires --controls".into());
        }
        if calculator {
            if document.is_some() {
                return Err("choose --document or --calculator".into());
            }
            return Ok(Self::Calculator { source });
        }
        if let Some(document) = document {
            return Ok(Self::Run { document, source });
        }
        if source.is_some() {
            return Err("--run requires --document or --calculator".into());
        }
        Ok(Self::Help)
    }
}

fn decimal(value: Option<OsString>, field: &str) -> Result<u64, String> {
    value
        .as_ref()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| format!("missing or invalid {field}"))
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
    fn auxiliary_discovery_requires_a_window_and_is_not_a_grant() {
        let parse = |args: &[&str]| Options::parse(args.iter().map(OsString::from));
        let scope = ["--app", "/Applications/Test.app", "--window-id", "fixture"];
        assert!(matches!(
            parse(&[scope.as_slice(), &["--auxiliary-processes"]].concat()),
            Ok(Options::AuxiliaryProcesses { .. })
        ));
        for action in [
            &["--widgets"][..],
            &["--controls"],
            &["--discover"],
            &["--run", "source.lese"],
            &["--windows"],
            &["--auxiliary-processes"],
            &["--help"],
        ] {
            assert!(
                parse(&[scope.as_slice(), &["--auxiliary-processes"], action].concat()).is_err()
            );
        }
        assert!(parse(&["--auxiliary-processes"]).is_err());
        assert!(parse(&["--app", "/Applications/Test.app", "--auxiliary-processes"]).is_err());
    }

    #[test]
    fn auxiliary_grants_require_complete_explicit_widget_identities() {
        let parse = |args: &[&str]| Options::parse(args.iter().map(OsString::from));
        let scope = [
            "--app",
            "/Applications/Test.app",
            "--window-id",
            "fixture",
            "--widgets",
            "--discover",
        ];
        let grant = [
            "--allow-auxiliary",
            "42",
            "1234",
            "56",
            "/System/Library/Test Helper",
        ];
        let parsed = parse(&[scope.as_slice(), grant.as_slice()].concat()).unwrap();
        let Options::Widgets { auxiliaries, .. } = parsed else {
            panic!("wrong action")
        };
        assert_eq!(
            auxiliaries,
            vec![AuxiliaryProcessSpec {
                pid: 42,
                start_seconds: 1234,
                start_micros: 56,
                executable_path: "/System/Library/Test Helper".into(),
            }]
        );
        let Options::Widgets { auxiliaries, .. } = parse(&scope).unwrap() else {
            panic!("wrong action")
        };
        assert!(auxiliaries.is_empty());
        assert!(parse(&grant).is_err());
        assert!(parse(&[scope.as_slice(), grant.as_slice(), grant.as_slice()].concat()).is_err());
        for bad in [
            &["--allow-auxiliary"][..],
            &["--allow-auxiliary", "42", "1234", "56"],
            &["--allow-auxiliary", "0", "1234", "56", "/helper"],
            &["--allow-auxiliary", "2147483648", "1234", "56", "/helper"],
            &["--allow-auxiliary", "42", "0", "56", "/helper"],
            &["--allow-auxiliary", "42", "-1", "56", "/helper"],
            &["--allow-auxiliary", "42", "1234", "1000000", "/helper"],
            &["--allow-auxiliary", "42", "1234", "56", "relative/helper"],
            &["--allow-auxiliary", "42", "1234", "56", "/bad\0helper"],
            &["--allow-auxiliary", "+42", "1234", "56", "/helper"],
        ] {
            assert!(
                parse(&[scope.as_slice(), bad].concat()).is_err(),
                "accepted {bad:?}"
            );
        }
    }

    #[test]
    fn auxiliary_grant_count_is_bounded_before_native_access() {
        let mut args: Vec<OsString> = [
            "--app",
            "/Applications/Test.app",
            "--window-id",
            "fixture",
            "--widgets",
            "--run",
            "source.lese",
        ]
        .into_iter()
        .map(Into::into)
        .collect();
        for pid in 1..=MAX_AUXILIARY_PROCESSES {
            args.extend([
                "--allow-auxiliary".into(),
                (pid + 10).to_string().into(),
                "1234".into(),
                "0".into(),
                "/helper".into(),
            ]);
        }
        assert!(Options::parse(args.clone()).is_ok());
        args.extend(["--allow-auxiliary", "99", "1234", "0", "/helper"].map(Into::into));
        assert!(Options::parse(args).is_err());
    }

    #[test]
    fn widgets_require_an_explicit_generic_scope_and_single_action() {
        let parse = |args: &[&str]| Options::parse(args.iter().map(OsString::from));
        assert!(matches!(
            parse(&[
                "--app",
                "/Applications/Test.app",
                "--window-title",
                "Fixture",
                "--widgets",
                "--discover"
            ]),
            Ok(Options::Widgets {
                input: ControlInput::Discover,
                ..
            })
        ));
        assert!(matches!(
            parse(&[
                "--app",
                "/Applications/Test.app",
                "--window-id",
                "fixture",
                "--widgets",
                "--run",
                "widgets.lese"
            ]),
            Ok(Options::Widgets {
                input: ControlInput::Run(_),
                ..
            })
        ));
        for args in [
            &["--widgets"][..],
            &["--widgets", "--calculator", "--discover"],
            &["--widgets", "--document", "a.txt", "--discover"],
            &[
                "--widgets",
                "--controls",
                "--app",
                "/Applications/Test.app",
                "--window-title",
                "Fixture",
                "--discover",
            ],
            &["--widgets", "--app", "/Applications/Test.app", "--windows"],
            &["--widgets", "--app", "/Applications/Test.app", "--discover"],
            &[
                "--widgets",
                "--widgets",
                "--app",
                "/Applications/Test.app",
                "--window-id",
                "fixture",
                "--discover",
            ],
            &[
                "--widgets",
                "--app",
                "/Applications/Test.app",
                "--window-id",
                "fixture",
            ],
            &[
                "--widgets",
                "--app",
                "/Applications/Test.app",
                "--window-id",
                "fixture",
                "--discover",
                "--run",
                "widgets.lese",
            ],
        ] {
            assert!(parse(args).is_err(), "{args:?}");
        }
    }
    #[test]
    fn generic_application_actions_require_an_explicit_window_or_inventory() {
        let parse = |args: &[&str]| Options::parse(args.iter().map(OsString::from));
        let app = ApplicationSpec {
            bundle_path: "/Applications/Test.app".into(),
            pid: Some(42),
        };
        assert_eq!(
            parse(&[
                "--app",
                "/Applications/Test.app",
                "--pid",
                "42",
                "--windows"
            ])
            .unwrap(),
            Options::Windows(app.clone())
        );
        assert_eq!(
            parse(&[
                "--app",
                "/Applications/Test.app",
                "--pid",
                "42",
                "--window-title",
                "Test",
                "--window-id",
                "editor",
                "--controls",
                "--run",
                "test.lese"
            ])
            .unwrap(),
            Options::Controls {
                scope: ControlScope::Application {
                    application: app,
                    window: WindowSelector::Exact {
                        title: Some("Test".into()),
                        identifier: Some("editor".into()),
                        document: None
                    }
                },
                input: ControlInput::Run("test.lese".into()),
            }
        );
        assert!(matches!(
            parse(&[
                "--app",
                "/Applications/Test.app",
                "--controls",
                "--window-document",
                "note.rtfd",
                "--discover"
            ]),
            Ok(Options::Controls {
                input: ControlInput::Discover,
                ..
            })
        ));
    }
    #[test]
    fn generic_scope_conflicts_and_unbounded_inputs_refuse_before_native_access() {
        let parse = |args: &[&str]| Options::parse(args.iter().map(OsString::from));
        for suffix in [
            vec![],
            vec!["--controls", "--discover"],
            vec!["--run", "test.lese"],
            vec!["--windows", "--run", "test.lese"],
            vec!["--windows", "--controls"],
            vec!["--windows", "--window-title", "Test"],
            vec!["--windows", "--windows"],
            vec!["--windows", "--calculator"],
            vec!["--windows", "--document", "note.txt"],
            vec!["--windows", "--prepare", "note.txt"],
            vec!["--windows", "--check-permission"],
            vec!["--windows", "--help"],
            vec!["--windows", "--app", "/Other.app"],
            vec!["--windows", "--pid", "0"],
            vec!["--windows", "--pid", "-1"],
            vec!["--windows", "--pid", "2147483648"],
            vec!["--windows", "--pid", "42", "--pid", "42"],
            vec!["--controls", "--discover", "--window-title", ""],
            vec!["--controls", "--discover", "--window-title", "bad\0title"],
            vec![
                "--controls",
                "--discover",
                "--window-id",
                "a",
                "--window-id",
                "a",
            ],
            vec![
                "--controls",
                "--discover",
                "--window-id",
                "a",
                "--run",
                "test.lese",
            ],
            vec!["--controls", "--discover", "--window-document", ""],
        ] {
            let args: Vec<_> = ["--app", "/Applications/Test.app"]
                .into_iter()
                .chain(suffix)
                .collect();
            assert!(parse(&args).is_err(), "{args:?}");
        }
        for args in [
            &["--pid", "42"][..],
            &["--windows"],
            &["--window-title", "Test"],
            &["--window-id", "main", "--calculator"],
            &["--app", "Test.app", "--windows"],
            &["--app", "/Applications/Test", "--windows"],
        ] {
            assert!(parse(args).is_err(), "{args:?}");
        }
        let long = "x".repeat(4097);
        assert!(
            parse(&[
                "--app",
                "/Applications/Test.app",
                "--controls",
                "--discover",
                "--window-title",
                &long
            ])
            .is_err()
        );
    }
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
    fn calculator_profile_cannot_be_combined_with_document_authority() {
        let parse = |args: &[&str]| Options::parse(args.iter().map(OsString::from));
        assert_eq!(
            parse(&["--calculator"]).unwrap(),
            Options::Calculator { source: None }
        );
        assert_eq!(
            parse(&["--run", "calc.lese", "--calculator"]).unwrap(),
            Options::Calculator {
                source: Some("calc.lese".into())
            }
        );
        for args in [
            &["--calculator", "--document", "a.txt"][..],
            &["--document", "a.txt", "--calculator"],
            &["--calculator", "--calculator"],
            &["--calculator", "--prepare", "a.txt"],
            &["--calculator", "--check-permission"],
            &["--calculator", "--help"],
            &["--calculator", "--run"],
            &["--calculator", "--run", "a", "--run", "b"],
        ] {
            assert!(parse(args).is_err(), "{args:?}");
        }
    }
    #[test]
    fn controls_require_an_explicit_scope_and_action() {
        let parse = |args: &[&str]| Options::parse(args.iter().map(OsString::from));
        assert_eq!(
            parse(&["--controls", "--calculator", "--discover"]).unwrap(),
            Options::Controls {
                scope: ControlScope::Calculator,
                input: ControlInput::Discover
            }
        );
        assert_eq!(
            parse(&["--document", "test.txt", "--controls", "--run", "test.lese"]).unwrap(),
            Options::Controls {
                scope: ControlScope::Document("test.txt".into()),
                input: ControlInput::Run("test.lese".into())
            }
        );
        for args in [
            &["--controls"][..],
            &["--controls", "--calculator"],
            &["--controls", "--discover"],
            &["--discover", "--calculator"],
            &[
                "--controls",
                "--calculator",
                "--document",
                "a",
                "--discover",
            ],
            &["--controls", "--calculator", "--discover", "--run", "a"],
            &["--controls", "--calculator", "--discover", "--discover"],
            &["--controls", "--calculator", "--controls", "--run", "a"],
            &["--controls", "--prepare", "a", "--discover"],
            &["--controls", "--check-permission"],
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
