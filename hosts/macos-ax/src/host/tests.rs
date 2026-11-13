use super::*;
use crate::driver::{self, Vm};
use leselang_vm::native::{
    HostWorkOutcome, LocalVm,
    hir::effect_session::{EffectSessionEnd, EffectSessionPoll, EffectSessionStatus},
};
use pure_evaluation::PureEvaluationEnvironment;

struct Fake {
    state: RefCell<Snapshot>,
    permitted: Cell<bool>,
    available: Cell<bool>,
    writes: Cell<usize>,
    outcome: Cell<u8>,
}

#[test]
fn offline_description_matches_live_document_catalog_without_authority() {
    let fake = Fake::new();
    with_host(&fake, |host, _| {
        host.set_granted(false);
        fake.permitted.set(false);
        catalog::assert_live_description(host, crate::discovery::Profile::TextEdit);
        assert!(
            host.compile("fn main() = gui.find(name: \"document\")")
                .is_err()
        );
    })
    .unwrap();
    assert_eq!(fake.writes.get(), 0);
}
impl Fake {
    fn new() -> Self {
        Self {
            state: RefCell::new(Snapshot {
                identity: 1,
                document: "file:///tmp/leselang-test.txt".into(),
                role: "AXTextArea".into(),
                value: INITIAL_TEXT.into(),
                editable: true,
            }),
            permitted: Cell::new(true),
            available: Cell::new(true),
            writes: Cell::new(0),
            outcome: Cell::new(0),
        }
    }
    fn value(&self) -> String {
        self.state.borrow().value.clone()
    }
}
impl Backend for Fake {
    fn snapshot(&self) -> Result<Snapshot, Error> {
        self.check_permission()?;
        if !self.available.get() {
            return Err(Error::Unavailable("document closed"));
        }
        Ok(self.state.borrow().clone())
    }
    fn check_permission(&self) -> Result<(), Error> {
        if self.permitted.get() {
            Ok(())
        } else {
            Err(Error::PermissionDenied)
        }
    }
    fn set_text(&self, expected: &Snapshot, text: &str) -> Result<(), Error> {
        // Simulate a remote change after the host's last snapshot but before
        // the bridge's immediate comparison.
        if self.outcome.get() == 3 {
            self.state.borrow_mut().value = "external race".into();
        }
        if self.snapshot()? != *expected {
            return Err(Error::Stale);
        }
        if !expected.editable {
            return Err(Error::NotEditable);
        }
        self.writes.set(self.writes.get() + 1);
        if self.outcome.get() != 2 {
            self.state.borrow_mut().value = text.into();
        }
        if self.outcome.get() == 1 {
            Err(Error::Uncertain(-25204))
        } else {
            Ok(())
        }
    }
}
fn session(
    fake: &Fake,
    source: &str,
    body: impl for<'ir, 's> FnOnce(&Host<'_, 's>, &mut Vm<'ir, 's>),
) {
    with_host(fake, |host, scope| {
        let node = host.compile(source).unwrap();
        let mut vm = LocalVm::new(scope, &node, Vec::new(), Fuel::new(1000), STACK);
        body(host, &mut vm);
    })
    .unwrap();
}
fn find<'ir, 's>(host: &Host<'_, 's>, vm: &mut Vm<'ir, 's>) -> ObjectRef {
    let work = driver::next(vm, host).unwrap();
    assert_eq!(work.operation(), "gui.find");
    let receipt = driver::dispatch(vm, host, work).unwrap();
    let Reply::Object(object) = &receipt.reply else {
        panic!("expected object")
    };
    let object = object.clone();
    driver::accept(vm, host, receipt).unwrap();
    object
}
fn observe<'ir, 's>(host: &Host<'_, 's>, vm: &mut Vm<'ir, 's>) -> Rc<Observation<Snapshot>> {
    let work = driver::next(vm, host).unwrap();
    assert_eq!(work.operation(), "gui.observe");
    let receipt = driver::dispatch(vm, host, work).unwrap();
    let Reply::Snapshot(snapshot) = &receipt.reply else {
        panic!("expected snapshot")
    };
    let snapshot = snapshot.clone();
    driver::accept(vm, host, receipt).unwrap();
    snapshot
}
fn ready<'ir, 's>(host: &Host<'_, 's>, vm: &mut Vm<'ir, 's>) -> Work<'s> {
    let _ = find(host, vm);
    observe(host, vm);
    let work = driver::next(vm, host).unwrap();
    assert_eq!(work.operation(), "gui.set_text");
    work
}
fn failed(vm: &Vm<'_, '_>) {
    assert_eq!(
        vm.status(),
        EffectSessionStatus::Terminal(EffectSessionEnd::Failed)
    );
}
#[test]
fn calculator_operations_and_fields_are_not_admitted_by_document_profile() {
    let fake = Fake::new();
    for source in [
        "fn main() = bind(target: gui.find(name: \"document\"), body: bind(state: gui.observe(target: target), body: field(value: state, name: \"last_expression\")))",
        "fn main() = bind(target: gui.find(name: \"document\"), body: gui.press(target: target, expected_revision: 0, button: \"one\"))",
    ] {
        assert!(driver::run(&fake, source).is_err());
    }
    assert_eq!(fake.writes.get(), 0);
}
#[test]
fn complete_source_workflow_uses_four_effects_and_observed_value() {
    let fake = Fake::new();
    let summary = driver::run(&fake, PROGRAM).unwrap();
    assert_eq!(summary.effects, 4);
    assert_eq!(summary.observations.len(), 2);
    assert_eq!(summary.value, ScalarValue::String(RESULT_TEXT.into()));
    assert_eq!(fake.writes.get(), 1);
    assert_eq!(summary.observations[0].value, INITIAL_TEXT);
    assert_eq!(summary.observations[1].value, RESULT_TEXT);
    assert!(summary.fuel_remaining < 1000);
}
#[test]
fn submission_pump_accept_and_historical_observation_are_separate() {
    let fake = Fake::new();
    session(&fake, PROGRAM, |host, vm| {
        let _ = find(host, vm);
        let before = observe(host, vm);
        let work = driver::next(vm, host).unwrap();
        let receipt = driver::dispatch(vm, host, work).unwrap();
        assert_eq!(fake.value(), INITIAL_TEXT);
        assert!(host.has_pending());
        assert!(matches!(
            vm.poll(host).unwrap(),
            EffectSessionPoll::Awaiting
        ));
        let receipt = driver::pump(vm, host, receipt).unwrap();
        assert_eq!(fake.value(), RESULT_TEXT);
        assert_eq!(fake.writes.get(), 1);
        assert!(!host.has_pending());
        assert_eq!(vm.status(), EffectSessionStatus::Awaiting);
        assert_eq!(before.data().value, INITIAL_TEXT);
        assert_eq!(
            host.field(&NativeValue::Snapshot(before.clone()), &Field::Value)
                .unwrap(),
            ScalarValue::String(INITIAL_TEXT.into())
        );
        driver::accept(vm, host, receipt).unwrap();
        let after = observe(host, vm);
        assert!(after.revision() > before.revision());
        assert_eq!(after.data().value, RESULT_TEXT);
    });
}
#[test]
fn demo_refuses_to_edit_unexpected_initial_content() {
    let fake = Fake::new();
    fake.state.borrow_mut().value = "unrelated document contents".into();
    let summary = driver::run(&fake, PROGRAM).unwrap();
    assert_eq!(summary.effects, 2);
    assert_eq!(
        summary.value,
        ScalarValue::String("initial content mismatch".into())
    );
    assert_eq!(fake.writes.get(), 0);
}
#[test]
fn observed_state_identity_and_authority_fences_precede_writes() {
    for queued in [false, true] {
        for cause in 0..7 {
            let fake = Fake::new();
            session(&fake, PROGRAM, |host, vm| {
                let mut work = Some(ready(host, vm));
                let receipt =
                    queued.then(|| driver::dispatch(vm, host, work.take().unwrap()).unwrap());
                match cause {
                    0 => fake.state.borrow_mut().value = "external edit".into(),
                    1 => fake.state.borrow_mut().identity += 1,
                    2 => fake.state.borrow_mut().document = "file:///tmp/other.txt".into(),
                    3 => fake.permitted.set(false),
                    4 => host.set_granted(false),
                    5 => fake.state.borrow_mut().editable = false,
                    6 => fake.available.set(false),
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
                assert_eq!(fake.writes.get(), 0);
                failed(vm);
            });
        }
    }
}
#[test]
fn changed_then_observed_then_restored_state_invalidates_revision() {
    let fake = Fake::new();
    session(&fake, PROGRAM, |host, vm| {
        let work = ready(host, vm);
        fake.state.borrow_mut().value = "external".into();
        host.refresh().unwrap();
        fake.state.borrow_mut().value = INITIAL_TEXT.into();
        let result = vm
            .with_metered_host_outcome(work, |work, fuel| host.dispatch(work, fuel))
            .unwrap();
        assert!(result.is_err());
        assert_eq!(fake.writes.get(), 0);
        failed(vm);
    });
}
#[test]
fn bridge_recheck_rejects_a_late_external_change_without_overwrite() {
    let fake = Fake::new();
    session(&fake, PROGRAM, |host, vm| {
        let work = ready(host, vm);
        let receipt = driver::dispatch(vm, host, work).unwrap();
        fake.outcome.set(3);
        assert!(matches!(driver::pump(vm, host, receipt), Err(Error::Stale)));
        failed(vm);
        assert_eq!(fake.value(), "external race");
        assert_eq!(fake.writes.get(), 0);
    });
}
#[test]
fn cancellation_and_late_replies_preserve_completed_effects_and_fuel() {
    for delivered in [false, true] {
        let fake = Fake::new();
        session(&fake, PROGRAM, |host, vm| {
            let work = ready(host, vm);
            let receipt = driver::dispatch(vm, host, work).unwrap();
            let receipt = if delivered {
                driver::pump(vm, host, receipt).unwrap()
            } else {
                receipt
            };
            assert!(vm.cancel());
            let fuel = vm.fuel_remaining();
            let rejected = vm
                .try_accept(&receipt.identity, receipt.reply, host)
                .err()
                .unwrap();
            assert!(matches!(rejected.reply, Reply::Submitted(true)));
            assert_eq!(vm.fuel_remaining(), fuel);
            assert_eq!(
                fake.value(),
                if delivered { RESULT_TEXT } else { INITIAL_TEXT }
            );
            if !delivered {
                // The closed execution also denies direct delivery of its queue.
                let receipt = Receipt {
                    identity: receipt.identity,
                    reply: rejected.reply,
                };
                assert!(matches!(
                    host.pump(receipt, &mut Fuel::new(100)),
                    HostWorkOutcome::Failed(Err(_))
                ));
                assert!(host.discard_pending());
            }
            assert_eq!(fake.writes.get(), usize::from(delivered));
        });
    }
}
#[test]
fn original_fuel_is_required_at_poll_and_at_queued_execution() {
    let fake = Fake::new();
    with_host(&fake, |host, scope| {
        let node = host.compile(PROGRAM).unwrap();
        let mut vm = LocalVm::new(scope, &node, Vec::new(), Fuel::new(0), STACK);
        assert!(vm.poll(host).is_err());
        assert_eq!(vm.fuel_remaining(), 0);
    })
    .unwrap();
    session(&fake, PROGRAM, |host, vm| {
        let work = ready(host, vm);
        let receipt = driver::dispatch(vm, host, work).unwrap();
        vm.with_metered_host_outcome((), |_, fuel| {
            fuel.charge(fuel.remaining()).unwrap();
            HostWorkOutcome::Pending(())
        })
        .unwrap();
        assert!(matches!(driver::pump(vm, host, receipt), Err(Error::Fuel)));
        assert_eq!(vm.fuel_remaining(), 0);
        failed(vm);
    });
    assert_eq!(fake.writes.get(), 0);
}
#[test]
fn uncertain_write_preserves_mutation_and_closes_vm_without_retry() {
    let fake = Fake::new();
    session(&fake, PROGRAM, |host, vm| {
        let work = ready(host, vm);
        let receipt = driver::dispatch(vm, host, work).unwrap();
        fake.outcome.set(1);
        assert!(matches!(
            driver::pump(vm, host, receipt),
            Err(Error::Uncertain(_))
        ));
        assert_eq!(
            vm.status(),
            EffectSessionStatus::Terminal(EffectSessionEnd::HostUncertain)
        );
        assert_eq!(fake.value(), RESULT_TEXT);
        assert_eq!(fake.writes.get(), 1);
        assert!(!host.has_pending());
        assert!(matches!(
            vm.poll(host).unwrap(),
            EffectSessionPoll::Terminal(EffectSessionEnd::HostUncertain)
        ));
        assert_eq!(fake.writes.get(), 1);
    });
}
#[test]
fn a_completed_submission_cannot_be_pumped_twice() {
    let fake = Fake::new();
    session(&fake, PROGRAM, |host, vm| {
        let work = ready(host, vm);
        let receipt = driver::dispatch(vm, host, work).unwrap();
        let receipt = driver::pump(vm, host, receipt).unwrap();
        let replay = Receipt {
            identity: receipt.identity.clone(),
            reply: Reply::Submitted(true),
        };
        assert!(driver::pump(vm, host, replay).is_err());
        failed(vm);
        assert_eq!(fake.writes.get(), 1);
        assert_eq!(fake.value(), RESULT_TEXT);
        assert!(
            vm.try_accept(&receipt.identity, receipt.reply, host)
                .is_err()
        );
    });
}
#[test]
fn permission_revocation_after_write_rejects_reply_without_undoing_text() {
    let fake = Fake::new();
    session(&fake, PROGRAM, |host, vm| {
        let work = ready(host, vm);
        let receipt = driver::dispatch(vm, host, work).unwrap();
        let receipt = driver::pump(vm, host, receipt).unwrap();
        let fuel = vm.fuel_remaining();
        fake.permitted.set(false);
        let rejected = vm
            .try_accept(&receipt.identity, receipt.reply, host)
            .err()
            .unwrap();
        assert!(matches!(rejected.reply, Reply::Submitted(true)));
        assert_eq!(vm.fuel_remaining(), fuel);
        assert_eq!(vm.status(), EffectSessionStatus::Awaiting);
        assert_eq!(fake.writes.get(), 1);
        assert_eq!(fake.value(), RESULT_TEXT);
        assert!(vm.cancel());
    });
}
#[test]
fn acknowledgement_without_the_observed_postcondition_is_not_success() {
    let fake = Fake::new();
    fake.outcome.set(2);
    let summary = driver::run(&fake, PROGRAM).unwrap();
    assert_eq!(
        summary.value,
        ScalarValue::String("verification failed".into())
    );
    assert_eq!(fake.value(), INITIAL_TEXT);
    assert_eq!(fake.writes.get(), 1);
}
#[test]
fn foreign_object_and_spoofed_snapshot_replies_preserve_pending_request() {
    let fake = Fake::new();
    session(&fake, PROGRAM, |host, vm| {
        let work = driver::next(vm, host).unwrap();
        let receipt = driver::dispatch(vm, host, work).unwrap();
        let foreign_owner = LocalObjectOwner::new();
        let foreign = foreign_owner.reference(&host.lease()).unwrap();
        let fuel = vm.fuel_remaining();
        let rejected = vm
            .try_accept(&receipt.identity, Reply::Object(foreign), host)
            .err()
            .unwrap();
        assert!(matches!(rejected.reply, Reply::Object(_)));
        assert_eq!(vm.status(), EffectSessionStatus::Awaiting);
        assert_eq!(vm.fuel_remaining(), fuel);
        driver::accept(vm, host, receipt).unwrap();
        let work = driver::next(vm, host).unwrap();
        let receipt = driver::dispatch(vm, host, work).unwrap();
        let Reply::Snapshot(original) = &receipt.reply else {
            panic!("snapshot")
        };
        let mut corrupt = original.data().clone();
        corrupt.identity += 1;
        let corrupt = host
            .owner()
            .observe(original.object(), &host.lease(), corrupt)
            .unwrap();
        let fuel = vm.fuel_remaining();
        let rejected = vm
            .try_accept(&receipt.identity, Reply::Snapshot(Rc::new(corrupt)), host)
            .err()
            .unwrap();
        assert!(matches!(rejected.reply, Reply::Snapshot(_)));
        assert_eq!(vm.status(), EffectSessionStatus::Awaiting);
        assert_eq!(vm.fuel_remaining(), fuel);
        driver::accept(vm, host, receipt).unwrap();
    });
}
#[test]
fn scalar_runner_and_native_final_result_preserve_scope_boundary() {
    let fake = Fake::new();
    assert_eq!(
        driver::run(&fake, "fn main() = add(left: 20, right: 22)")
            .unwrap()
            .value,
        ScalarValue::Integer(42)
    );
    assert!(
        driver::run(&fake, "fn main() = gui.find(name: \"document\")")
            .unwrap_err()
            .contains("session-bound native value")
    );
    let source = r#"fn main() = bind(target: gui.find(name: "document"), body: bind(before: gui.observe(target: target), body: bind(submitted: gui.set_text(target: target, expected_revision: field(value: before, name: "revision"), text: "retained effect"), body: gui.observe(target: target))))"#;
    assert!(
        driver::run(&fake, source)
            .unwrap_err()
            .contains("session-bound native value")
    );
    assert_eq!(fake.value(), "retained effect");
    assert_eq!(fake.writes.get(), 1);
}
#[test]
fn malformed_roles_bounds_and_selectors_refuse_before_write() {
    for cause in 0..4 {
        let fake = Fake::new();
        match cause {
            0 => fake.state.borrow_mut().role = "AXButton".into(),
            1 => fake.state.borrow_mut().value = "x".repeat(MAX_SCALAR_STRING_BYTES + 1),
            2 => fake.state.borrow_mut().identity = 0,
            3 => fake.permitted.set(false),
            _ => unreachable!(),
        }
        assert!(driver::run(&fake, PROGRAM).is_err());
        assert_eq!(fake.writes.get(), 0);
    }
    let fake = Fake::new();
    assert!(
        driver::run(
            &fake,
            &PROGRAM.replace("name: \"document\"", "name: \"other\"")
        )
        .is_err()
    );
    let oversized = PROGRAM.replace(
        "LeseLang cross-process ✓\\n",
        &"x".repeat(MAX_SCALAR_STRING_BYTES + 1),
    );
    assert!(driver::run(&fake, &oversized).is_err());
    assert_eq!(fake.writes.get(), 0);
}
