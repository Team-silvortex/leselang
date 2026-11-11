use host_protocol::{ObjectError, ObjectRef, Observation};
use leselang_vm::native::{
    HostWorkOutcome, LocalVm,
    core::*,
    hir::{
        effect_reentry::RestoredEffectBindings,
        effect_session::{EffectSessionEnd, EffectSessionPoll, EffectSessionStatus},
        pure_evaluation::PureValue,
    },
};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
};

#[path = "support/protocol_host.rs"]
mod support;
use support::*;

type Vm<'ir, 's, A> = LocalVm<
    'ir,
    's,
    Node,
    NativeValue<<A as Adapter>::Snapshot>,
    RestoredEffectBindings<'ir, NativeValue<<A as Adapter>::Snapshot>>,
    Identity,
    Domain<'s>,
    Reply<<A as Adapter>::Snapshot>,
>;

fn next<'ir, 's, A: Adapter>(
    vm: &mut Vm<'ir, 's, A>,
    host: &ProtocolHost<'_, 's, A>,
) -> Work<'s, A::Snapshot> {
    let EffectSessionPoll::Request(work) = vm.poll(host).unwrap() else {
        panic!("expected effect request")
    };
    work
}
fn dispatch<'ir, 's, A: Adapter>(
    vm: &mut Vm<'ir, 's, A>,
    host: &ProtocolHost<'_, 's, A>,
    work: Work<'s, A::Snapshot>,
) -> Result<Receipt<A::Snapshot>, Error> {
    vm.with_metered_host_outcome(work, |work, fuel| host.dispatch(work, fuel))
        .unwrap()
}
fn accept<A: Adapter>(
    vm: &mut Vm<'_, '_, A>,
    host: &ProtocolHost<'_, '_, A>,
    receipt: Receipt<A::Snapshot>,
) {
    vm.try_accept(&receipt.identity, receipt.reply, host)
        .unwrap();
}
fn find<'ir, 's, A: Adapter>(vm: &mut Vm<'ir, 's, A>, host: &ProtocolHost<'_, 's, A>) -> ObjectRef {
    let work = next(vm, host);
    assert_eq!(work.operation(), "object.find");
    host.authorize(work.identity()).unwrap();
    let receipt = dispatch(vm, host, work).unwrap();
    let Reply::Object(object) = &receipt.reply else {
        panic!("expected native object");
    };
    let object = object.clone();
    accept(vm, host, receipt);
    object
}
fn observe<'ir, 's, A: Adapter>(
    vm: &mut Vm<'ir, 's, A>,
    host: &ProtocolHost<'_, 's, A>,
) -> Rc<Observation<A::Snapshot>> {
    let work = next(vm, host);
    assert_eq!(work.operation(), "object.observe");
    host.authorize(work.identity()).unwrap();
    let receipt = dispatch(vm, host, work).unwrap();
    let Reply::Snapshot(snapshot) = &receipt.reply else {
        panic!("observation is a native result")
    };
    let snapshot = snapshot.clone();
    accept(vm, host, receipt);
    snapshot
}
fn pump<A: Adapter>(
    vm: &mut Vm<'_, '_, A>,
    host: &ProtocolHost<'_, '_, A>,
    receipt: Receipt<A::Snapshot>,
) -> Result<Receipt<A::Snapshot>, Error> {
    vm.with_metered_host_outcome(receipt, |receipt, fuel| host.pump(receipt, fuel))
        .unwrap()
}
fn consume_fuel<A: Adapter>(vm: &mut Vm<'_, '_, A>) {
    vm.with_metered_host_outcome((), |_, fuel| {
        fuel.charge(fuel.remaining()).unwrap();
        HostWorkOutcome::Pending(())
    })
    .unwrap();
}
macro_rules! both {
    ($test:ident, $case:ident) => {
        #[test]
        fn $test() {
            $case::<GuiAdapter>();
            $case::<CliAdapter>();
        }
    };
}

#[test]
fn both_reference_adapters_run_the_same_observe_apply_verify_program() {
    for summary in [run_reference_gui().unwrap(), run_reference_cli().unwrap()] {
        assert_eq!(summary.text, "ready");
        assert_eq!(
            (summary.observations, summary.submissions, summary.pumps),
            (2, 1, 1)
        );
        assert!(summary.fuel_remaining < 1000);
    }
}

fn discovery<A: Adapter>() {
    let adapter = A::default();
    with_host(&adapter, |host, _scope| {
        let schemas = host.catalog().schemas();
        assert_eq!(schemas.len(), 3);
        assert_eq!(schemas[0].key, "object.find");
        assert_eq!(schemas[0].result.result_name(), "object");
        assert_eq!(schemas[1].key, "object.observe");
        assert_eq!(schemas[1].required_capability, 1);
        assert_eq!(schemas[1].result.result_name(), "observation");
        assert_eq!(
            schemas[1].parameters[0],
            NamedParameter::required("target", ArgumentDomain::Object)
        );
        assert_eq!(schemas[2].key, "object.apply");
        assert_eq!(schemas[2].required_capability, 2);
        assert_eq!(schemas[2].result.result_name(), "submission");
        assert_eq!(
            schemas[2].parameters[1],
            NamedParameter::required(
                "expected_revision",
                ArgumentDomain::Scalar(ScalarTypeSet::only(ScalarType::Integer))
            )
        );
        assert_eq!(
            schemas[2].parameters[2],
            NamedParameter::required(
                "text",
                ArgumentDomain::Scalar(ScalarTypeSet::only(ScalarType::String))
            )
        );
        assert_eq!(adapter.counts(), (0, 0, 0));
        assert_eq!(adapter.owner().revision(), 0);
        assert_eq!(adapter.text(), "draft");
        for source in [
            "fn main() = object.apply(target: 1, expected_revision: 0, text: \"ready\")",
            "fn main() = bind(before: object.observe(), body: field(value: before, name: \"object\"))",
            "fn main() = bind(before: object.observe(), body: add(left: before, right: 1))",
        ] {
            assert!(host.compile(source).is_err());
        }
        host.set_granted(false);
        assert_eq!(host.catalog().schemas().len(), 3);
        assert!(host.compile(PROGRAM).is_err());
        assert_eq!(adapter.counts(), (0, 0, 0));
    });
}
both!(
    discovery_is_shared_metadata_without_native_work_or_scalar_object_tokens,
    discovery
);

fn phases<A: Adapter>() {
    let adapter = A::default();
    with_host(&adapter, |host, scope| {
        let node = host.compile(PROGRAM).unwrap();
        let mut vm = LocalVm::new(scope, &node, Vec::new(), Fuel::new(1000), STACK);
        let _ = find(&mut vm, host);
        let before = observe(&mut vm, host);
        assert_eq!(before.data().text().unwrap(), "draft");
        let work = next(&mut vm, host);
        assert_eq!(work.operation(), "object.apply");
        host.authorize(work.identity()).unwrap();
        let receipt = dispatch(&mut vm, host, work).unwrap();
        assert!(matches!(receipt.reply, Reply::Submitted(true)));
        assert_eq!(adapter.text(), "draft");
        assert_eq!(adapter.counts(), (1, 1, 0));
        assert!(matches!(
            vm.poll(host).unwrap(),
            EffectSessionPoll::Awaiting
        ));
        let fuel = vm.fuel_remaining();
        let receipt = pump(&mut vm, host, receipt).unwrap();
        assert_eq!(vm.fuel_remaining(), fuel - 6);
        assert_eq!(adapter.text(), "ready");
        assert_eq!(before.data().text().unwrap(), "draft");
        assert!(before.revision() < adapter.owner().revision());
        adapter
            .owner()
            .validate(before.object(), &host.lease())
            .unwrap();
        assert_eq!(vm.status(), EffectSessionStatus::Awaiting);
        accept(&mut vm, host, receipt);
        let after = observe(&mut vm, host);
        assert_eq!(after.data().text().unwrap(), "ready");
        assert!(after.revision() > before.revision());
        assert!(
            matches!(vm.poll(host).unwrap(), EffectSessionPoll::Value(PureValue::Scalar(ScalarValue::String(value))) if value == "ready")
        );
        assert!(!host.lease().is_open());
    });
}
both!(
    submission_is_not_an_observation_and_only_the_explicit_pump_commits,
    phases
);

fn historical<A: Adapter>() {
    let adapter = A::default();
    let source = PROGRAM.replace("field(value: after, name: \"text\")", "concat(left: field(value: before, name: \"text\"), right: field(value: after, name: \"text\"))");
    with_host(&adapter, |host, scope| {
        let node = host.compile(&source).unwrap();
        let mut vm = LocalVm::new(scope, &node, Vec::new(), Fuel::new(1000), STACK);
        let _ = find(&mut vm, host);
        observe(&mut vm, host);
        let work = next(&mut vm, host);
        let receipt = dispatch(&mut vm, host, work).unwrap();
        let receipt = pump(&mut vm, host, receipt).unwrap();
        accept(&mut vm, host, receipt);
        observe(&mut vm, host);
        assert!(
            matches!(vm.poll(host).unwrap(), EffectSessionPoll::Value(PureValue::Scalar(ScalarValue::String(value))) if value == "draftready")
        );
    });
}
both!(
    language_projection_preserves_historical_snapshot_after_the_action,
    historical
);

fn dispatch_fences<A: Adapter>() {
    for cause in 0..3 {
        let adapter = A::default();
        with_host(&adapter, |host, scope| {
            let node = host.compile(PROGRAM).unwrap();
            let mut vm = LocalVm::new(scope, &node, Vec::new(), Fuel::new(1000), STACK);
            let _ = find(&mut vm, host);
            observe(&mut vm, host);
            let work = next(&mut vm, host);
            host.authorize(work.identity()).unwrap();
            match cause {
                0 => adapter.external_change("external"),
                1 => {
                    adapter.owner().replace().unwrap();
                }
                _ => host.set_granted(false),
            }
            let error = dispatch(&mut vm, host, work).err().unwrap();
            if cause == 0 {
                assert!(matches!(error, Error::Object(ObjectError::StaleRevision)));
            }
            if cause == 1 {
                assert!(matches!(error, Error::Object(ObjectError::StaleGeneration)));
            }
            assert_eq!(adapter.counts(), (1, 0, 0));
            assert_eq!(
                adapter.text(),
                if cause == 0 { "external" } else { "draft" }
            );
            assert_eq!(
                vm.status(),
                EffectSessionStatus::Terminal(EffectSessionEnd::Failed)
            );
            assert!(!host.lease().is_open());
        });
    }
}
both!(
    stale_revision_replaced_target_and_revoked_permission_refuse_before_action,
    dispatch_fences
);

fn pump_fences<A: Adapter>() {
    for cause in 0..4 {
        let adapter = A::default();
        with_host(&adapter, |host, scope| {
            let node = host.compile(PROGRAM).unwrap();
            let mut vm = LocalVm::new(scope, &node, Vec::new(), Fuel::new(1000), STACK);
            let _ = find(&mut vm, host);
            observe(&mut vm, host);
            let work = next(&mut vm, host);
            let receipt = dispatch(&mut vm, host, work).unwrap();
            match cause {
                0 => adapter.external_change("external"),
                1 => {
                    adapter.owner().replace().unwrap();
                }
                2 => host.set_granted(false),
                _ => consume_fuel::<A>(&mut vm),
            }
            assert!(pump(&mut vm, host, receipt).is_err());
            assert_eq!(adapter.counts(), (1, 1, 0));
            assert_eq!(
                adapter.text(),
                if cause == 0 { "external" } else { "draft" }
            );
            assert_eq!(
                vm.status(),
                EffectSessionStatus::Terminal(EffectSessionEnd::Failed)
            );
            assert!(!host.lease().is_open());
            if cause == 3 {
                assert_eq!(vm.fuel_remaining(), 0);
            }
        });
    }
}
both!(
    queued_work_rechecks_revision_generation_grants_and_original_fuel,
    pump_fences
);

fn failures<A: Adapter>() {
    for pump_phase in [false, true] {
        for failure in [
            FailureMode::Before,
            FailureMode::AfterCommit,
            FailureMode::Uncertain,
            FailureMode::PanicAfterCommit,
        ] {
            let adapter = A::default();
            with_host(&adapter, |host, scope| {
                let node = host.compile(PROGRAM).unwrap();
                let mut vm = LocalVm::new(scope, &node, Vec::new(), Fuel::new(1000), STACK);
                let _ = find(&mut vm, host);
                observe(&mut vm, host);
                let work = next(&mut vm, host);
                let result = if pump_phase {
                    let receipt = dispatch(&mut vm, host, work).unwrap();
                    adapter.set_pump_failure(failure);
                    catch_unwind(AssertUnwindSafe(|| pump(&mut vm, host, receipt)))
                } else {
                    adapter.set_failure(failure);
                    catch_unwind(AssertUnwindSafe(|| dispatch(&mut vm, host, work)))
                };
                if failure == FailureMode::PanicAfterCommit {
                    assert!(result.is_err());
                } else {
                    assert!(result.unwrap().is_err());
                }
                let expected_end = if matches!(
                    failure,
                    FailureMode::Uncertain | FailureMode::PanicAfterCommit
                ) {
                    EffectSessionEnd::HostUncertain
                } else {
                    EffectSessionEnd::Failed
                };
                assert_eq!(vm.status(), EffectSessionStatus::Terminal(expected_end));
                assert_eq!(
                    adapter.text(),
                    if failure == FailureMode::Before {
                        "draft"
                    } else {
                        "ready"
                    }
                );
                assert!(!host.lease().is_open());
                assert!(adapter.owner().revision() > 0);
                assert_eq!(adapter.counts().1, u64::from(pump_phase));
                assert!(!vm.cancel());
                let fuel = vm.fuel_remaining();
                assert!(
                    matches!(vm.poll(host).unwrap(), EffectSessionPoll::Terminal(end) if end == expected_end)
                );
                assert_eq!(vm.fuel_remaining(), fuel);
            });
        }
    }
}
both!(
    action_and_pump_failure_uncertainty_and_panic_preserve_partial_effects,
    failures
);

fn cancellation<A: Adapter>() {
    let adapter = A::default();
    with_host(&adapter, |host, scope| {
        let node = host.compile(PROGRAM).unwrap();
        let mut vm = LocalVm::new(scope, &node, Vec::new(), Fuel::new(1000), STACK);
        let _ = find(&mut vm, host);
        observe(&mut vm, host);
        let work = next(&mut vm, host);
        let receipt = dispatch(&mut vm, host, work).unwrap();
        assert!(vm.cancel());
        let original = vm
            .with_metered_host_outcome(receipt, |_, _| -> HostWorkOutcome<()> {
                panic!("cancelled pump was entered")
            })
            .err()
            .unwrap();
        assert!(matches!(original.reply, Reply::Submitted(true)));
        assert_eq!(adapter.text(), "draft");
        // The host explicitly disposes its external queue after VM cancellation.
        assert!(adapter.take_update(&host.lease()).unwrap().is_some());
        assert_eq!(adapter.counts(), (1, 1, 0));
    });
    // Disposing the cancelled owner's queue makes the same native resource
    // available to a fresh execution, without rearming the cancelled request.
    assert_eq!(run_reference(&adapter).unwrap().text, "ready");
    assert_eq!(adapter.counts(), (3, 2, 1));
    let adapter = A::default();
    with_host(&adapter, |host, scope| {
        let node = host.compile(PROGRAM).unwrap();
        let mut vm = LocalVm::new(scope, &node, Vec::new(), Fuel::new(1000), STACK);
        let _ = find(&mut vm, host);
        observe(&mut vm, host);
        let work = next(&mut vm, host);
        let receipt = dispatch(&mut vm, host, work).unwrap();
        let receipt = pump(&mut vm, host, receipt).unwrap();
        accept(&mut vm, host, receipt);
        let work = next(&mut vm, host);
        let receipt = dispatch(&mut vm, host, work).unwrap();
        let Reply::Snapshot(snapshot) = &receipt.reply else {
            panic!()
        };
        let pointer = Rc::as_ptr(snapshot);
        vm.cancel();
        let fuel = vm.fuel_remaining();
        let rejected = vm
            .try_accept(&receipt.identity, receipt.reply, host)
            .err()
            .unwrap();
        let Reply::Snapshot(snapshot) = rejected.reply else {
            panic!()
        };
        assert_eq!(Rc::as_ptr(&snapshot), pointer);
        assert_eq!(snapshot.data().text().unwrap(), "ready");
        assert_eq!(adapter.text(), "ready");
        assert_eq!(vm.fuel_remaining(), fuel);
        assert_eq!(
            vm.status(),
            EffectSessionStatus::Terminal(EffectSessionEnd::Cancelled)
        );
    });
}
both!(
    cancellation_stops_queued_mutation_and_late_observation_keeps_committed_evidence,
    cancellation
);

fn no_replay<A: Adapter>() {
    let adapter = A::default();
    with_host(&adapter, |host, scope| {
        let node = host.compile(PROGRAM).unwrap();
        let mut vm = LocalVm::new(scope, &node, Vec::new(), Fuel::new(1000), STACK);
        let _ = find(&mut vm, host);
        observe(&mut vm, host);
        let work = next(&mut vm, host);
        let receipt = dispatch(&mut vm, host, work).unwrap();
        // The low-level native host can retain identity metadata, but that data
        // cannot create a second queued action or authorize replay.
        let replay = Receipt {
            identity: receipt.identity.clone(),
            reply: Reply::Submitted(true),
        };
        let _original = pump(&mut vm, host, receipt).unwrap();
        assert!(pump(&mut vm, host, replay).is_err());
        assert_eq!(adapter.text(), "ready");
        assert_eq!(adapter.counts(), (1, 1, 1));
        assert_eq!(
            vm.status(),
            EffectSessionStatus::Terminal(EffectSessionEnd::Failed)
        );
    });
}
both!(
    a_submission_receipt_cannot_replay_an_already_consumed_native_update,
    no_replay
);

fn cross_session<A: Adapter>() {
    let adapter = A::default();
    with_host(&adapter, |first, first_scope| {
        with_host(&adapter, |second, second_scope| {
            assert!(matches!(
                adapter.owner().validate(&first.target(), &second.lease()),
                Err(ObjectError::WrongExecution)
            ));
            let node = first.compile(PROGRAM).unwrap();
            let other = second.compile(PROGRAM).unwrap();
            let mut one = LocalVm::new(first_scope, &node, Vec::new(), Fuel::new(1000), STACK);
            let mut two = LocalVm::new(second_scope, &other, Vec::new(), Fuel::new(1000), STACK);
            let _ = find(&mut one, first);
            let _ = find(&mut two, second);
            let work = next(&mut one, first);
            assert!(second.authorize(work.identity()).is_err());
            let receipt = dispatch(&mut one, first, work).unwrap();
            let pending_two = next(&mut two, second);
            let rejected = two
                .try_accept(&receipt.identity, receipt.reply, second)
                .err()
                .unwrap();
            assert_eq!(two.status(), EffectSessionStatus::Awaiting);
            one.try_accept(&receipt.identity, rejected.reply, first)
                .unwrap();
            let work = next(&mut one, first);
            let submitted = dispatch(&mut one, first, work).unwrap();
            assert!(matches!(
                adapter.take_update(&second.lease()),
                Err(Error::Object(ObjectError::WrongExecution))
            ));
            // The failed foreign access did not remove the original queue item.
            let submitted = pump(&mut one, first, submitted).unwrap();
            accept(&mut one, first, submitted);
            let receipt = dispatch(&mut two, second, pending_two).unwrap();
            let Reply::Snapshot(snapshot) = &receipt.reply else {
                panic!()
            };
            assert_eq!(snapshot.data().text().unwrap(), "ready");
            accept(&mut two, second, receipt);
            one.cancel();
            two.cancel();
        });
    });
}
both!(
    cross_session_references_receipts_and_queue_access_do_not_transfer_authority,
    cross_session
);

fn fuel<A: Adapter>() {
    let adapter = A::default();
    with_host(&adapter, |host, scope| {
        let node = host.compile(PROGRAM).unwrap();
        let mut vm = LocalVm::new(scope, &node, Vec::new(), Fuel::new(0), STACK);
        assert!(vm.poll(host).is_err());
        assert_eq!(adapter.counts(), (0, 0, 0));
        assert_eq!(vm.fuel_remaining(), 0);
    });
    with_host(&adapter, |host, scope| {
        let node = host.compile(PROGRAM).unwrap();
        let mut vm = LocalVm::new(scope, &node, Vec::new(), Fuel::new(1000), STACK);
        let _ = find(&mut vm, host);
        observe(&mut vm, host);
        let work = next(&mut vm, host);
        let receipt = dispatch(&mut vm, host, work).unwrap();
        let receipt = pump(&mut vm, host, receipt).unwrap();
        consume_fuel::<A>(&mut vm);
        accept(&mut vm, host, receipt);
        assert!(vm.poll(host).is_err());
        assert_eq!(vm.fuel_remaining(), 0);
        assert_eq!(adapter.text(), "ready");
        assert_eq!(adapter.counts(), (1, 1, 1));
        assert_eq!(
            vm.status(),
            EffectSessionStatus::Terminal(EffectSessionEnd::Failed)
        );
    });
}
both!(
    fuel_is_shared_across_native_pump_and_language_reentry_without_rollback,
    fuel
);

fn reply_domains<A: Adapter>() {
    let adapter = A::default();
    with_host(&adapter, |host, scope| {
        let node = host.compile(r#"fn main() = bind(target: object.find(name: "save"), body: object.observe(target: target))"#).unwrap();
        let mut vm = LocalVm::new(scope, &node, Vec::new(), Fuel::new(1000), STACK);
        let _ = find(&mut vm, host);
        let work = next(&mut vm, host);
        let receipt = dispatch(&mut vm, host, work).unwrap();
        let fuel = vm.fuel_remaining();
        let rejected = vm
            .try_accept(&receipt.identity, Reply::Submitted(true), host)
            .err()
            .unwrap();
        assert!(matches!(rejected.reply, Reply::Submitted(true)));
        assert_eq!(vm.status(), EffectSessionStatus::Awaiting);
        adapter.external_change(&"x".repeat(MAX_SCALAR_STRING_BYTES + 1));
        let unbounded = Rc::new(
            adapter
                .owner()
                .observe(&host.target(), &host.lease(), adapter.snapshot())
                .unwrap(),
        );
        let pointer = Rc::as_ptr(&unbounded);
        let rejected = vm
            .try_accept(&receipt.identity, Reply::Snapshot(unbounded), host)
            .err()
            .unwrap();
        let Reply::Snapshot(returned) = rejected.reply else {
            panic!()
        };
        assert_eq!(Rc::as_ptr(&returned), pointer);
        assert_eq!(vm.fuel_remaining(), fuel);
        assert_eq!(vm.status(), EffectSessionStatus::Awaiting);
        // The original bounded historical snapshot remains admissible.
        accept(&mut vm, host, receipt);
        assert!(matches!(
            vm.poll(host).unwrap(),
            EffectSessionPoll::Value(PureValue::Result(_))
        ));
    });
    with_host(&adapter, |host, scope| {
        let node = host.compile(r#"fn main() = bind(target: object.find(name: "save"), body: object.observe(target: target))"#).unwrap();
        let mut vm = LocalVm::new(scope, &node, Vec::new(), Fuel::new(1000), STACK);
        let _ = find(&mut vm, host);
        let work = next(&mut vm, host);
        let observations = adapter.counts().0;
        let fuel = vm.fuel_remaining();
        assert!(dispatch(&mut vm, host, work).is_err());
        assert_eq!(adapter.counts().0, observations);
        assert_eq!(vm.fuel_remaining(), fuel);
        assert_eq!(
            vm.status(),
            EffectSessionStatus::Terminal(EffectSessionEnd::Failed)
        );
    });
}
both!(
    native_snapshot_type_and_value_bounds_are_checked_without_cloning_rejected_data,
    reply_domains
);

fn drive_source<A: Adapter>(
    host: &ProtocolHost<'_, '_, A>,
    scope: LocalExecutionScope,
    source: &str,
) -> String {
    let node = host.compile(source).unwrap();
    let mut vm = LocalVm::new(scope, &node, Vec::new(), Fuel::new(2000), STACK);
    loop {
        match vm.poll(host).unwrap() {
            EffectSessionPoll::Request(work) => {
                host.authorize(work.identity()).unwrap();
                let action = work.operation() == "object.apply";
                let receipt = dispatch(&mut vm, host, work).unwrap();
                let receipt = if action {
                    pump(&mut vm, host, receipt).unwrap()
                } else {
                    receipt
                };
                accept(&mut vm, host, receipt);
            }
            EffectSessionPoll::Value(PureValue::Scalar(ScalarValue::String(text))) => return text,
            _ => panic!("unexpected execution boundary"),
        }
    }
}

fn dynamic_targets<A: Adapter>() {
    for select_other in [false, true] {
        let primary = A::default();
        let other = A::default();
        primary.external_change("primary");
        other.external_change("secondary");
        // Both finds execute, and choose selects a native alias as an ordinary
        // pure call operand. Source order deliberately differs from schema order.
        let source = format!(
            r#"
fn locate(name: string) = object.find(name: name)
fn main() = bind(
    first: locate(name: "save"),
    body: bind(
        second: locate(name: "other"),
        body: bind(
            target: choose(when: {select_other}, then: second, otherwise: first),
            body: bind(
                before: object.observe(target: target),
                body: bind(
                    applied: object.apply(text: "selected", expected_revision: field(value: before, name: "revision"), target: bind(alias: target, body: alias)),
                    body: bind(after: object.observe(target: choose(when: true, then: target, otherwise: first)), body: field(value: after, name: "text")),
                ),
            ),
        ),
    ),
)
"#
        );
        with_targets(&primary, &other, |host, scope| {
            assert_eq!(drive_source(host, scope, &source), "selected");
        });
        assert_eq!(
            primary.text(),
            if select_other { "primary" } else { "selected" }
        );
        assert_eq!(
            other.text(),
            if select_other {
                "selected"
            } else {
                "secondary"
            }
        );
        assert_eq!(
            primary.counts(),
            if select_other { (0, 0, 0) } else { (2, 1, 1) }
        );
        assert_eq!(
            other.counts(),
            if select_other { (2, 1, 1) } else { (0, 0, 0) }
        );
    }
}
both!(
    source_selects_between_two_native_objects_through_helpers_and_pure_aliases,
    dynamic_targets
);

fn native_type_failures<A: Adapter>() {
    let adapter = A::default();
    with_host(&adapter, |host, _scope| {
        for source in [
            r#"fn main() = object.observe(target: "save")"#,
            r#"fn main() = object.observe(target: 1)"#,
            r#"fn main() = object.observe(target: object.find(name: "save"))"#,
            r#"fn main() = bind(target: object.find(name: "save"), body: object.find(name: target))"#,
            r#"fn main() = bind(target: object.find(name: "save"), body: field(value: target, name: "revision"))"#,
            r#"fn main() = bind(target: object.find(name: "save"), body: bind(snapshot: object.observe(target: target), body: object.observe(target: snapshot)))"#,
            r#"fn main() = bind(target: object.find(name: "save"), body: choose(when: true, then: object.observe(target: target), otherwise: object.observe(target: false)))"#,
            r#"fn main() = bind(target: object.find(name: "save"), body: object.observe(target: choose(when: true, then: target, otherwise: 0)))"#,
            r#"fn main() = object.find(name: "save")
fn unused() = object.observe(target: "save")"#,
            r#"fn pass(target: string) = object.observe(target: target)
fn main() = bind(target: object.find(name: "save"), body: pass(target: target))"#,
        ] {
            assert!(
                host.compile(source).is_err(),
                "accepted invalid native operand: {source}"
            );
        }
        assert_eq!(adapter.counts(), (0, 0, 0));
    });
}
both!(
    cold_source_rejects_forged_native_operands_wrong_types_and_hidden_effects,
    native_type_failures
);

fn native_value_failures<A: Adapter>() {
    use leselang_vm::native::hir::ir::ComputedArgument;
    for cause in 0..4 {
        let adapter = A::default();
        let foreign = A::default();
        let other_scope = LocalExecutionScope::new();
        with_host(&adapter, |host, scope| {
            let target = match cause {
                0 => foreign.owner().reference(&host.lease()).unwrap(),
                1 => adapter.owner().reference(&other_scope.lease()).unwrap(),
                _ => host.target(),
            };
            if cause == 2 {
                adapter.owner().replace().unwrap();
            }
            if cause == 3 {
                adapter.owner().retire();
            }
            let node = Node::Call {
                operation: "object.observe",
                arguments: vec![ComputedArgument {
                    name: "target".into(),
                    value: Node::Local {
                        name: "target".into(),
                    },
                }],
            };
            let mut vm = LocalVm::new(
                scope,
                &node,
                vec![("target", PureValue::Result(NativeValue::Object(target)))],
                Fuel::new(1000),
                STACK,
            );
            assert!(vm.poll(host).is_err());
            assert_eq!(adapter.counts(), (0, 0, 0));
            assert_eq!(
                vm.status(),
                EffectSessionStatus::Terminal(EffectSessionEnd::Failed)
            );
        });
    }
    let adapter = A::default();
    with_host(&adapter, |host, scope| {
        let node = host.compile(PROGRAM).unwrap();
        let mut vm = LocalVm::new(scope, &node, Vec::new(), Fuel::new(1000), STACK);
        let target = find(&mut vm, host);
        adapter.owner().replace().unwrap();
        assert!(matches!(
            adapter.owner().validate(&target, &host.lease()),
            Err(ObjectError::StaleGeneration)
        ));
        assert!(vm.poll(host).is_err());
        assert_eq!(adapter.counts(), (0, 0, 0));
    });
}
both!(
    native_preparation_rejects_foreign_closed_and_replaced_identity_without_rebinding,
    native_value_failures
);

fn discovery_failures<A: Adapter>() {
    let adapter = A::default();
    with_host(&adapter, |host, scope| {
        let node = host
            .compile(r#"fn main() = object.find(name: "missing")"#)
            .unwrap();
        let mut vm = LocalVm::new(scope, &node, Vec::new(), Fuel::new(1000), STACK);
        let work = next(&mut vm, host);
        assert!(dispatch(&mut vm, host, work).is_err());
        assert_eq!(
            vm.status(),
            EffectSessionStatus::Terminal(EffectSessionEnd::Failed)
        );
        assert_eq!(adapter.counts(), (0, 0, 0));
    });
    with_host(&adapter, |host, scope| {
        let node = host.compile(PROGRAM).unwrap();
        let mut vm = LocalVm::new(scope, &node, Vec::new(), Fuel::new(1000), STACK);
        let work = next(&mut vm, host);
        let receipt = dispatch(&mut vm, host, work).unwrap();
        let foreign = A::default();
        let forged = foreign.owner().reference(&host.lease()).unwrap();
        let rejected = vm
            .try_accept(&receipt.identity, Reply::Object(forged), host)
            .err()
            .unwrap();
        let Reply::Object(returned) = rejected.reply else {
            panic!();
        };
        foreign.owner().validate(&returned, &host.lease()).unwrap();
        assert_eq!(vm.status(), EffectSessionStatus::Awaiting);
        assert!(vm.cancel());
        let rejected = vm
            .try_accept(&receipt.identity, receipt.reply, host)
            .err()
            .unwrap();
        let Reply::Object(returned) = rejected.reply else {
            panic!();
        };
        assert!(matches!(
            adapter.owner().validate(&returned, &host.lease()),
            Err(ObjectError::ClosedExecution)
        ));
        assert_eq!(adapter.counts(), (0, 0, 0));
    });
}
both!(
    discovery_failure_and_late_native_object_replies_preserve_session_boundaries,
    discovery_failures
);

fn reply_target<A: Adapter>() {
    let adapter = A::default();
    let other = A::default();
    with_targets(&adapter, &other, |host, scope| {
        let node = host.compile(PROGRAM).unwrap();
        let mut vm = LocalVm::new(scope, &node, Vec::new(), Fuel::new(1000), STACK);
        let work = next(&mut vm, host);
        let receipt = dispatch(&mut vm, host, work).unwrap();
        let other_ref = other.owner().reference(&host.lease()).unwrap();
        let rejected = vm
            .try_accept(&receipt.identity, Reply::Object(other_ref.clone()), host)
            .err()
            .unwrap();
        assert!(matches!(rejected.reply, Reply::Object(_)));
        assert_eq!(vm.status(), EffectSessionStatus::Awaiting);
        accept(&mut vm, host, receipt);
        let work = next(&mut vm, host);
        let receipt = dispatch(&mut vm, host, work).unwrap();
        let other_snapshot = Rc::new(
            other
                .owner()
                .observe(&other_ref, &host.lease(), other.snapshot())
                .unwrap(),
        );
        let ptr = Rc::as_ptr(&other_snapshot);
        let fuel = vm.fuel_remaining();
        let rejected = vm
            .try_accept(&receipt.identity, Reply::Snapshot(other_snapshot), host)
            .err()
            .unwrap();
        let Reply::Snapshot(snapshot) = rejected.reply else {
            panic!();
        };
        assert_eq!(Rc::as_ptr(&snapshot), ptr);
        assert_eq!(vm.fuel_remaining(), fuel);
        assert_eq!(vm.status(), EffectSessionStatus::Awaiting);
        accept(&mut vm, host, receipt);
        assert_eq!(adapter.counts(), (1, 0, 0));
        assert_eq!(other.counts(), (0, 0, 0));
        vm.cancel();
    });
}
both!(
    native_replies_must_match_the_exact_discovered_or_observed_object,
    reply_target
);
