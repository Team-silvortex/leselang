//! Public scalar evidence only: no native identity, session or grant is encoded.
use leselang_vm::native::core::{MAX_SCALAR_STRING_BYTES, ScalarValue};
use serde_json::{Value, json};
use std::io::{self, Write};

const MAX_REPORT_BYTES: usize = 128 * 1024;

pub(crate) struct Snapshot<'a> {
    pub identifier: &'a str,
    pub role: &'a str,
    pub title: &'a str,
    pub value: &'a str,
    pub editable: bool,
    pub enabled: bool,
    pub clicks: u64,
}

#[cfg(target_os = "macos")]
impl<'a> From<&'a leselang_appkit_host::gui::WidgetSnapshot> for Snapshot<'a> {
    fn from(value: &'a leselang_appkit_host::gui::WidgetSnapshot) -> Self {
        Self {
            identifier: &value.identifier,
            role: &value.role,
            title: &value.title,
            value: &value.value,
            editable: value.editable,
            enabled: value.enabled,
            clicks: value.clicks,
        }
    }
}

fn snapshot(value: &Snapshot<'_>) -> Result<Value, String> {
    if [value.identifier, value.role, value.title, value.value]
        .iter()
        .any(|text| text.len() > MAX_SCALAR_STRING_BYTES)
    {
        return Err("native report text exceeds scalar bounds".into());
    }
    Ok(json!({
        "identifier": value.identifier,
        "role": value.role,
        "title": value.title,
        "value": value.value,
        "editable": value.editable,
        "enabled": value.enabled,
        "clicks": value.clicks,
    }))
}

fn scalar(value: &ScalarValue) -> Result<Value, String> {
    if !value.is_bounded() {
        return Err("report result exceeds scalar bounds".into());
    }
    let (kind, value) = match value {
        ScalarValue::Integer(value) => ("integer", json!(value)),
        ScalarValue::Boolean(value) => ("boolean", json!(value)),
        ScalarValue::String(value) => ("string", json!(value)),
        ScalarValue::None => ("none", Value::Null),
        ScalarValue::OptionalString(value) => ("optional_string", json!(value.0)),
        ScalarValue::StringList(value) => ("string_list", json!(value.0)),
    };
    Ok(json!({"type": kind, "value": value}))
}

/// Encode completely before writing stdout. Native samples are extra host-side
/// verification reads; they are not VM Effects and do not change the VM's Fuel.
pub(crate) fn encode(
    result: &ScalarValue,
    effects: usize,
    fuel_remaining: u64,
    native_before: &[Snapshot<'_>; 3],
    native_after: &[Snapshot<'_>; 3],
) -> Result<String, String> {
    let before = native_before
        .iter()
        .map(snapshot)
        .collect::<Result<Vec<_>, _>>()?;
    let after = native_after
        .iter()
        .map(snapshot)
        .collect::<Result<Vec<_>, _>>()?;
    let value = json!({
        "schema_version": 1,
        "kind": "appkit_run",
        "profile": "appkit",
        "result": scalar(result)?,
        "effects": effects,
        "fuel_remaining": fuel_remaining,
        "native_before": before,
        "native_after": after,
    });
    let mut encoded = BoundedOutput(Vec::new());
    serde_json::to_writer(&mut encoded, &value)
        .map_err(|error| format!("cannot encode run report: {error}"))?;
    encoded
        .write_all(b"\n")
        .map_err(|error| error.to_string())?;
    // serde_json emits UTF-8 and the only appended byte is LF.
    String::from_utf8(encoded.0).map_err(|_| "run report is not UTF-8".into())
}

struct BoundedOutput(Vec<u8>);
impl Write for BoundedOutput {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > MAX_REPORT_BYTES - self.0.len() {
            return Err(io::Error::other("run report exceeds 128 KiB"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(crate) fn write(output: &mut dyn Write, encoded: &str) -> Result<(), String> {
    output
        .write_all(encoded.as_bytes())
        .and_then(|()| output.flush())
        .map_err(|error| format!("cannot write run report: {error}; output may be partial"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use leselang_vm::native::core::{OptionalStringValue, StringListValue};

    fn snapshots(text: &str) -> [Snapshot<'_>; 3] {
        ["save", "other", "input"].map(|identifier| Snapshot {
            identifier,
            role: "text_field",
            title: "title",
            value: text,
            editable: true,
            enabled: false,
            clicks: 7,
        })
    }

    #[test]
    fn all_scalar_kinds_and_public_native_fields_round_trip() {
        let text = "quoted \" \\\n\u{0} 保存 ✓";
        let native = snapshots(text);
        for (value, kind, expected) in [
            (ScalarValue::Integer(u64::MAX), "integer", json!(u64::MAX)),
            (ScalarValue::Boolean(false), "boolean", json!(false)),
            (ScalarValue::String(text.into()), "string", json!(text)),
            (ScalarValue::None, "none", Value::Null),
            (
                ScalarValue::OptionalString(OptionalStringValue(None)),
                "optional_string",
                Value::Null,
            ),
            (
                ScalarValue::OptionalString(OptionalStringValue(Some(text.into()))),
                "optional_string",
                json!(text),
            ),
            (
                ScalarValue::StringList(StringListValue(vec![text.into(), "".into()])),
                "string_list",
                json!([text, ""]),
            ),
        ] {
            let output = encode(&value, 9, 17, &native, &native).unwrap();
            assert_eq!(output.lines().count(), 1);
            assert!(output.ends_with('\n'));
            let report: Value = serde_json::from_str(&output).unwrap();
            assert_eq!(report["schema_version"], 1);
            assert_eq!(report["kind"], "appkit_run");
            assert_eq!(report["profile"], "appkit");
            assert_eq!(report["result"], json!({"type":kind,"value":expected}));
            assert_eq!(report["effects"], 9);
            assert_eq!(report["fuel_remaining"], 17);
            for field in ["native_before", "native_after"] {
                let entries = report[field].as_array().unwrap();
                assert_eq!(entries.len(), 3);
                for entry in entries {
                    assert_eq!(entry.as_object().unwrap().len(), 7);
                    assert_eq!(entry["value"], text);
                    for absent in ["identity", "mutation", "lease", "session"] {
                        assert!(entry.get(absent).is_none());
                    }
                }
            }
        }
    }

    #[test]
    fn bounded_encoding_rejects_large_values_and_escape_growth() {
        let native = snapshots("");
        let large = "x".repeat(MAX_SCALAR_STRING_BYTES + 1);
        assert!(encode(&ScalarValue::String(large.clone()), 0, 0, &native, &native).is_err());
        assert!(encode(&ScalarValue::None, 0, 0, &snapshots(&large), &native).is_err());
        let escaped = "\u{0}".repeat(MAX_SCALAR_STRING_BYTES);
        assert!(
            encode(
                &ScalarValue::None,
                0,
                0,
                &snapshots(&escaped),
                &snapshots(&escaped)
            )
            .is_err()
        );
        let mut boundary = BoundedOutput(Vec::new());
        boundary
            .write_all(&vec![b'x'; MAX_REPORT_BYTES - 1])
            .unwrap();
        boundary.write_all(b"\n").unwrap();
        assert_eq!(boundary.0.len(), MAX_REPORT_BYTES);
        assert!(boundary.write_all(b"x").is_err());
        assert_eq!(boundary.0.len(), MAX_REPORT_BYTES);
    }

    struct PartialWriter {
        bytes: Vec<u8>,
        remaining: usize,
        fail_flush: bool,
        failures: usize,
    }
    impl Write for PartialWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.remaining == 0 {
                self.failures += 1;
                return Err(io::Error::other("write failure"));
            }
            let length = self.remaining.min(bytes.len());
            self.bytes.extend_from_slice(&bytes[..length]);
            self.remaining -= length;
            Ok(length)
        }
        fn flush(&mut self) -> io::Result<()> {
            if self.fail_flush {
                self.failures += 1;
                Err(io::Error::other("flush failure"))
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn output_failures_preserve_exact_prefix_and_are_not_retried() {
        let native = snapshots("");
        let encoded = encode(&ScalarValue::Boolean(true), 9, 11, &native, &native).unwrap();
        for (remaining, fail_flush) in [(0, false), (19, false), (encoded.len(), true)] {
            let mut output = PartialWriter {
                bytes: Vec::new(),
                remaining,
                fail_flush,
                failures: 0,
            };
            let error = write(&mut output, &encoded).unwrap_err();
            assert!(error.contains("output may be partial"));
            assert_eq!(output.bytes, encoded.as_bytes()[..remaining]);
            assert_eq!(output.failures, 1);
        }
    }
}
