//! External embedding contract for explicit observation waits. No desktop,
//! native Accessibility call, wall-clock sleep, or background worker is used.
use leselang_macos_ax_host::{
    Error,
    controls::ControlInfo,
    driver,
    host::{Field, Host, ProfileReceipt, ProfileReply, STACK},
    observation_wait::{self, FieldEquals, ObservationWait, WaitError, WaitPolicy, WaitStep},
    scope::{AuxiliaryGrant, AuxiliaryProcessSpec},
    widgets::{
        self, WidgetAction, WidgetCapabilities, WidgetInfo, WidgetScope, WidgetSnapshot,
        WidgetsBackend,
    },
};
use leselang_vm::native::{
    HostWorkOutcome, LocalVm,
    core::{Fuel, MAX_CLOCK_MS, OptionalStringValue, ScalarValue},
    hir::{effect_session::EffectSessionPoll, pure_evaluation::PureValue},
};
use std::{
    cell::{Cell, RefCell},
    time::Duration,
};

const READ: &str = r#"fn main() = bind(target: gui.select(role: "AXTextArea", identifier: "status", label: ""), body: bind(observed: gui.observe(target: target), body: value_or(left: field(value: observed, name: "value"), right: "missing")))"#;
const WRITE_THEN_READ: &str = r#"fn main() = bind(target: gui.select(role: "AXTextArea", identifier: "status", label: ""), body: bind(before: gui.observe(target: target), body: bind(sent: gui.set_text(target: target, expected_revision: field(value: before, name: "revision"), text: "ready"), body: bind(after: gui.observe(target: target), body: value_or(left: field(value: after, name: "value"), right: "missing")))))"#;

struct DelayedBackend {
    state: RefCell<WidgetScope>,
    permitted: Cell<bool>,
    grant: Option<AuxiliaryGrant>,
    delivered: RefCell<Vec<String>>,
    pending_value: RefCell<Option<String>>,
    snapshots: Cell<usize>,
    fail_reads: Cell<bool>,
}
impl DelayedBackend {
    fn new() -> Self {
        Self {
            state: RefCell::new(WidgetScope {
                identity: 1,
                revision: 1,
                widgets: vec![WidgetInfo {
                    control: ControlInfo {
                        identity: 2,
                        key: "fixture/status".into(),
                        role: "AXTextArea".into(),
                        identifier: "status".into(),
                        label: String::new(),
                        value: Some("initial".into()),
                        enabled: true,
                        editable: true,
                        pressable: false,
                    },
                    checked: None,
                    selected: None,
                    position: None,
                    orientation: None,
                    capabilities: WidgetCapabilities::default(),
                    parent: None,
                }],
            }),
            permitted: Cell::new(true),
            grant: None,
            delivered: RefCell::new(Vec::new()),
            pending_value: RefCell::new(None),
            snapshots: Cell::new(0),
            fail_reads: Cell::new(false),
        }
    }
    fn set_value(&self, value: &str) {
        let mut state = self.state.borrow_mut();
        state.widgets[0].control.value = Some(value.into());
        state.revision += 1;
    }
    fn complete_delivery(&self) {
        let value = self.pending_value.borrow_mut().take().unwrap();
        self.set_value(&value);
    }
}
impl WidgetsBackend for DelayedBackend {
    fn snapshot(&self) -> Result<WidgetScope, Error> {
        self.check_permission()?;
        self.snapshots.set(self.snapshots.get() + 1);
        if self.fail_reads.get() {
            return Err(Error::Api(-25204));
        }
        Ok(self.state.borrow().clone())
    }
    fn check_permission(&self) -> Result<(), Error> {
        if !self.permitted.get() {
            return Err(Error::PermissionDenied);
        }
        if let Some(grant) = &self.grant {
            grant.check_active()?;
        }
        Ok(())
    }
    fn perform(
        &self,
        expected: &WidgetScope,
        target: u64,
        action: &WidgetAction,
    ) -> Result<(), Error> {
        self.check_permission()?;
        if *self.state.borrow() != *expected {
            return Err(Error::Stale);
        }
        expected.admit(target, action)?;
        let WidgetAction::SetText(value) = action else {
            return Err(Error::Invalid("fixture supports only text writes"));
        };
        self.delivered.borrow_mut().push(value.clone());
        *self.pending_value.borrow_mut() = Some(value.clone());
        Ok(())
    }
}

fn policy(attempts: u32) -> WaitPolicy {
    WaitPolicy {
        max_attempts: attempts,
        interval: Duration::from_millis(10),
        timeout: Duration::from_millis(50),
    }
}
fn condition() -> FieldEquals {
    FieldEquals {
        field: Field::Value,
        expected: ScalarValue::OptionalString(OptionalStringValue(Some("ready".into()))),
    }
}
fn waiting(step: WaitStep<WidgetSnapshot>) -> ObservationWait<WidgetSnapshot> {
    match step {
        WaitStep::Waiting(wait) => wait,
        WaitStep::Ready(_) => panic!("expected a pending explicit wait"),
    }
}
fn ready(step: WaitStep<WidgetSnapshot>) -> ProfileReceipt<WidgetSnapshot> {
    match step {
        WaitStep::Ready(receipt) => receipt,
        WaitStep::Waiting(_) => panic!("expected a matching observation"),
    }
}
fn expect_error(
    result: Result<WaitStep<WidgetSnapshot>, WaitError>,
    predicate: impl FnOnce(&WaitError) -> bool,
) {
    match result {
        Err(error) => assert!(predicate(&error), "unexpected wait failure: {error:?}"),
        Ok(_) => panic!("expected wait failure"),
    }
}
fn with_receipt<T>(
    backend: &DelayedBackend,
    source: &str,
    prior_effects: usize,
    use_receipt: impl for<'ir, 's> FnOnce(
        &Host<'_, 's, WidgetSnapshot>,
        &mut driver::Vm<'ir, 's, WidgetSnapshot>,
        ProfileReceipt<WidgetSnapshot>,
    ) -> T,
) -> T {
    widgets::with_host(backend, |host, scope| {
        let node = host.compile(source).unwrap();
        let mut vm = LocalVm::new(scope, &node, Vec::new(), Fuel::new(10_000), STACK);
        for _ in 0..prior_effects {
            let work = driver::next(&mut vm, host).unwrap();
            let receipt = driver::dispatch(&mut vm, host, work).unwrap();
            let receipt = if receipt.reply.is_submission() {
                driver::pump(&mut vm, host, receipt).unwrap()
            } else {
                receipt
            };
            driver::accept(&mut vm, host, receipt).unwrap();
        }
        let work = driver::next(&mut vm, host).unwrap();
        let receipt = driver::dispatch(&mut vm, host, work).unwrap();
        use_receipt(host, &mut vm, receipt)
    })
    .unwrap()
}

#[test]
fn delayed_match_uses_explicit_steps_original_fuel_and_historical_observations() {
    let backend = DelayedBackend::new();
    with_receipt(&backend, READ, 1, |host, vm, receipt| {
        let ProfileReply::Snapshot(original) = &receipt.reply else {
            panic!("snapshot")
        };
        let original = original.clone();
        let now = Cell::new(Duration::ZERO);
        let mut clock = || now.get();
        let fuel_before = vm.fuel_remaining();
        let wait = waiting(
            observation_wait::begin(vm, host, receipt, condition(), policy(4), &mut clock).unwrap(),
        );
        assert_eq!(wait.status().attempts, 1);
        assert_eq!(wait.status().next_at, Duration::from_millis(10));
        assert_eq!(wait.status().deadline, Duration::from_millis(50));
        now.set(Duration::from_millis(9));
        let wait = waiting(observation_wait::step(vm, host, wait, &mut clock).unwrap());
        assert_eq!(wait.status().attempts, 1);
        now.set(Duration::from_millis(10));
        let wait = waiting(observation_wait::step(vm, host, wait, &mut clock).unwrap());
        assert_eq!(wait.status().attempts, 2);
        backend.set_value("ready");
        now.set(Duration::from_millis(20));
        let receipt = ready(observation_wait::step(vm, host, wait, &mut clock).unwrap());
        assert!(vm.fuel_remaining() < fuel_before);
        let ProfileReply::Snapshot(matched) = &receipt.reply else {
            panic!("snapshot")
        };
        assert!(matched.revision() > original.revision());
        assert_eq!(
            original.data().scope.widgets[0].control.value.as_deref(),
            Some("initial")
        );
        assert_eq!(
            matched.data().scope.widgets[0].control.value.as_deref(),
            Some("ready")
        );
        backend.set_value("later");
        driver::accept(vm, host, receipt).unwrap();
        assert!(
            matches!(vm.poll(host).unwrap(), EffectSessionPoll::Value(PureValue::Scalar(ScalarValue::String(value))) if value == "ready")
        );
    });
    assert!(backend.delivered.borrow().is_empty());
}

#[test]
fn an_already_matching_receipt_returns_ready_without_implicitly_accepting() {
    let backend = DelayedBackend::new();
    backend.set_value("ready");
    with_receipt(&backend, READ, 1, |host, vm, receipt| {
        // begin compares the dispatched historical snapshot, even if the
        // current native value has already advanced during authorization.
        backend.set_value("later");
        let mut clock = || Duration::ZERO;
        let receipt = ready(
            observation_wait::begin(vm, host, receipt, condition(), policy(1), &mut clock).unwrap(),
        );
        // The helper has not accepted the reply or resumed the source program.
        driver::accept(vm, host, receipt).unwrap();
        assert!(
            matches!(vm.poll(host).unwrap(), EffectSessionPoll::Value(PureValue::Scalar(ScalarValue::String(value))) if value == "ready")
        );
    });
}

#[test]
fn attempt_and_time_limits_are_distinct_terminal_failures() {
    for by_time in [false, true] {
        let backend = DelayedBackend::new();
        with_receipt(&backend, READ, 1, |host, vm, receipt| {
            let now = Cell::new(Duration::ZERO);
            let mut clock = || now.get();
            let wait = waiting(
                observation_wait::begin(vm, host, receipt, condition(), policy(2), &mut clock)
                    .unwrap(),
            );
            now.set(Duration::from_millis(if by_time { 50 } else { 10 }));
            expect_error(
                observation_wait::step(vm, host, wait, &mut clock),
                |error| {
                    if by_time {
                        matches!(error, WaitError::TimedOut)
                    } else {
                        matches!(error, WaitError::AttemptsExhausted)
                    }
                },
            );
            assert!(matches!(
                vm.poll(host).unwrap(),
                EffectSessionPoll::Terminal(_)
            ));
        });
    }
}

#[test]
fn matching_after_the_sample_deadline_does_not_escape_the_time_limit() {
    let backend = DelayedBackend::new();
    with_receipt(&backend, READ, 1, |host, vm, receipt| {
        let mut first_clock = || Duration::ZERO;
        let wait = waiting(
            observation_wait::begin(vm, host, receipt, condition(), policy(4), &mut first_clock)
                .unwrap(),
        );
        backend.set_value("ready");
        let mut calls = 0;
        let mut crossing_clock = || {
            calls += 1;
            Duration::from_millis(if calls == 1 { 10 } else { 51 })
        };
        expect_error(
            observation_wait::step(vm, host, wait, &mut crossing_clock),
            |e| matches!(e, WaitError::TimedOut),
        );
        assert!(calls >= 2);
    });
}

#[test]
fn authority_revocation_is_checked_even_before_the_next_due_time() {
    for authority in 0..3 {
        let grant = AuxiliaryGrant::new(vec![AuxiliaryProcessSpec {
            pid: 42,
            executable_path: "/Test/Service".into(),
            start_seconds: 1000,
            start_micros: 1,
        }])
        .unwrap();
        let mut backend = DelayedBackend::new();
        backend.grant = Some(grant.clone());
        with_receipt(&backend, READ, 1, |host, vm, receipt| {
            let mut clock = || Duration::ZERO;
            let wait = waiting(
                observation_wait::begin(vm, host, receipt, condition(), policy(4), &mut clock)
                    .unwrap(),
            );
            match authority {
                0 => host.set_granted(false),
                1 => backend.permitted.set(false),
                _ => {
                    grant.revoke();
                }
            }
            expect_error(observation_wait::step(vm, host, wait, &mut clock), |e| {
                matches!(e, WaitError::Host(Error::PermissionDenied))
            });
            assert!(backend.delivered.borrow().is_empty());
        });
    }
}

#[test]
fn cancellation_stops_pending_waits_and_rejects_late_matching_receipts() {
    for late_ready in [false, true] {
        let backend = DelayedBackend::new();
        with_receipt(&backend, READ, 1, |host, vm, receipt| {
            let now = Cell::new(Duration::ZERO);
            let mut clock = || now.get();
            let wait = waiting(
                observation_wait::begin(vm, host, receipt, condition(), policy(4), &mut clock)
                    .unwrap(),
            );
            if late_ready {
                backend.set_value("ready");
                now.set(Duration::from_millis(10));
                let receipt = ready(observation_wait::step(vm, host, wait, &mut clock).unwrap());
                assert!(vm.cancel());
                assert!(driver::accept(vm, host, receipt).is_err());
            } else {
                assert!(vm.cancel());
                let fuel = vm.fuel_remaining();
                let reads = backend.snapshots.get();
                expect_error(observation_wait::step(vm, host, wait, &mut clock), |e| {
                    matches!(e, WaitError::InvalidState)
                });
                assert_eq!(vm.fuel_remaining(), fuel);
                assert_eq!(backend.snapshots.get(), reads);
            }
        });
    }
}

#[test]
fn replacement_and_read_errors_end_the_wait_without_reselection_or_retry() {
    for (replacement, step_at_ms) in [(false, 10), (true, 0), (true, 10)] {
        let backend = DelayedBackend::new();
        with_receipt(&backend, READ, 1, |host, vm, receipt| {
            let now = Cell::new(Duration::ZERO);
            let mut clock = || now.get();
            let wait = waiting(
                observation_wait::begin(vm, host, receipt, condition(), policy(4), &mut clock)
                    .unwrap(),
            );
            if replacement {
                let mut state = backend.state.borrow_mut();
                state.widgets[0].control.identity = 3;
                state.widgets[0].control.value = Some("ready".into());
                state.revision += 1;
            } else {
                backend.fail_reads.set(true);
            }
            now.set(Duration::from_millis(step_at_ms));
            expect_error(observation_wait::step(vm, host, wait, &mut clock), |e| {
                matches!(e, WaitError::Host(_))
            });
            assert!(matches!(
                vm.poll(host).unwrap(),
                EffectSessionPoll::Terminal(_)
            ));
        });
        assert!(backend.delivered.borrow().is_empty());
    }
}

#[test]
fn resampling_spends_the_original_vm_fuel_without_a_fresh_budget() {
    let backend = DelayedBackend::new();
    with_receipt(&backend, READ, 1, |host, vm, receipt| {
        let now = Cell::new(Duration::ZERO);
        let mut clock = || now.get();
        let wait = waiting(
            observation_wait::begin(vm, host, receipt, condition(), policy(4), &mut clock).unwrap(),
        );
        vm.with_metered_host_outcome((), |_, fuel| {
            fuel.charge(fuel.remaining()).unwrap();
            HostWorkOutcome::Pending(())
        })
        .unwrap();
        now.set(Duration::from_millis(10));
        expect_error(observation_wait::step(vm, host, wait, &mut clock), |e| {
            matches!(e, WaitError::Host(Error::Fuel))
        });
        assert_eq!(vm.fuel_remaining(), 0);
    });
}

#[test]
fn timeout_preserves_prior_delivery_and_never_repeats_the_mutation() {
    let backend = DelayedBackend::new();
    with_receipt(&backend, WRITE_THEN_READ, 3, |host, vm, receipt| {
        assert_eq!(&*backend.delivered.borrow(), &["ready"]);
        let now = Cell::new(Duration::ZERO);
        let mut clock = || now.get();
        let wait = waiting(
            observation_wait::begin(vm, host, receipt, condition(), policy(2), &mut clock).unwrap(),
        );
        now.set(Duration::from_millis(10));
        expect_error(observation_wait::step(vm, host, wait, &mut clock), |e| {
            matches!(e, WaitError::AttemptsExhausted)
        });
        assert_eq!(&*backend.delivered.borrow(), &["ready"]);
        backend.complete_delivery();
        assert_eq!(
            backend.state.borrow().widgets[0].control.value.as_deref(),
            Some("ready")
        );
    });
}

#[test]
fn a_wait_handle_cannot_be_advanced_with_another_host_or_vm() {
    let backend = DelayedBackend::new();
    let other = DelayedBackend::new();
    for foreign_vm in [false, true] {
        with_receipt(&backend, READ, 1, |host, vm, receipt| {
            let mut clock = || Duration::ZERO;
            let wait = waiting(
                observation_wait::begin(vm, host, receipt, condition(), policy(4), &mut clock)
                    .unwrap(),
            );
            with_receipt(&other, READ, 1, |other_host, other_vm, _| {
                let reads = backend.snapshots.get();
                let other_reads = other.snapshots.get();
                let fuel = vm.fuel_remaining();
                let other_fuel = other_vm.fuel_remaining();
                let result = if foreign_vm {
                    observation_wait::step(other_vm, host, wait, &mut clock)
                } else {
                    observation_wait::step(vm, other_host, wait, &mut clock)
                };
                expect_error(result, |e| matches!(e, WaitError::ForeignSession));
                assert_eq!(vm.fuel_remaining(), fuel);
                assert_eq!(other_vm.fuel_remaining(), other_fuel);
                assert_eq!(backend.snapshots.get(), reads);
                assert_eq!(other.snapshots.get(), other_reads);
                assert!(vm.lease().is_open());
                assert!(other_vm.lease().is_open());
            });
        });
    }
}

#[test]
fn invalid_conditions_policies_and_clock_regression_fail_closed() {
    for invalid in 0..9 {
        let backend = DelayedBackend::new();
        with_receipt(&backend, READ, 1, |host, vm, receipt| {
            let mut policy = policy(4);
            let mut condition = condition();
            match invalid {
                0 => policy.max_attempts = 0,
                1 => policy.interval = Duration::ZERO,
                2 => policy.interval = Duration::from_millis(100),
                3 => policy.max_attempts = 65,
                4 => policy.interval = Duration::from_millis(1001),
                5 => policy.timeout = Duration::from_secs(31),
                6 => policy.timeout = Duration::ZERO,
                7 => condition.field = Field::Document,
                _ => condition.expected = ScalarValue::String("ready".into()),
            }
            let mut clock = || Duration::ZERO;
            expect_error(
                observation_wait::begin(vm, host, receipt, condition, policy, &mut clock),
                |e| {
                    if invalid >= 7 {
                        matches!(e, WaitError::InvalidCondition)
                    } else {
                        matches!(e, WaitError::InvalidPolicy)
                    }
                },
            );
        });
    }
    let backend = DelayedBackend::new();
    with_receipt(&backend, READ, 1, |host, vm, receipt| {
        let now = Cell::new(Duration::from_millis(10));
        let mut clock = || now.get();
        let wait = waiting(
            observation_wait::begin(vm, host, receipt, condition(), policy(4), &mut clock).unwrap(),
        );
        now.set(Duration::from_millis(9));
        expect_error(observation_wait::step(vm, host, wait, &mut clock), |e| {
            matches!(e, WaitError::ClockRegression)
        });
    });
}

#[test]
fn ready_cannot_restart_the_same_request_with_a_new_attempt_or_time_budget() {
    let backend = DelayedBackend::new();
    backend.set_value("ready");
    with_receipt(&backend, READ, 1, |host, vm, receipt| {
        let mut clock = || Duration::ZERO;
        let receipt = ready(
            observation_wait::begin(vm, host, receipt, condition(), policy(1), &mut clock).unwrap(),
        );
        expect_error(
            observation_wait::begin(vm, host, receipt, condition(), policy(64), &mut clock),
            |error| matches!(error, WaitError::Host(Error::Invalid(_))),
        );
        assert!(matches!(
            vm.poll(host).unwrap(),
            EffectSessionPoll::Terminal(_)
        ));
    });
}

#[test]
fn a_new_observe_request_can_begin_its_own_wait_after_accepting_ready() {
    const TWICE: &str = r#"fn main() = bind(target: gui.select(role: "AXTextArea", identifier: "status", label: ""), body: bind(first: gui.observe(target: target), body: bind(second: gui.observe(target: target), body: value_or(left: field(value: second, name: "value"), right: "missing"))))"#;
    let backend = DelayedBackend::new();
    backend.set_value("ready");
    with_receipt(&backend, TWICE, 1, |host, vm, receipt| {
        let mut clock = || Duration::ZERO;
        let receipt = ready(
            observation_wait::begin(vm, host, receipt, condition(), policy(1), &mut clock).unwrap(),
        );
        driver::accept(vm, host, receipt).unwrap();
        let work = driver::next(vm, host).unwrap();
        let receipt = driver::dispatch(vm, host, work).unwrap();
        let receipt = ready(
            observation_wait::begin(vm, host, receipt, condition(), policy(1), &mut clock).unwrap(),
        );
        driver::accept(vm, host, receipt).unwrap();
        assert!(matches!(
            vm.poll(host).unwrap(),
            EffectSessionPoll::Value(_)
        ));
    });
}

#[test]
fn only_observation_receipts_are_waitable_and_queued_writes_are_never_pumped() {
    for mutation in [false, true] {
        let backend = DelayedBackend::new();
        let (source, prior_effects) = if mutation {
            (WRITE_THEN_READ, 2)
        } else {
            (READ, 0)
        };
        with_receipt(&backend, source, prior_effects, |host, vm, receipt| {
            assert_eq!(host.has_pending(), mutation);
            let mut clock = || Duration::ZERO;
            expect_error(
                observation_wait::begin(vm, host, receipt, condition(), policy(4), &mut clock),
                |error| matches!(error, WaitError::Host(Error::Invalid(_))),
            );
            assert!(backend.delivered.borrow().is_empty());
            assert!(matches!(
                vm.poll(host).unwrap(),
                EffectSessionPoll::Terminal(_)
            ));
        });
    }
}

#[test]
fn a_ready_receipt_still_requires_live_authority_when_explicitly_accepted() {
    for authority in 0..3 {
        let grant = AuxiliaryGrant::new(vec![AuxiliaryProcessSpec {
            pid: 42,
            executable_path: "/Test/Service".into(),
            start_seconds: 1000,
            start_micros: 1,
        }])
        .unwrap();
        let mut backend = DelayedBackend::new();
        backend.grant = Some(grant.clone());
        backend.set_value("ready");
        with_receipt(&backend, READ, 1, |host, vm, receipt| {
            let mut clock = || Duration::ZERO;
            let receipt = ready(
                observation_wait::begin(vm, host, receipt, condition(), policy(1), &mut clock)
                    .unwrap(),
            );
            match authority {
                0 => host.set_granted(false),
                1 => backend.permitted.set(false),
                _ => {
                    grant.revoke();
                }
            }
            assert!(driver::accept(vm, host, receipt).is_err());
        });
    }
}

#[test]
fn a_delayed_write_can_be_verified_without_delivering_it_twice() {
    let backend = DelayedBackend::new();
    with_receipt(&backend, WRITE_THEN_READ, 3, |host, vm, receipt| {
        assert_eq!(&*backend.delivered.borrow(), &["ready"]);
        let now = Cell::new(Duration::ZERO);
        let mut clock = || now.get();
        let wait = waiting(
            observation_wait::begin(vm, host, receipt, condition(), policy(4), &mut clock).unwrap(),
        );
        now.set(Duration::from_millis(10));
        let wait = waiting(observation_wait::step(vm, host, wait, &mut clock).unwrap());
        backend.complete_delivery();
        now.set(Duration::from_millis(20));
        let receipt = ready(observation_wait::step(vm, host, wait, &mut clock).unwrap());
        driver::accept(vm, host, receipt).unwrap();
        assert!(
            matches!(vm.poll(host).unwrap(), EffectSessionPoll::Value(PureValue::Scalar(ScalarValue::String(value))) if value == "ready")
        );
        assert_eq!(&*backend.delivered.borrow(), &["ready"]);
    });
}

#[test]
fn clock_range_and_deadline_arithmetic_fail_without_sampling_again() {
    for overflow in [false, true] {
        let backend = DelayedBackend::new();
        with_receipt(&backend, READ, 1, |host, vm, receipt| {
            let reads = backend.snapshots.get();
            let mut clock = || Duration::from_millis(MAX_CLOCK_MS + u64::from(!overflow));
            expect_error(
                observation_wait::begin(vm, host, receipt, condition(), policy(4), &mut clock),
                |error| {
                    if overflow {
                        matches!(error, WaitError::ClockOverflow)
                    } else {
                        matches!(error, WaitError::ClockRange)
                    }
                },
            );
            assert_eq!(backend.snapshots.get(), reads);
            assert!(matches!(
                vm.poll(host).unwrap(),
                EffectSessionPoll::Terminal(_)
            ));
        });
    }
}

#[test]
fn an_initial_match_returning_at_the_deadline_is_too_late() {
    let backend = DelayedBackend::new();
    backend.set_value("ready");
    with_receipt(&backend, READ, 1, |host, vm, receipt| {
        let mut calls = 0;
        let mut clock = || {
            calls += 1;
            Duration::from_millis(if calls == 1 { 0 } else { 50 })
        };
        expect_error(
            observation_wait::begin(vm, host, receipt, condition(), policy(1), &mut clock),
            |error| matches!(error, WaitError::TimedOut),
        );
        assert_eq!(calls, 2);
        assert!(matches!(
            vm.poll(host).unwrap(),
            EffectSessionPoll::Terminal(_)
        ));
    });
}
