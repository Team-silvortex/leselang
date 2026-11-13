//! A bounded scalar report for the Calculator `1 + 2` reference workflow.
//!
//! The extra native reads bracket the ordinary shared VM driver. They consume
//! neither VM Effects nor VM Fuel and never press, clear, launch, or retry.
use crate::{
    calculator::{self, CalculatorBackend, CalculatorSnapshot},
    host::ProfileSnapshot,
};
use leselang_vm::native::core::ScalarValue;
use serde_json::{Value, json};
use std::io::Write;

/// Includes the terminating LF. Oversized reports are refused before output.
pub const MAX_REPORT_BYTES: usize = 128 * 1024;

#[derive(Debug)]
pub struct CalculatorReport {
    record: Value,
    verified: bool,
}

impl CalculatorReport {
    pub fn verified(&self) -> bool {
        self.verified
    }

    /// One complete JSON record followed by LF. No native identity is exported.
    pub fn to_json(&self) -> Result<String, String> {
        let mut json = serde_json::to_string(&self.record)
            .map_err(|error| format!("cannot encode Calculator report: {error}"))?;
        if json.len() >= MAX_REPORT_BYTES {
            return Err("Calculator report exceeds 131072 bytes; no report was written".into());
        }
        json.push('\n');
        Ok(json)
    }

    /// Validate and encode before writing, then flush exactly once. A failed
    /// write may be partial; neither it nor the native workflow is retried.
    pub fn write_to(&self, output: &mut impl Write) -> Result<(), String> {
        let json = self.to_json()?;
        output
            .write_all(json.as_bytes())
            .and_then(|()| output.flush())
            .map_err(|error| format!("cannot write Calculator report: {error}; output may be partial; native effects remain; do not retry automatically"))
    }
}

/// Execute with stderr diagnostics and independently check fresh native state.
/// A completed program with a failed postcondition produces `verified: false`.
/// Native/VM/read failures return an error without claiming a complete report;
/// all side effects already performed remain in the application.
pub fn run(backend: &dyn CalculatorBackend, source: &str) -> Result<CalculatorReport, String> {
    let run = || {
        let before = read_fresh(backend)?;
        let summary = calculator::run_diagnostics(backend, source)?;
        let after = read_fresh(backend)?;
        let ScalarValue::String(value) = &summary.value else {
            return Err("Calculator reporting requires a string program result".into());
        };
        let verified = before.display == "0"
            && value == "3"
            && summary.effects == 10
            && after.display == "3"
            && after.last_expression.as_deref() == Some("1 + 2")
            && before.same_instance(&after)
            && summary
                .observations
                .iter()
                .all(|snapshot| before.same_instance(snapshot))
            && before.buttons.iter().all(|button| button.enabled)
            && after.buttons.iter().all(|button| button.enabled);
        Ok(CalculatorReport {
            record: json!({
                "schema_version": 1,
                "kind": "macos_ax_calculator_run",
                "profile": "macos_ax.calculator",
                "result": { "type": "string", "value": value },
                "effects": summary.effects,
                "fuel_remaining": summary.fuel_remaining,
                "verified": verified,
                "native_before": [scalar_snapshot(&before)],
                "native_after": [scalar_snapshot(&after)],
            }),
            verified,
        })
    };
    run().map_err(|error: String| {
        format!("{error}; Calculator run has no complete verification report; native effects may already have occurred; do not retry automatically")
    })
}

fn read_fresh(backend: &dyn CalculatorBackend) -> Result<CalculatorSnapshot, String> {
    backend
        .check_permission()
        .map_err(|error| error.to_string())?;
    let snapshot = backend.snapshot().map_err(|error| error.to_string())?;
    backend
        .check_permission()
        .map_err(|error| error.to_string())?;
    snapshot.validate().map_err(|error| error.to_string())?;
    Ok(snapshot)
}

fn scalar_snapshot(snapshot: &CalculatorSnapshot) -> Value {
    json!({
        "identifier": "calculator",
        "role": "AXWindow",
        "display": snapshot.display,
        "last_expression": snapshot.last_expression,
        "buttons": snapshot.buttons.iter().map(|button| json!({
            "name": button.name,
            "enabled": button.enabled,
        })).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Error,
        calculator::{BUTTON_NAMES, ButtonState, PROGRAM},
    };
    use std::cell::{Cell, RefCell};

    #[derive(Clone, Copy)]
    enum ReadFault {
        Permission,
        RevokeAfterRead,
        ReplaceWindow,
        WrongExpression,
    }
    struct Fake {
        state: RefCell<CalculatorSnapshot>,
        permitted: Cell<bool>,
        reads: Cell<usize>,
        read_fault: Cell<Option<(usize, ReadFault)>>,
        presses: RefCell<Vec<String>>,
        uncertain: Cell<bool>,
        wrong_expression: Cell<bool>,
        revoke_after_press: Cell<Option<usize>>,
    }
    impl Fake {
        fn new() -> Self {
            Self {
                state: RefCell::new(CalculatorSnapshot {
                    identity: 10,
                    display: "0".into(),
                    last_expression: None,
                    buttons: BUTTON_NAMES
                        .iter()
                        .enumerate()
                        .map(|(index, name)| ButtonState {
                            name: (*name).into(),
                            identity: 20 + index as u64,
                            enabled: true,
                        })
                        .collect(),
                }),
                permitted: Cell::new(true),
                reads: Cell::new(0),
                read_fault: Cell::new(None),
                presses: RefCell::new(Vec::new()),
                uncertain: Cell::new(false),
                wrong_expression: Cell::new(false),
                revoke_after_press: Cell::new(None),
            }
        }
    }
    impl CalculatorBackend for Fake {
        fn check_permission(&self) -> Result<(), Error> {
            if self.permitted.get() {
                Ok(())
            } else {
                Err(Error::PermissionDenied)
            }
        }
        fn snapshot(&self) -> Result<CalculatorSnapshot, Error> {
            self.check_permission()?;
            self.reads.set(self.reads.get() + 1);
            if let Some((at, fault)) = self.read_fault.get()
                && at == self.reads.get()
            {
                match fault {
                    ReadFault::Permission => {
                        self.permitted.set(false);
                        return Err(Error::PermissionDenied);
                    }
                    ReadFault::RevokeAfterRead => self.permitted.set(false),
                    ReadFault::ReplaceWindow => self.state.borrow_mut().identity += 1,
                    ReadFault::WrongExpression => {
                        self.state.borrow_mut().last_expression = Some("0 + 3".into());
                    }
                }
            }
            Ok(self.state.borrow().clone())
        }
        fn press(&self, expected: &CalculatorSnapshot, button: &str) -> Result<(), Error> {
            let current = self.snapshot()?;
            if expected != &current {
                return Err(Error::Stale);
            }
            current.admit_action(button)?;
            self.presses.borrow_mut().push(button.into());
            let mut state = self.state.borrow_mut();
            match button {
                "one" | "two" => {
                    if state.display == "0" {
                        state.display.clear();
                    }
                    state.display.push(if button == "one" { '1' } else { '2' });
                }
                "add" => state.display.push('+'),
                "equals" => {
                    state.last_expression = Some(if self.wrong_expression.get() {
                        "0 + 3".into()
                    } else {
                        state.display.replace('+', " + ")
                    });
                    let terms = state
                        .display
                        .split('+')
                        .map(str::parse::<i64>)
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(|_| Error::Uncertain(-1))?;
                    state.display = terms.iter().sum::<i64>().to_string();
                }
                _ => return Err(Error::Invalid("unknown button")),
            }
            if self.revoke_after_press.get() == Some(self.presses.borrow().len()) {
                self.permitted.set(false);
            }
            if self.uncertain.get() {
                Err(Error::Uncertain(-1))
            } else {
                Ok(())
            }
        }
    }

    fn record(report: &CalculatorReport) -> Value {
        serde_json::from_str(&report.to_json().unwrap()).unwrap()
    }
    fn keys(value: &Value) -> Vec<&str> {
        value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect()
    }
    fn successful_read_count() -> usize {
        let fake = Fake::new();
        assert!(run(&fake, PROGRAM).unwrap().verified());
        fake.reads.get()
    }

    #[test]
    fn complete_report_is_bounded_scalar_only_and_keeps_vm_accounting() {
        let fake = Fake::new();
        let report = run(&fake, PROGRAM).unwrap();
        assert!(report.verified());
        let text = report.to_json().unwrap();
        assert!(text.len() <= MAX_REPORT_BYTES);
        assert!(text.ends_with('\n'));
        assert_eq!(text.lines().count(), 1);
        let json = record(&report);
        assert_eq!(
            keys(&json),
            [
                "effects",
                "fuel_remaining",
                "kind",
                "native_after",
                "native_before",
                "profile",
                "result",
                "schema_version",
                "verified"
            ]
        );
        assert_eq!(json["schema_version"], 1);
        assert_eq!(json["kind"], "macos_ax_calculator_run");
        assert_eq!(json["profile"], "macos_ax.calculator");
        assert_eq!(json["result"], json!({"type":"string", "value":"3"}));
        assert_eq!(json["verified"], true);
        assert_eq!(json["effects"], 10);
        assert_eq!(fake.presses.borrow().as_slice(), BUTTON_NAMES);
        for name in ["native_before", "native_after"] {
            let snapshots = json[name].as_array().unwrap();
            assert_eq!(snapshots.len(), 1);
            let snapshot = &snapshots[0];
            assert_eq!(
                keys(snapshot),
                [
                    "buttons",
                    "display",
                    "identifier",
                    "last_expression",
                    "role"
                ]
            );
            assert_eq!(snapshot["identifier"], "calculator");
            assert_eq!(snapshot["role"], "AXWindow");
            let buttons = snapshot["buttons"].as_array().unwrap();
            assert_eq!(buttons.len(), BUTTON_NAMES.len());
            for (button, name) in buttons.iter().zip(BUTTON_NAMES) {
                assert_eq!(keys(button), ["enabled", "name"]);
                assert_eq!(button["name"], name);
                assert_eq!(button["enabled"], true);
            }
        }
        assert_eq!(json["native_before"][0]["display"], "0");
        assert_eq!(json["native_before"][0]["last_expression"], Value::Null);
        assert_eq!(json["native_after"][0]["display"], "3");
        assert_eq!(json["native_after"][0]["last_expression"], "1 + 2");
        let plain = Fake::new();
        let summary = calculator::run(&plain, PROGRAM).unwrap();
        assert_eq!(json["fuel_remaining"], summary.fuel_remaining);
        assert_eq!(json["effects"], summary.effects);
        assert_eq!(fake.reads.get(), plain.reads.get() + 2);
        assert_eq!(fake.state, plain.state);
    }

    #[test]
    fn independent_catalog_agent_program_runs_the_same_ten_effect_workflow() {
        // The Consumer planner checks its generated source against this fixture.
        // This host test executes that actual output without depending on the
        // Consumer or granting a test access to a desktop application.
        let source =
            include_str!("../../../consumer/examples/catalog_agent/fixtures/calculator.lese");
        let fake = Fake::new();
        let report = run(&fake, source).unwrap();
        assert!(report.verified());
        let json = record(&report);
        assert_eq!(json["effects"], 10);
        assert!(json["fuel_remaining"].as_u64().unwrap() < 2000);
        assert_eq!(fake.presses.borrow().as_slice(), BUTTON_NAMES);
        assert_eq!(fake.state.borrow().display, "3");
        assert_eq!(
            fake.state.borrow().last_expression.as_deref(),
            Some("1 + 2")
        );
    }

    #[test]
    fn metadata_planned_program_keeps_fuel_guards_and_partial_native_effects() {
        // This is the generic Consumer planner's actual output, tied to its
        // published host-metadata fixture by the Consumer's golden source test.
        let source = include_str!(
            "../../../consumer/examples/catalog_agent/fixtures/calculator-semantics.lese"
        );
        let success = Fake::new();
        let report = run(&success, source).unwrap();
        let json = record(&report);
        assert!(report.verified());
        assert_eq!(json["effects"], 10);
        assert!(
            json["fuel_remaining"]
                .as_u64()
                .is_some_and(|fuel| fuel > 0 && fuel < 2000)
        );
        assert_eq!(success.presses.borrow().as_slice(), BUTTON_NAMES);

        let nonzero = Fake::new();
        nonzero.state.borrow_mut().display = "15".into();
        nonzero.state.borrow_mut().last_expression = Some("8 + 7".into());
        let report = run(&nonzero, source).unwrap();
        let json = record(&report);
        assert!(!report.verified());
        assert_eq!(json["effects"], 2);
        assert_eq!(json["result"]["value"], "initial display mismatch");
        assert_eq!(json["native_before"], json["native_after"]);
        assert!(nonzero.presses.borrow().is_empty());

        let wrong = Fake::new();
        wrong.wrong_expression.set(true);
        let report = run(&wrong, source).unwrap();
        assert!(!report.verified());
        assert_eq!(record(&report)["result"]["value"], "verification failed");
        assert_eq!(wrong.presses.borrow().as_slice(), BUTTON_NAMES);
        assert_eq!(wrong.state.borrow().display, "3");
        assert_eq!(
            wrong.state.borrow().last_expression.as_deref(),
            Some("0 + 3")
        );

        for revoke in [false, true] {
            let partial = Fake::new();
            if revoke {
                partial.revoke_after_press.set(Some(1));
            } else {
                partial.uncertain.set(true);
            }
            let error = run(&partial, source).unwrap_err();
            assert!(error.contains("no complete verification report"));
            assert_eq!(partial.presses.borrow().as_slice(), ["one"]);
            assert_eq!(partial.state.borrow().display, "1");
        }
    }

    #[test]
    fn initial_nonzero_returns_a_failed_report_without_pressing_or_clearing() {
        let fake = Fake::new();
        fake.state.borrow_mut().display = "9".into();
        let report = run(&fake, PROGRAM).unwrap();
        assert!(!report.verified());
        let json = record(&report);
        assert_eq!(json["result"]["value"], "initial display mismatch");
        assert_eq!(json["effects"], 2);
        assert_eq!(json["native_before"][0]["display"], "9");
        assert_eq!(json["native_after"][0]["display"], "9");
        assert!(fake.presses.borrow().is_empty());
    }

    #[test]
    fn zero_display_may_retain_an_old_expression_before_the_workflow() {
        let fake = Fake::new();
        fake.state.borrow_mut().last_expression = Some("2 - 2".into());
        assert!(run(&fake, PROGRAM).unwrap().verified());
    }

    #[test]
    fn same_numeric_result_with_wrong_expression_is_not_verified() {
        let fake = Fake::new();
        fake.wrong_expression.set(true);
        let report = run(&fake, PROGRAM).unwrap();
        assert!(!report.verified());
        let json = record(&report);
        assert_eq!(json["effects"], 10);
        assert_eq!(json["result"]["value"], "verification failed");
        assert_eq!(json["native_after"][0]["display"], "3");
        assert_eq!(json["native_after"][0]["last_expression"], "0 + 3");
        assert_eq!(fake.presses.borrow().len(), 4);
    }

    #[test]
    fn final_fresh_reads_can_refuse_a_successful_vm_result() {
        let final_read = successful_read_count();
        for fault in [ReadFault::ReplaceWindow, ReadFault::WrongExpression] {
            let fake = Fake::new();
            fake.read_fault.set(Some((final_read, fault)));
            let report = run(&fake, PROGRAM).unwrap();
            assert!(!report.verified());
            assert_eq!(record(&report)["result"]["value"], "3");
            assert_eq!(fake.presses.borrow().len(), 4);
            assert_eq!(fake.reads.get(), final_read);
        }
    }

    #[test]
    fn final_permission_loss_has_no_complete_report_and_preserves_effects() {
        let final_read = successful_read_count();
        for fault in [ReadFault::Permission, ReadFault::RevokeAfterRead] {
            let fake = Fake::new();
            fake.read_fault.set(Some((final_read, fault)));
            let error = run(&fake, PROGRAM).unwrap_err();
            assert!(error.contains("no complete verification report"));
            assert!(error.contains("do not retry automatically"));
            assert_eq!(fake.presses.borrow().len(), 4);
            assert_eq!(fake.state.borrow().display, "3");
            assert_eq!(fake.reads.get(), final_read);
        }
    }

    #[test]
    fn uncertain_action_is_not_retried_or_reported_as_success() {
        let fake = Fake::new();
        fake.uncertain.set(true);
        let error = run(&fake, PROGRAM).unwrap_err();
        assert!(error.contains("no complete verification report"));
        assert!(error.contains("do not retry automatically"));
        assert_eq!(fake.presses.borrow().as_slice(), ["one"]);
        assert_eq!(fake.state.borrow().display, "1");
    }

    #[test]
    fn revoked_permission_after_a_press_prevents_later_actions_and_a_report() {
        let fake = Fake::new();
        fake.revoke_after_press.set(Some(2));
        let error = run(&fake, PROGRAM).unwrap_err();
        assert!(error.contains("no complete verification report"));
        assert_eq!(fake.presses.borrow().as_slice(), ["one", "add"]);
        assert_eq!(fake.state.borrow().display, "1+");
    }

    #[test]
    fn preflight_permission_and_compilation_failures_have_no_native_actions() {
        let denied = Fake::new();
        denied.permitted.set(false);
        assert!(run(&denied, PROGRAM).is_err());
        assert_eq!(denied.reads.get(), 0);
        assert!(denied.presses.borrow().is_empty());
        let invalid = Fake::new();
        assert!(run(&invalid, "fn main() = missing(").is_err());
        assert!(invalid.presses.borrow().is_empty());
        assert_eq!(invalid.state.borrow().display, "0");
    }

    #[test]
    fn returning_three_without_the_workflow_is_not_verification() {
        let fake = Fake::new();
        let report = run(&fake, "fn main() = \"3\"").unwrap();
        assert!(!report.verified());
        assert_eq!(record(&report)["effects"], 0);
        assert!(fake.presses.borrow().is_empty());
        assert_eq!(fake.state.borrow().display, "0");
    }

    #[test]
    fn report_limit_includes_lf_and_is_checked_before_writing() {
        let mut report = CalculatorReport {
            record: json!(""),
            verified: false,
        };
        report.record = json!("x".repeat(MAX_REPORT_BYTES - 3));
        let mut output = Vec::new();
        report.write_to(&mut output).unwrap();
        assert_eq!(output.len(), MAX_REPORT_BYTES);
        report.record = json!("x".repeat(MAX_REPORT_BYTES - 2));
        output.clear();
        assert!(report.write_to(&mut output).is_err());
        assert!(output.is_empty());
    }

    #[test]
    fn partial_output_failure_does_not_restart_report_or_native_work() {
        struct BrokenOutput {
            output: Vec<u8>,
            calls: usize,
        }
        impl Write for BrokenOutput {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.calls += 1;
                if self.calls == 1 {
                    self.output.extend_from_slice(&bytes[..7]);
                    Ok(7)
                } else {
                    Err(std::io::ErrorKind::BrokenPipe.into())
                }
            }
            fn flush(&mut self) -> std::io::Result<()> {
                panic!("failed write must not flush");
            }
        }
        let fake = Fake::new();
        let report = run(&fake, PROGRAM).unwrap();
        let mut output = BrokenOutput {
            output: Vec::new(),
            calls: 0,
        };
        let error = report.write_to(&mut output).unwrap_err();
        assert!(error.contains("output may be partial"));
        assert!(error.contains("do not retry automatically"));
        assert_eq!(output.calls, 2);
        assert_eq!(output.output.len(), 7);
        assert_eq!(fake.presses.borrow().len(), 4);
        assert_eq!(fake.state.borrow().display, "3");
    }
}
