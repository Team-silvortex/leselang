//! A bounded Calculator-window capability using the shared native host engine.
use crate::{
    Error, driver,
    host::{Field, Profile, ProfileSnapshot},
};
use leselang_vm::native::core::{
    MAX_SCALAR_STRING_BYTES, OptionalStringValue, ScalarType, ScalarValue, StringListValue,
};

pub const BUTTON_NAMES: [&str; 4] = ["one", "add", "two", "equals"];
pub const PROGRAM: &str = include_str!("../examples/calculator.lese");

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ButtonState {
    pub name: String,
    pub identity: u64,
    pub enabled: bool,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CalculatorSnapshot {
    pub identity: u64,
    pub display: String,
    pub last_expression: Option<String>,
    pub buttons: Vec<ButtonState>,
}
/// One pinned Calculator process/window. The display and every admitted button
/// are action dependencies, checked again immediately before entering AXPress.
pub trait CalculatorBackend {
    fn snapshot(&self) -> Result<CalculatorSnapshot, Error>;
    fn check_permission(&self) -> Result<(), Error>;
    /// Every failure after entering the native press must be `Error::Uncertain`.
    fn press(&self, expected: &CalculatorSnapshot, button: &str) -> Result<(), Error>;
}
struct CalculatorProfile<'b>(&'b dyn CalculatorBackend);
impl Profile for CalculatorProfile<'_> {
    type Snapshot = CalculatorSnapshot;
    fn snapshot(&self) -> Result<CalculatorSnapshot, Error> {
        self.0.snapshot()
    }
    fn check_permission(&self) -> Result<(), Error> {
        self.0.check_permission()
    }
    fn apply(&self, expected: &CalculatorSnapshot, button: &str) -> Result<(), Error> {
        self.0.press(expected, button)
    }
}
impl ProfileSnapshot for CalculatorSnapshot {
    const SELECTOR: &'static str = "calculator";
    const OPERATION: &'static str = "gui.press";
    const ARGUMENT: &'static str = "button";
    fn validate(&self) -> Result<(), Error> {
        if self.display.len() > MAX_SCALAR_STRING_BYTES
            || self
                .last_expression
                .as_ref()
                .is_some_and(|value| value.len() > MAX_SCALAR_STRING_BYTES)
        {
            return Err(Error::Bounds);
        }
        if self.identity == 0 || self.buttons.len() != BUTTON_NAMES.len() {
            return Err(Error::Invalid("invalid Calculator snapshot metadata"));
        }
        for (index, button) in self.buttons.iter().enumerate() {
            if button.name != BUTTON_NAMES[index]
                || button.identity == 0
                || button.identity == self.identity
                || self.buttons[..index]
                    .iter()
                    .any(|other| other.identity == button.identity)
            {
                return Err(Error::Invalid("invalid Calculator button metadata"));
            }
        }
        Ok(())
    }
    fn same_instance(&self, other: &Self) -> bool {
        self.identity == other.identity
            && self.buttons.len() == other.buttons.len()
            && self
                .buttons
                .iter()
                .zip(&other.buttons)
                .all(|(a, b)| a.name == b.name && a.identity == b.identity)
    }
    fn copy_cost(&self) -> u64 {
        (self.display.len()
            + self
                .last_expression
                .as_ref()
                .map_or(1, |value| value.len() + 1)
            + self
                .buttons
                .iter()
                .map(|button| button.name.len() + 1)
                .sum::<usize>()) as u64
    }
    fn describe(&self) -> String {
        format!(
            "role=\"AXWindow\", display={:?}, last_expression={:?}, buttons={:?}",
            self.display,
            self.last_expression,
            self.buttons
                .iter()
                .map(|button| (&button.name, button.enabled))
                .collect::<Vec<_>>()
        )
    }
    fn field_type(field: Field) -> Option<ScalarType> {
        match field {
            Field::Display | Field::Role => Some(ScalarType::String),
            Field::Buttons => Some(ScalarType::StringList),
            Field::LastExpression => Some(ScalarType::OptionalString),
            Field::Revision => Some(ScalarType::Integer),
            _ => None,
        }
    }
    fn field(&self, field: Field) -> Result<ScalarValue, Error> {
        Ok(match field {
            Field::Display => ScalarValue::String(self.display.clone()),
            Field::Role => ScalarValue::String("AXWindow".into()),
            Field::Buttons => ScalarValue::StringList(StringListValue(
                self.buttons
                    .iter()
                    .map(|button| button.name.clone())
                    .collect(),
            )),
            Field::LastExpression => {
                ScalarValue::OptionalString(OptionalStringValue(self.last_expression.clone()))
            }
            _ => {
                return Err(Error::Invalid(
                    "field is unavailable in the Calculator profile",
                ));
            }
        })
    }
    fn admit_action(&self, button: &str) -> Result<(), Error> {
        if button.len() > MAX_SCALAR_STRING_BYTES {
            return Err(Error::Bounds);
        }
        let state = self
            .buttons
            .iter()
            .find(|candidate| candidate.name == button)
            .ok_or(Error::Invalid("button is outside the Calculator allowlist"))?;
        if !state.enabled {
            return Err(Error::Invalid("Calculator button is disabled"));
        }
        Ok(())
    }
}
pub fn run(
    backend: &dyn CalculatorBackend,
    source: &str,
) -> Result<driver::ProfileRunSummary<CalculatorSnapshot>, String> {
    driver::run_profile(&CalculatorProfile(backend), source, 2000)
}

pub(crate) fn run_diagnostics(
    backend: &dyn CalculatorBackend,
    source: &str,
) -> Result<driver::ProfileRunSummary<CalculatorSnapshot>, String> {
    driver::run_profile_diagnostics(&CalculatorProfile(backend), source, 2000)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        driver::Vm,
        host::{self, Host, ProfileReply as Reply, STACK, Work},
    };
    use leselang_vm::native::{
        HostWorkOutcome, LocalVm,
        core::*,
        hir::effect_session::{EffectSessionEnd, EffectSessionPoll, EffectSessionStatus},
    };
    use std::cell::{Cell, RefCell};

    struct Fake {
        state: RefCell<CalculatorSnapshot>,
        permitted: Cell<bool>,
        available: Cell<bool>,
        presses: RefCell<Vec<String>>,
        behavior: Cell<u8>,
        attempts: Cell<usize>,
    }
    #[test]
    fn offline_description_matches_live_calculator_catalog_and_fields() {
        let fake = Fake::new();
        host::with_profile(&CalculatorProfile(&fake), |host, _| {
            host.set_granted(false);
            fake.permitted.set(false);
            host::catalog::assert_live_description(host, crate::discovery::Profile::Calculator);
        })
        .unwrap();
        assert!(fake.presses.borrow().is_empty());
    }
    impl Fake {
        fn new() -> Self {
            Self {
                state: RefCell::new(CalculatorSnapshot {
                    identity: 7,
                    display: "0".into(),
                    last_expression: None,
                    buttons: BUTTON_NAMES
                        .iter()
                        .enumerate()
                        .map(|(index, name)| ButtonState {
                            name: (*name).into(),
                            identity: index as u64 + 20,
                            enabled: true,
                        })
                        .collect(),
                }),
                permitted: Cell::new(true),
                available: Cell::new(true),
                presses: RefCell::new(Vec::new()),
                behavior: Cell::new(0),
                attempts: Cell::new(0),
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
            if !self.available.get() {
                return Err(Error::Unavailable("Calculator closed"));
            }
            Ok(self.state.borrow().clone())
        }
        fn press(&self, expected: &CalculatorSnapshot, button: &str) -> Result<(), Error> {
            self.attempts.set(self.attempts.get() + 1);
            if self.behavior.get() == 1 {
                self.state.borrow_mut().display = "9".into();
            }
            let current = self.snapshot()?;
            if expected != &current {
                return Err(Error::Stale);
            }
            current.admit_action(button)?;
            if self.behavior.get() == 3 {
                return Ok(());
            }
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
                    state.last_expression = Some(state.display.replace('+', " + "));
                    let values: Result<Vec<i64>, _> =
                        state.display.split('+').map(str::parse).collect();
                    state.display = values
                        .map_err(|_| Error::Uncertain(-1))?
                        .iter()
                        .sum::<i64>()
                        .to_string();
                    if self.behavior.get() == 4 {
                        state.last_expression = Some("0 + 3".into());
                    }
                }
                _ => return Err(Error::Invalid("unknown button")),
            }
            if self.behavior.get() == 2 {
                Err(Error::Uncertain(-25204))
            } else {
                Ok(())
            }
        }
    }
    fn session(
        fake: &Fake,
        source: &str,
        body: impl for<'ir, 's> FnOnce(
            &Host<'_, 's, CalculatorSnapshot>,
            &mut Vm<'ir, 's, CalculatorSnapshot>,
        ),
    ) {
        host::with_profile(&CalculatorProfile(fake), |host, scope| {
            let node = host.compile(source).unwrap();
            let mut vm = LocalVm::new(scope, &node, Vec::new(), Fuel::new(2000), STACK);
            body(host, &mut vm);
        })
        .unwrap();
    }
    fn ready<'ir, 's>(
        host: &Host<'_, 's, CalculatorSnapshot>,
        vm: &mut Vm<'ir, 's, CalculatorSnapshot>,
    ) -> Work<'s, CalculatorSnapshot> {
        for operation in ["gui.find", "gui.observe"] {
            let work = driver::next(vm, host).unwrap();
            assert_eq!(work.operation(), operation);
            let reply = driver::dispatch(vm, host, work).unwrap();
            driver::accept(vm, host, reply).unwrap();
        }
        let work = driver::next(vm, host).unwrap();
        assert_eq!(work.operation(), "gui.press");
        work
    }
    fn failed(vm: &Vm<'_, '_, CalculatorSnapshot>) {
        assert_eq!(
            vm.status(),
            EffectSessionStatus::Terminal(EffectSessionEnd::Failed)
        );
    }
    #[test]
    fn real_source_contract_uses_ten_effects_and_an_observed_result() {
        let fake = Fake::new();
        let result = run(&fake, PROGRAM).unwrap();
        assert_eq!(result.value, ScalarValue::String("3".into()));
        assert_eq!(result.effects, 10);
        assert_eq!(result.observations.len(), 5);
        assert_eq!(fake.presses.borrow().as_slice(), BUTTON_NAMES);
        assert!(result.fuel_remaining < 2000);
        assert_eq!(
            fake.state.borrow().last_expression.as_deref(),
            Some("1 + 2")
        );
    }
    #[test]
    fn optional_last_expression_is_observable_and_bounded() {
        let fake = Fake::new();
        let source = "fn main() = bind(target: gui.find(name: \"calculator\"), body: bind(state: gui.observe(target: target), body: field(value: state, name: \"last_expression\")))";
        assert_eq!(
            run(&fake, source).unwrap().value,
            ScalarValue::OptionalString(OptionalStringValue(None))
        );
        fake.state.borrow_mut().last_expression = Some("1 + 2".into());
        assert_eq!(
            run(&fake, source).unwrap().value,
            ScalarValue::OptionalString(OptionalStringValue(Some("1 + 2".into())))
        );
        fake.state.borrow_mut().last_expression = Some("x".repeat(MAX_SCALAR_STRING_BYTES + 1));
        assert!(run(&fake, source).is_err());
        assert_eq!(fake.attempts.get(), 0);
    }
    #[test]
    fn nonzero_initial_display_is_not_cleared_or_overwritten() {
        let fake = Fake::new();
        fake.state.borrow_mut().display = "42".into();
        let result = run(&fake, PROGRAM).unwrap();
        assert_eq!(
            result.value,
            ScalarValue::String("initial display mismatch".into())
        );
        assert_eq!(result.effects, 2);
        assert_eq!(fake.attempts.get(), 0);
        assert_eq!(fake.state.borrow().display, "42");
    }
    #[test]
    fn submission_pump_and_observation_are_distinct() {
        let fake = Fake::new();
        session(&fake, PROGRAM, |host, vm| {
            let work = ready(host, vm);
            let queued = driver::dispatch(vm, host, work).unwrap();
            assert_eq!(fake.state.borrow().display, "0");
            assert!(host.has_pending());
            let receipt = driver::pump(vm, host, queued).unwrap();
            assert_eq!(fake.state.borrow().display, "1");
            assert_eq!(vm.status(), EffectSessionStatus::Awaiting);
            driver::accept(vm, host, receipt).unwrap();
            let work = driver::next(vm, host).unwrap();
            assert_eq!(work.operation(), "gui.observe");
            let receipt = driver::dispatch(vm, host, work).unwrap();
            let Reply::Snapshot(snapshot) = &receipt.reply else {
                panic!("snapshot expected")
            };
            assert_eq!(snapshot.data().display, "1");
            assert!(snapshot.revision() > 0);
            driver::accept(vm, host, receipt).unwrap();
        });
    }
    #[test]
    fn changed_dependencies_identity_permission_and_enabled_state_refuse_press() {
        for queued in [false, true] {
            for cause in 0..7 {
                let fake = Fake::new();
                session(&fake, PROGRAM, |host, vm| {
                    let mut work = Some(ready(host, vm));
                    let receipt =
                        queued.then(|| driver::dispatch(vm, host, work.take().unwrap()).unwrap());
                    match cause {
                        0 => fake.state.borrow_mut().display = "9".into(),
                        1 => fake.state.borrow_mut().identity += 1,
                        2 => fake.state.borrow_mut().buttons[0].identity += 100,
                        3 => fake.state.borrow_mut().buttons[0].enabled = false,
                        4 => fake.permitted.set(false),
                        5 => fake.available.set(false),
                        6 => fake.state.borrow_mut().last_expression = Some("3 + 4".into()),
                        _ => unreachable!(),
                    }
                    let result = if let Some(receipt) = receipt {
                        driver::pump(vm, host, receipt)
                    } else {
                        vm.with_metered_host_outcome(work.take().unwrap(), |work, fuel| {
                            host.dispatch(work, fuel)
                        })
                        .unwrap()
                    };
                    assert!(result.is_err(), "queued={queued}, cause={cause}");
                    assert_eq!(fake.attempts.get(), 0);
                    failed(vm);
                });
            }
        }
    }
    #[test]
    fn bridge_comparison_catches_a_change_after_host_preflight() {
        let fake = Fake::new();
        fake.behavior.set(1);
        assert!(run(&fake, PROGRAM).is_err());
        assert_eq!(fake.attempts.get(), 1);
        assert!(fake.presses.borrow().is_empty());
        assert_eq!(fake.state.borrow().display, "9");
    }
    #[test]
    fn a_matching_number_with_the_wrong_expression_does_not_verify() {
        let fake = Fake::new();
        fake.behavior.set(4);
        let result = run(&fake, PROGRAM).unwrap();
        assert_eq!(fake.state.borrow().display, "3");
        assert_eq!(
            result.value,
            ScalarValue::String("verification failed".into())
        );
        assert_eq!(result.effects, 10);
    }
    #[test]
    fn ignored_native_presses_do_not_satisfy_the_postcondition() {
        let fake = Fake::new();
        fake.behavior.set(3);
        let result = run(&fake, PROGRAM).unwrap();
        assert_eq!(
            result.value,
            ScalarValue::String("verification failed".into())
        );
        assert_eq!(fake.state.borrow().display, "0");
        assert_eq!(fake.attempts.get(), 4);
    }
    #[test]
    fn original_fuel_is_checked_before_poll_and_before_press_delivery() {
        let fake = Fake::new();
        host::with_profile(&CalculatorProfile(&fake), |host, scope| {
            let node = host.compile(PROGRAM).unwrap();
            let mut vm = LocalVm::new(scope, &node, Vec::new(), Fuel::new(0), STACK);
            assert!(vm.poll(host).is_err());
        })
        .unwrap();
        session(&fake, PROGRAM, |host, vm| {
            let work = ready(host, vm);
            let queued = driver::dispatch(vm, host, work).unwrap();
            vm.with_metered_host_outcome((), |(), fuel| {
                fuel.charge(fuel.remaining()).unwrap();
                HostWorkOutcome::Pending(())
            })
            .unwrap();
            assert!(driver::pump(vm, host, queued).is_err());
            assert_eq!(vm.fuel_remaining(), 0);
            assert_eq!(fake.attempts.get(), 0);
            failed(vm);
        });
    }
    #[test]
    fn cancellation_before_or_after_press_rejects_late_reply_and_preserves_effects() {
        for delivered in [false, true] {
            let fake = Fake::new();
            session(&fake, PROGRAM, |host, vm| {
                let work = ready(host, vm);
                let queued = driver::dispatch(vm, host, work).unwrap();
                let receipt = if delivered {
                    driver::pump(vm, host, queued).unwrap()
                } else {
                    queued
                };
                assert!(vm.cancel());
                let fuel = vm.fuel_remaining();
                assert!(
                    vm.try_accept(&receipt.identity, receipt.reply, host)
                        .is_err()
                );
                if !delivered {
                    assert!(host.discard_pending());
                }
                assert_eq!(
                    fake.state.borrow().display,
                    if delivered { "1" } else { "0" }
                );
                assert_eq!(vm.fuel_remaining(), fuel);
            });
        }
    }
    #[test]
    fn uncertain_press_is_not_retried_or_rolled_back() {
        let fake = Fake::new();
        fake.behavior.set(2);
        session(&fake, PROGRAM, |host, vm| {
            let work = ready(host, vm);
            let receipt = driver::dispatch(vm, host, work).unwrap();
            assert!(matches!(
                driver::pump(vm, host, receipt),
                Err(Error::Uncertain(_))
            ));
            assert_eq!(
                vm.status(),
                EffectSessionStatus::Terminal(EffectSessionEnd::HostUncertain)
            );
            assert_eq!(fake.state.borrow().display, "1");
            assert_eq!(fake.attempts.get(), 1);
            assert!(matches!(
                vm.poll(host).unwrap(),
                EffectSessionPoll::Terminal(EffectSessionEnd::HostUncertain)
            ));
        });
    }
    #[test]
    fn an_already_delivered_press_cannot_be_replayed() {
        let fake = Fake::new();
        session(&fake, PROGRAM, |host, vm| {
            let work = ready(host, vm);
            let receipt = driver::dispatch(vm, host, work).unwrap();
            let replay = host::ProfileReceipt {
                identity: receipt.identity.clone(),
                reply: Reply::Submitted(true),
            };
            let original = driver::pump(vm, host, receipt).unwrap();
            assert!(driver::pump(vm, host, replay).is_err());
            assert_eq!(fake.attempts.get(), 1);
            assert_eq!(fake.state.borrow().display, "1");
            assert!(
                vm.try_accept(&original.identity, original.reply, host)
                    .is_err()
            );
            failed(vm);
        });
    }
    #[test]
    fn a_later_failure_does_not_undo_an_accepted_press() {
        let fake = Fake::new();
        session(&fake, PROGRAM, |host, vm| {
            let work = ready(host, vm);
            let receipt = driver::dispatch(vm, host, work).unwrap();
            let receipt = driver::pump(vm, host, receipt).unwrap();
            driver::accept(vm, host, receipt).unwrap();
            let work = driver::next(vm, host).unwrap();
            fake.available.set(false);
            assert!(
                vm.with_metered_host_outcome(work, |work, fuel| host.dispatch(work, fuel))
                    .unwrap()
                    .is_err()
            );
            assert_eq!(fake.state.borrow().display, "1");
            assert_eq!(fake.presses.borrow().as_slice(), ["one"]);
            failed(vm);
        });
    }
    #[test]
    fn bounded_button_catalog_and_profile_fields_do_not_bleed_between_hosts() {
        let fake = Fake::new();
        for source in [
            PROGRAM.replace("button: \"one\"", "button: \"clear\""),
            PROGRAM.replace("name: \"calculator\"", "name: \"document\""),
            PROGRAM.replace("name: \"display\"", "name: \"value\""),
            PROGRAM.replace("gui.press", "gui.set_text"),
        ] {
            assert!(run(&fake, &source).is_err());
        }
        assert_eq!(fake.attempts.get(), 0);
        fake.state.borrow_mut().display = "x".repeat(MAX_SCALAR_STRING_BYTES + 1);
        assert!(run(&fake, PROGRAM).is_err());
        fake.state.borrow_mut().display = "0".into();
        let identity = fake.state.borrow().buttons[0].identity;
        fake.state.borrow_mut().buttons[1].identity = identity;
        assert!(run(&fake, PROGRAM).is_err());
        fake.state.borrow_mut().buttons[1].identity = 21;
        fake.state.borrow_mut().buttons[0].identity = 7;
        assert!(run(&fake, PROGRAM).is_err());
        assert_eq!(fake.attempts.get(), 0);
    }
}
