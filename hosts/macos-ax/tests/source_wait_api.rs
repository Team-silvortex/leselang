//! Source-declared waits through public embedding APIs, with a simulated GUI.
//! Tests deliberately use direct VM acceptance as well as the convenience driver.
use leselang_macos_ax_host::{
    Error,
    controls::ControlInfo,
    driver,
    host::{Field, Host, ProfileReceipt, ProfileReply, STACK, Work},
    observation_wait::{self, FieldEquals, ObservationWait, WaitError, WaitPolicy, WaitStep},
    scope::{AuxiliaryGrant, AuxiliaryProcessSpec},
    widgets::{
        self, WidgetAction, WidgetCapabilities, WidgetInfo, WidgetScope, WidgetSnapshot,
        WidgetsBackend,
    },
};
use leselang_vm::native::{
    HostWorkOutcome, LocalVm,
    core::{Fuel, OptionalStringValue, ScalarValue, host_protocol::Observation},
    hir::{effect_session::EffectSessionPoll, pure_evaluation::PureValue},
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

const CALL: &str = r#"gui.wait_value(target: target, expected: optional_string(value: "ready"), max_attempts: 4, interval_ms: 10, timeout_ms: 50)"#;
const WAIT: &str = r#"fn main() = bind(target: gui.select(role: "AXTextArea", identifier: "status", label: ""), body: bind(after: gui.wait_value(target: target, expected: optional_string(value: "ready"), max_attempts: 4, interval_ms: 10, timeout_ms: 50), body: value_or(left: field(value: after, name: "value"), right: "missing")))"#;
const OBSERVE_WAIT: &str = r#"fn main() = bind(target: gui.select(role: "AXTextArea", identifier: "status", label: ""), body: bind(before: gui.observe(target: target), body: bind(after: gui.wait_value(target: target, expected: optional_string(value: "ready"), max_attempts: 4, interval_ms: 10, timeout_ms: 50), body: value_or(left: field(value: after, name: "value"), right: "missing"))))"#;
const WRITE_WAIT: &str = r#"fn main() = bind(target: gui.select(role: "AXTextArea", identifier: "status", label: ""), body: bind(before: gui.observe(target: target), body: bind(sent: gui.set_text(target: target, expected_revision: field(value: before, name: "revision"), text: "ready"), body: bind(after: gui.wait_value(target: target, expected: optional_string(value: "ready"), max_attempts: 4, interval_ms: 10, timeout_ms: 50), body: value_or(left: field(value: after, name: "value"), right: "missing")))))"#;

struct Window {
    state: RefCell<WidgetScope>,
    reads: Cell<usize>,
    permitted: Cell<bool>,
    grant: Option<AuxiliaryGrant>,
    fail_read: Cell<bool>,
    writes: RefCell<Vec<String>>,
}
impl Window {
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
            reads: Cell::new(0),
            permitted: Cell::new(true),
            grant: None,
            fail_read: Cell::new(false),
            writes: RefCell::new(Vec::new()),
        }
    }
    fn value(&self, value: Option<&str>) {
        let mut state = self.state.borrow_mut();
        state.widgets[0].control.value = value.map(str::to_owned);
        state.revision += 1;
    }
}
impl WidgetsBackend for Window {
    fn snapshot(&self) -> Result<WidgetScope, Error> {
        self.check_permission()?;
        self.reads.set(self.reads.get() + 1);
        if self.fail_read.get() {
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
        if expected != &*self.state.borrow() {
            return Err(Error::Stale);
        }
        expected.admit(target, action)?;
        let WidgetAction::SetText(value) = action else {
            return Err(Error::Invalid("unexpected fixture action"));
        };
        // Acknowledgement is immediate, visible completion is independently delayed.
        self.writes.borrow_mut().push(value.clone());
        Ok(())
    }
}

fn waiting(step: WaitStep<WidgetSnapshot>) -> ObservationWait<WidgetSnapshot> {
    match step {
        WaitStep::Waiting(wait) => wait,
        WaitStep::Ready(_) => panic!("expected Waiting"),
    }
}
fn ready(step: WaitStep<WidgetSnapshot>) -> ProfileReceipt<WidgetSnapshot> {
    match step {
        WaitStep::Ready(receipt) => receipt,
        WaitStep::Waiting(_) => panic!("expected Ready"),
    }
}
fn error(
    result: Result<WaitStep<WidgetSnapshot>, WaitError>,
    expected: impl FnOnce(&WaitError) -> bool,
) {
    match result {
        Err(error) => assert!(expected(&error), "unexpected failure: {error:?}"),
        Ok(_) => panic!("expected failure"),
    }
}
fn finish(
    host: &Host<'_, '_, WidgetSnapshot>,
    vm: &mut driver::Vm<'_, '_, WidgetSnapshot>,
    receipt: ProfileReceipt<WidgetSnapshot>,
) {
    driver::accept(vm, host, receipt).unwrap();
}
fn with_work<T>(
    backend: &Window,
    source: &str,
    preceding: usize,
    use_work: impl for<'ir, 's> FnOnce(
        &Host<'_, 's, WidgetSnapshot>,
        &mut driver::Vm<'ir, 's, WidgetSnapshot>,
        Work<'s, WidgetSnapshot>,
        Vec<Rc<Observation<WidgetSnapshot>>>,
    ) -> T,
) -> T {
    widgets::with_wait_host(backend, |host, scope| {
        let node = host.compile(source).unwrap();
        let mut vm = LocalVm::new(scope, &node, Vec::new(), Fuel::new(20_000), STACK);
        let mut history = Vec::new();
        for _ in 0..preceding {
            let work = driver::next(&mut vm, host).unwrap();
            let receipt = driver::dispatch(&mut vm, host, work).unwrap();
            if let ProfileReply::Snapshot(snapshot) = &receipt.reply {
                history.push(snapshot.clone());
            }
            let receipt = if receipt.reply.is_submission() {
                driver::pump(&mut vm, host, receipt).unwrap()
            } else {
                receipt
            };
            driver::accept(&mut vm, host, receipt).unwrap();
        }
        let work = driver::next(&mut vm, host).unwrap();
        use_work(host, &mut vm, work, history)
    })
    .unwrap()
}

#[test]
fn old_widgets_catalog_rejects_source_waits_in_active_and_cold_positions() {
    let backend = Window::new();
    let cold = WAIT.replace(
        CALL,
        &format!("choose(when: true, then: gui.observe(target: target), otherwise: {CALL})"),
    );
    let unused = format!(
        "fn unused() = {}\nfn main() = true",
        WAIT.strip_prefix("fn main() = ").unwrap()
    );
    for source in [WAIT, &cold, &unused] {
        widgets::with_host(&backend, |host, _scope| {
            let reads = backend.reads.get();
            assert!(host.compile(source).is_err());
            assert_eq!(backend.reads.get(), reads);
        })
        .unwrap();
        widgets::with_wait_host(&backend, |host, _scope| {
            let reads = backend.reads.get();
            host.compile(source).unwrap();
            assert_eq!(backend.reads.get(), reads);
        })
        .unwrap();
    }
    assert!(backend.writes.borrow().is_empty());
}

#[test]
fn source_wait_requires_five_named_typed_arguments_without_coercion() {
    let backend = Window::new();
    widgets::with_wait_host(&backend, |host, _scope| {
        host.compile(WAIT).unwrap();
        host.compile(&WAIT.replace(
            "optional_string(value: \"ready\")",
            "optional_string(value: none)",
        ))
        .unwrap();
        for (from, to) in [
            (
                "target: target, expected",
                "target: \"fixture/status\", expected",
            ),
            ("optional_string(value: \"ready\")", "\"ready\""),
            ("max_attempts: 4", "max_attempts: true"),
            ("interval_ms: 10", "interval_ms: \"10\""),
            (", timeout_ms: 50", ""),
            ("timeout_ms: 50", "timeout_ms: 50, timeout_ms: 50"),
            ("timeout_ms: 50", "timeout_ms: 50, scheduler: false"),
        ] {
            assert!(
                host.compile(&WAIT.replace(from, to)).is_err(),
                "accepted {to}"
            );
        }
    })
    .unwrap();
}

#[test]
fn source_declared_match_resumes_only_after_an_explicit_due_step_and_accept() {
    let backend = Window::new();
    with_work(&backend, WAIT, 1, |host, vm, work, _| {
        let now = Cell::new(Duration::ZERO);
        let mut clock = || now.get();
        let fuel = vm.fuel_remaining();
        let wait = waiting(observation_wait::begin_source(vm, host, work, &mut clock).unwrap());
        assert_eq!(wait.status().attempts, 1);
        assert_eq!(wait.status().deadline, Duration::from_millis(50));
        backend.value(Some("ready"));
        now.set(Duration::from_millis(9));
        let wait = waiting(observation_wait::step(vm, host, wait, &mut clock).unwrap());
        assert_eq!(wait.status().attempts, 1);
        now.set(Duration::from_millis(10));
        let receipt = ready(observation_wait::step(vm, host, wait, &mut clock).unwrap());
        assert!(vm.fuel_remaining() < fuel);
        assert!(matches!(
            vm.poll(host).unwrap(),
            EffectSessionPoll::Awaiting
        ));
        backend.value(Some("later"));
        finish(host, vm, receipt);
        assert!(
            matches!(vm.poll(host).unwrap(), EffectSessionPoll::Value(PureValue::Scalar(ScalarValue::String(value))) if value == "ready")
        );
    });
}

#[test]
fn missing_and_empty_values_have_distinct_source_conditions() {
    let backend = Window::new();
    for missing in [false, true] {
        backend.value(if missing { Some("") } else { None });
        let source = WAIT.replace(
            "optional_string(value: \"ready\")",
            if missing {
                "optional_string(value: none)"
            } else {
                "optional_string(value: \"\")"
            },
        );
        with_work(&backend, &source, 1, |host, vm, work, _| {
            let wait = waiting(
                observation_wait::begin_source(vm, host, work, &mut || Duration::ZERO).unwrap(),
            );
            backend.value(if missing { None } else { Some("") });
            let receipt = ready(
                observation_wait::step(vm, host, wait, &mut || Duration::from_millis(10)).unwrap(),
            );
            finish(host, vm, receipt);
            assert!(
                matches!(vm.poll(host).unwrap(), EffectSessionPoll::Value(PureValue::Scalar(ScalarValue::String(value))) if value == if missing { "missing" } else { "" })
            );
        });
    }
}

#[test]
fn ordinary_dispatch_cannot_complete_even_an_immediately_matching_source_wait() {
    let backend = Window::new();
    backend.value(Some("ready"));
    with_work(&backend, WAIT, 1, |host, vm, work, _| {
        assert!(driver::dispatch(vm, host, work).is_err());
        assert!(matches!(
            vm.poll(host).unwrap(),
            EffectSessionPoll::Terminal(_)
        ));
    });
}

#[test]
fn direct_accept_and_historical_receipt_substitution_cannot_bypass_readiness() {
    let backend = Window::new();
    backend.value(Some("ready"));
    with_work(&backend, OBSERVE_WAIT, 2, |host, vm, work, history| {
        let historical = history[0].clone();
        let identity = work.identity().clone();
        backend.value(Some("initial"));
        assert!(
            vm.try_accept(&identity, ProfileReply::Snapshot(historical.clone()), host)
                .is_err()
        );
        assert!(vm.lease().is_open());
        let wait = waiting(
            observation_wait::begin_source(vm, host, work, &mut || Duration::ZERO).unwrap(),
        );
        assert!(
            vm.try_accept(&identity, ProfileReply::Snapshot(historical.clone()), host)
                .is_err()
        );
        backend.value(Some("ready"));
        let receipt = ready(
            observation_wait::step(vm, host, wait, &mut || Duration::from_millis(10)).unwrap(),
        );
        assert!(
            vm.try_accept(&receipt.identity, ProfileReply::Snapshot(historical), host)
                .is_err()
        );
        let ProfileReply::Snapshot(matched) = &receipt.reply else {
            panic!("snapshot")
        };
        let same_data = host
            .owner()
            .observe(matched.object(), &host.lease(), matched.data().clone())
            .unwrap();
        assert_eq!(same_data.revision(), matched.revision());
        assert_eq!(same_data.data(), matched.data());
        assert!(
            vm.try_accept(
                &receipt.identity,
                ProfileReply::Snapshot(Rc::new(same_data)),
                host
            )
            .is_err()
        );
        finish(host, vm, receipt);
        assert!(matches!(
            vm.poll(host).unwrap(),
            EffectSessionPoll::Value(_)
        ));
    });
}

#[test]
fn a_previous_ready_proof_does_not_authorize_a_new_wait_request() {
    let backend = Window::new();
    backend.value(Some("ready"));
    let source = WAIT.replace("body: value_or(left: field(value: after, name: \"value\"), right: \"missing\")", &format!("body: bind(final: {CALL}, body: value_or(left: field(value: final, name: \"value\"), right: \"missing\"))"));
    with_work(&backend, &source, 1, |host, vm, work, _| {
        let receipt =
            ready(observation_wait::begin_source(vm, host, work, &mut || Duration::ZERO).unwrap());
        let old_identity = receipt.identity.clone();
        let ProfileReply::Snapshot(old) = &receipt.reply else {
            panic!("snapshot")
        };
        let old = old.clone();
        finish(host, vm, receipt);
        let next = driver::next(vm, host).unwrap();
        assert!(
            vm.try_accept(&old_identity, ProfileReply::Snapshot(old.clone()), host)
                .is_err()
        );
        assert!(
            vm.try_accept(next.identity(), ProfileReply::Snapshot(old), host)
                .is_err()
        );
        let receipt =
            ready(observation_wait::begin_source(vm, host, next, &mut || Duration::ZERO).unwrap());
        finish(host, vm, receipt);
        assert!(matches!(
            vm.poll(host).unwrap(),
            EffectSessionPoll::Value(_)
        ));
    });
}

#[test]
fn old_host_side_begin_cannot_override_source_expected_value_or_policy() {
    let backend = Window::new();
    with_work(&backend, OBSERVE_WAIT, 2, |host, vm, work, history| {
        let forged = ProfileReceipt {
            identity: work.identity().clone(),
            reply: ProfileReply::Snapshot(history[0].clone()),
        };
        error(
            observation_wait::begin(
                vm,
                host,
                forged,
                FieldEquals {
                    field: Field::Value,
                    expected: ScalarValue::OptionalString(OptionalStringValue(Some(
                        "initial".into(),
                    ))),
                },
                WaitPolicy::default(),
                &mut || Duration::ZERO,
            ),
            |error| matches!(error, WaitError::Host(Error::Invalid(_))),
        );
    });
}

#[test]
fn source_policy_bounds_are_checked_before_native_wait_sampling() {
    for (from, to) in [
        ("max_attempts: 4", "max_attempts: 0"),
        ("max_attempts: 4", "max_attempts: 65"),
        ("max_attempts: 4", "max_attempts: 4294967297"),
        ("interval_ms: 10", "interval_ms: 0"),
        ("interval_ms: 10", "interval_ms: 1001"),
        ("interval_ms: 10", "interval_ms: 51"),
        ("timeout_ms: 50", "timeout_ms: 0"),
        ("timeout_ms: 50", "timeout_ms: 30001"),
    ] {
        let backend = Window::new();
        with_work(&backend, &WAIT.replace(from, to), 1, |host, vm, work, _| {
            let reads = backend.reads.get();
            error(
                observation_wait::begin_source(vm, host, work, &mut || Duration::ZERO),
                |error| matches!(error, WaitError::InvalidPolicy),
            );
            assert_eq!(backend.reads.get(), reads);
            assert!(matches!(
                vm.poll(host).unwrap(),
                EffectSessionPoll::Terminal(_)
            ));
        });
    }
}

#[test]
fn original_fuel_governs_source_begin_and_every_later_step() {
    for after_begin in [false, true] {
        let backend = Window::new();
        with_work(&backend, WAIT, 1, |host, vm, work, _| {
            let drain = |vm: &mut driver::Vm<'_, '_, WidgetSnapshot>| {
                vm.with_metered_host_outcome((), |_, fuel| {
                    fuel.charge(fuel.remaining()).unwrap();
                    HostWorkOutcome::Pending(())
                })
                .unwrap();
            };
            let result = if after_begin {
                let wait = waiting(
                    observation_wait::begin_source(vm, host, work, &mut || Duration::ZERO).unwrap(),
                );
                drain(vm);
                observation_wait::step(vm, host, wait, &mut || Duration::from_millis(10))
            } else {
                drain(vm);
                observation_wait::begin_source(vm, host, work, &mut || Duration::ZERO)
            };
            error(result, |error| {
                matches!(error, WaitError::Host(Error::Fuel))
            });
            assert_eq!(vm.fuel_remaining(), 0);
            assert!(matches!(
                vm.poll(host).unwrap(),
                EffectSessionPoll::Terminal(_)
            ));
        });
    }
}

#[test]
fn initial_native_sample_is_included_in_the_source_deadline() {
    let backend = Window::new();
    backend.value(Some("ready"));
    with_work(&backend, WAIT, 1, |host, vm, work, _| {
        let mut clocks = 0;
        let mut clock = || {
            clocks += 1;
            Duration::from_millis(if clocks == 1 { 0 } else { 50 })
        };
        error(
            observation_wait::begin_source(vm, host, work, &mut clock),
            |error| matches!(error, WaitError::TimedOut),
        );
        assert!(clocks >= 2);
        assert!(matches!(
            vm.poll(host).unwrap(),
            EffectSessionPoll::Terminal(_)
        ));
    });
}

#[test]
fn source_attempt_limit_and_deadline_remain_distinct_terminal_failures() {
    for deadline in [false, true] {
        let backend = Window::new();
        with_work(
            &backend,
            &WAIT.replace("max_attempts: 4", "max_attempts: 2"),
            1,
            |host, vm, work, _| {
                let wait = waiting(
                    observation_wait::begin_source(vm, host, work, &mut || Duration::ZERO).unwrap(),
                );
                error(
                    observation_wait::step(vm, host, wait, &mut || {
                        Duration::from_millis(if deadline { 50 } else { 10 })
                    }),
                    |error| {
                        if deadline {
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
            },
        );
    }
}

#[test]
fn live_authority_is_required_before_due_and_again_at_ready_acceptance() {
    for before_due in [false, true] {
        for authority in 0..3 {
            let grant = AuxiliaryGrant::new(vec![AuxiliaryProcessSpec {
                pid: 42,
                executable_path: "/Test/Service".into(),
                start_seconds: 1000,
                start_micros: 1,
            }])
            .unwrap();
            let mut backend = Window::new();
            backend.grant = Some(grant.clone());
            if !before_due {
                backend.value(Some("ready"));
            }
            with_work(&backend, WAIT, 1, |host, vm, work, _| {
                let step =
                    observation_wait::begin_source(vm, host, work, &mut || Duration::ZERO).unwrap();
                match authority {
                    0 => host.set_granted(false),
                    1 => backend.permitted.set(false),
                    _ => {
                        grant.revoke();
                    }
                }
                if before_due {
                    error(
                        observation_wait::step(vm, host, waiting(step), &mut || Duration::ZERO),
                        |error| matches!(error, WaitError::Host(Error::PermissionDenied)),
                    );
                } else {
                    assert!(driver::accept(vm, host, ready(step)).is_err());
                }
            });
        }
    }
}

#[test]
fn cancellation_refuses_unstarted_waits_pending_steps_and_late_ready_receipts() {
    for stage in 0..3 {
        let backend = Window::new();
        if stage == 2 {
            backend.value(Some("ready"));
        }
        with_work(&backend, WAIT, 1, |host, vm, work, _| {
            if stage == 0 {
                assert!(vm.cancel());
                let fuel = vm.fuel_remaining();
                let reads = backend.reads.get();
                error(
                    observation_wait::begin_source(vm, host, work, &mut || {
                        panic!("cancelled wait read clock")
                    }),
                    |error| matches!(error, WaitError::InvalidState),
                );
                assert_eq!(vm.fuel_remaining(), fuel);
                assert_eq!(backend.reads.get(), reads);
            } else {
                let step =
                    observation_wait::begin_source(vm, host, work, &mut || Duration::ZERO).unwrap();
                assert!(vm.cancel());
                if stage == 1 {
                    error(
                        observation_wait::step(vm, host, waiting(step), &mut || {
                            panic!("cancelled step read clock")
                        }),
                        |error| matches!(error, WaitError::InvalidState),
                    );
                } else {
                    assert!(driver::accept(vm, host, ready(step)).is_err());
                }
            }
        });
    }
}

#[test]
fn native_identity_replacement_or_read_failure_never_causes_reselection() {
    for replaced in [false, true] {
        let backend = Window::new();
        with_work(&backend, WAIT, 1, |host, vm, work, _| {
            let wait = waiting(
                observation_wait::begin_source(vm, host, work, &mut || Duration::ZERO).unwrap(),
            );
            if replaced {
                backend.state.borrow_mut().widgets[0].control.identity = 3;
                backend.value(Some("ready"));
            } else {
                backend.fail_read.set(true);
            }
            error(
                observation_wait::step(vm, host, wait, &mut || Duration::from_millis(10)),
                |error| matches!(error, WaitError::Host(_)),
            );
            assert!(matches!(
                vm.poll(host).unwrap(),
                EffectSessionPoll::Terminal(_)
            ));
        });
        assert!(backend.writes.borrow().is_empty());
    }
}

#[test]
fn source_waits_never_repeat_or_undo_an_acknowledged_mutation() {
    for succeeds in [false, true] {
        let backend = Window::new();
        with_work(&backend, WRITE_WAIT, 3, |host, vm, work, _| {
            assert_eq!(&*backend.writes.borrow(), &["ready"]);
            let wait = waiting(
                observation_wait::begin_source(vm, host, work, &mut || Duration::ZERO).unwrap(),
            );
            if succeeds {
                backend.value(Some("ready"));
                let receipt = ready(
                    observation_wait::step(vm, host, wait, &mut || Duration::from_millis(10))
                        .unwrap(),
                );
                finish(host, vm, receipt);
                assert!(matches!(
                    vm.poll(host).unwrap(),
                    EffectSessionPoll::Value(_)
                ));
            } else {
                error(
                    observation_wait::step(vm, host, wait, &mut || Duration::from_millis(50)),
                    |error| matches!(error, WaitError::TimedOut),
                );
                // A timed-out observation cannot undo remote completion later.
                backend.value(Some("ready"));
            }
            assert_eq!(&*backend.writes.borrow(), &["ready"]);
        });
    }
}

#[test]
fn foreign_host_vm_and_request_are_rejected_before_clock_fuel_or_native_reads() {
    for foreign in 0..3 {
        let backend = Window::new();
        let other = Window::new();
        with_work(&backend, WAIT, 1, |host, vm, work, _| {
            with_work(&other, WAIT, 1, |other_host, other_vm, other_work, _| {
                let fuel = vm.fuel_remaining();
                let other_fuel = other_vm.fuel_remaining();
                let reads = backend.reads.get();
                let other_reads = other.reads.get();
                let mut clock = || panic!("foreign request read the clock");
                let result = match foreign {
                    0 => observation_wait::begin_source(vm, other_host, work, &mut clock),
                    1 => observation_wait::begin_source(other_vm, host, work, &mut clock),
                    _ => observation_wait::begin_source(vm, host, other_work, &mut clock),
                };
                error(result, |error| matches!(error, WaitError::ForeignSession));
                assert_eq!(vm.fuel_remaining(), fuel);
                assert_eq!(other_vm.fuel_remaining(), other_fuel);
                assert_eq!(backend.reads.get(), reads);
                assert_eq!(other.reads.get(), other_reads);
                assert!(vm.lease().is_open());
                assert!(other_vm.lease().is_open());
            });
        });
    }
}

#[test]
fn source_wait_entry_does_not_dispatch_an_ordinary_observation_or_mutation() {
    for preceding in [1, 2] {
        let backend = Window::new();
        with_work(&backend, WRITE_WAIT, preceding, |host, vm, work, _| {
            error(
                observation_wait::begin_source(vm, host, work, &mut || Duration::ZERO),
                |error| matches!(error, WaitError::Host(Error::Invalid(_))),
            );
            assert!(!host.has_pending());
            assert!(backend.writes.borrow().is_empty());
        });
    }
}

#[test]
fn an_expected_value_computed_from_an_observation_is_frozen_for_the_request() {
    let backend = Window::new();
    let source = OBSERVE_WAIT.replace(
        "optional_string(value: \"ready\")",
        "field(value: before, name: \"value\")",
    );
    with_work(&backend, &source, 2, |host, vm, work, _| {
        backend.value(Some("ready"));
        let wait = waiting(
            observation_wait::begin_source(vm, host, work, &mut || Duration::ZERO).unwrap(),
        );
        backend.value(Some("initial"));
        let receipt = ready(
            observation_wait::step(vm, host, wait, &mut || Duration::from_millis(10)).unwrap(),
        );
        finish(host, vm, receipt);
        assert!(
            matches!(vm.poll(host).unwrap(), EffectSessionPoll::Value(PureValue::Scalar(ScalarValue::String(value))) if value == "initial")
        );
    });
}
