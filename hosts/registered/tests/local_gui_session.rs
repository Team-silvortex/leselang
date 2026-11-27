//! Application changes may arrive between dispatch, the explicit pump and acceptance.
use std::cell::{Cell, RefCell};

use leselang_registered_host::{
    Error,
    gateway::{Backend, WidgetSnapshot},
    host::{self, Domain, Host, Identity, NativeValue, Node, Receipt, Reply, Work},
};
use leselang_vm::native::{
    LocalVm,
    core::{Fuel, ReplyAuthority, ScalarValue, host_protocol::ObjectError},
    hir::{
        effect_reentry::RestoredEffectBindings,
        effect_session::{EffectSessionEnd, EffectSessionPoll},
        pure_evaluation::PureValue,
    },
};

const EDIT: &str = r#"
fn main() = bind(target: gui.find(name: "draft"), body:
    bind(before: gui.observe(target: target), body:
        bind(saved: gui.set_text(target: target, expected_revision: field(value: before, name: "revision"), text: "edited"), body:
            bind(after: gui.observe(target: target), body:
                eq(left: field(value: after, name: "value"), right: "edited")))))
"#;

struct Editor {
    field: RefCell<WidgetSnapshot>,
    sibling: WidgetSnapshot,
    replacement: RefCell<Option<WidgetSnapshot>>,
    observations: Cell<usize>,
    writes: Cell<usize>,
    fail_after_write: Cell<bool>,
}

impl Editor {
    fn new() -> Self {
        let field = WidgetSnapshot {
            identity: 17,
            identifier: "draft".into(),
            role: "text_field".into(),
            title: "Draft".into(),
            value: "original".into(),
            editable: true,
            enabled: true,
            clicks: 0,
            mutation: 0,
        };
        Self {
            sibling: WidgetSnapshot {
                identity: 32,
                identifier: "subject".into(),
                ..field.clone()
            },
            field: RefCell::new(field),
            replacement: RefCell::new(None),
            observations: Cell::new(0),
            writes: Cell::new(0),
            fail_after_write: Cell::new(false),
        }
    }
}

impl Backend for Editor {
    fn object_count(&self) -> usize {
        2
    }
    fn is_open(&self) -> Result<bool, Error> {
        Ok(true)
    }
    fn list(&self) -> Result<Vec<String>, Error> {
        Ok(vec![
            self.field.borrow().identifier.clone(),
            self.sibling.identifier.clone(),
        ])
    }
    fn find(&self, name: &str) -> Result<usize, Error> {
        if name == self.field.borrow().identifier {
            Ok(0)
        } else if name == self.sibling.identifier {
            Ok(1)
        } else {
            Err(Error::Native("unknown draft field"))
        }
    }
    fn snapshot(&self, slot: usize) -> Result<WidgetSnapshot, Error> {
        match slot {
            0 => Ok(self.field.borrow().clone()),
            1 => Ok(self.sibling.clone()),
            _ => Err(Error::Native("unknown editor slot")),
        }
    }
    fn observe(&self, slot: usize) -> Result<(), Error> {
        assert_eq!(slot, 0);
        self.observations.set(self.observations.get() + 1);
        Ok(())
    }
    fn click(&self, _: usize, _: usize) -> Result<(), Error> {
        panic!("the editor must only receive text writes");
    }
    fn set_text(&self, slot: usize, identity: usize, text: &str) -> Result<(), Error> {
        assert_eq!(slot, 0);
        let mut field = self.field.borrow_mut();
        assert_eq!(field.identity, identity);
        field.value = text.into();
        field.mutation += 1;
        self.writes.set(self.writes.get() + 1);
        if self.fail_after_write.get() {
            return Err(Error::Native("editor lost its acknowledgement"));
        }
        Ok(())
    }
    fn replace(&self, slot: usize) -> Result<(), Error> {
        assert_eq!(slot, 0);
        let mut field = self.field.borrow_mut();
        if let Some(next) = self.replacement.take() {
            *field = next;
        } else {
            field.identity += 1;
        }
        Ok(())
    }
}

type Vm<'ir, 's> = LocalVm<
    'ir,
    's,
    Node,
    NativeValue,
    RestoredEffectBindings<'ir, NativeValue>,
    Identity,
    Domain<'s>,
    Reply,
>;

fn next<'s>(vm: &mut Vm<'_, 's>, host: &Host<'_, 's>, name: &str) -> Work<'s> {
    let EffectSessionPoll::Request(work) = vm.poll(host).unwrap() else {
        panic!("expected a request for {name}");
    };
    assert_eq!(work.operation(), name);
    host.authorize(work.identity()).unwrap();
    work
}

fn dispatch<'s>(
    vm: &mut Vm<'_, 's>,
    host: &Host<'_, 's>,
    work: Work<'s>,
) -> Result<Receipt, Error> {
    vm.with_metered_host_outcome(work, |work, fuel| host.dispatch(work, fuel))
        .unwrap()
}

fn pump(vm: &mut Vm<'_, '_>, host: &Host<'_, '_>, receipt: Receipt) -> Result<Receipt, Error> {
    vm.with_metered_host_outcome(receipt, |receipt, fuel| host.pump(receipt, fuel))
        .unwrap()
}

fn with_observed_field(
    editor: &Editor,
    check: impl for<'s> FnOnce(&Host<'_, 's>, &mut Vm<'_, 's>),
) {
    host::with_host(editor, |host, scope| {
        let program = host.compile(EDIT).unwrap();
        let mut vm = LocalVm::new(scope, &program, vec![], Fuel::new(10_000), host::STACK);
        for operation in ["gui.find", "gui.observe"] {
            let work = next(&mut vm, host, operation);
            let receipt = dispatch(&mut vm, host, work).unwrap();
            vm.try_accept(&receipt.identity, receipt.reply, host)
                .unwrap();
        }
        assert_eq!(editor.observations.get(), 1);
        check(host, &mut vm);
    })
    .unwrap();
}

#[test]
fn the_pump_commits_once_and_only_acceptance_enables_the_verifying_observation() {
    let editor = Editor::new();
    with_observed_field(&editor, |host, vm| {
        let work = next(vm, host, "gui.set_text");
        let receipt = dispatch(vm, host, work).unwrap();
        assert!(host.has_pending());
        assert_eq!(editor.writes.get(), 0);
        let receipt = pump(vm, host, receipt).unwrap();
        assert!(!host.has_pending());
        assert_eq!(editor.writes.get(), 1);
        assert_eq!(editor.field.borrow().value, "edited");
        assert_eq!(editor.observations.get(), 1);
        assert!(matches!(
            vm.poll(host).unwrap(),
            EffectSessionPoll::Awaiting
        ));
        vm.try_accept(&receipt.identity, receipt.reply, host)
            .unwrap();

        let work = next(vm, host, "gui.observe");
        let after = dispatch(vm, host, work).unwrap();
        vm.try_accept(&after.identity, after.reply, host).unwrap();
        assert!(matches!(
            vm.poll(host).unwrap(),
            EffectSessionPoll::Value(PureValue::Scalar(ScalarValue::Boolean(true)))
        ));
        assert_eq!(editor.observations.get(), 2);
        assert_eq!(editor.writes.get(), 1);
    });
}

#[derive(Debug, Clone, Copy)]
enum Change {
    Revoked,
    Replaced,
    ExternalEdit,
    Disabled,
    ReadOnly,
}

#[test]
fn the_original_grant_target_and_observation_are_rechecked_at_dispatch_and_pump() {
    for queued in [false, true] {
        for change in [
            Change::Revoked,
            Change::Replaced,
            Change::ExternalEdit,
            Change::Disabled,
            Change::ReadOnly,
        ] {
            let editor = Editor::new();
            with_observed_field(&editor, |host, vm| {
                let work = next(vm, host, "gui.set_text");
                // The host already authorized this work. Interleave the change
                // either immediately before dispatch or after queue admission.
                let change_widget_or_grant = || match change {
                    Change::Revoked => host.set_granted(false),
                    Change::Replaced => host.replace(0).unwrap(),
                    Change::ExternalEdit => {
                        editor.field.borrow_mut().value = "someone else's edit".into()
                    }
                    Change::Disabled => editor.field.borrow_mut().enabled = false,
                    Change::ReadOnly => editor.field.borrow_mut().editable = false,
                };
                let result = if queued {
                    let receipt = dispatch(vm, host, work).unwrap();
                    change_widget_or_grant();
                    pump(vm, host, receipt)
                } else {
                    change_widget_or_grant();
                    dispatch(vm, host, work)
                };
                let error = result.unwrap_err();
                assert!(
                    matches!(
                        (change, &error),
                        (Change::Revoked, Error::Native("operation denied"))
                            | (
                                Change::Replaced,
                                Error::Object(ObjectError::StaleGeneration)
                            )
                            | (
                                Change::ExternalEdit,
                                Error::Object(ObjectError::StaleRevision)
                            )
                            | (Change::Disabled, Error::Native("control is disabled"))
                            | (
                                Change::ReadOnly,
                                Error::Native("text field is not editable")
                            )
                    ),
                    "{change:?}, queued={queued}: {error}"
                );
                assert!(matches!(
                    vm.poll(host).unwrap(),
                    EffectSessionPoll::Terminal(EffectSessionEnd::Failed)
                ));
                assert_eq!(editor.writes.get(), 0);
                assert_eq!(editor.observations.get(), 1);
                assert_eq!(
                    editor.field.borrow().value,
                    if matches!(change, Change::ExternalEdit) {
                        "someone else's edit"
                    } else {
                        "original"
                    }
                );
            });
        }
    }
}

#[test]
fn a_committed_edit_with_a_lost_acknowledgement_stops_as_uncertain_without_verifying_or_replaying()
{
    let editor = Editor::new();
    editor.fail_after_write.set(true);
    with_observed_field(&editor, |host, vm| {
        let work = next(vm, host, "gui.set_text");
        let receipt = dispatch(vm, host, work).unwrap();
        assert!(matches!(
            pump(vm, host, receipt),
            Err(Error::Native("editor lost its acknowledgement"))
        ));
        assert!(!host.has_pending());
        assert!(matches!(
            vm.poll(host).unwrap(),
            EffectSessionPoll::Terminal(EffectSessionEnd::HostUncertain)
        ));
        assert_eq!(editor.observations.get(), 1);
        assert_eq!(editor.writes.get(), 1);
        assert_eq!(editor.field.borrow().value, "edited");
    });
}

#[test]
fn cancellation_rejects_a_late_receipt_and_preserves_only_edits_already_pumped() {
    for delivered in [false, true] {
        let editor = Editor::new();
        with_observed_field(&editor, |host, vm| {
            let work = next(vm, host, "gui.set_text");
            let receipt = dispatch(vm, host, work).unwrap();
            let receipt = if delivered {
                pump(vm, host, receipt).unwrap()
            } else {
                receipt
            };
            assert!(vm.cancel());
            assert!(
                vm.try_accept(&receipt.identity, receipt.reply, host)
                    .is_err()
            );
            assert_eq!(host.discard_pending(), !delivered);
            assert!(matches!(
                vm.poll(host).unwrap(),
                EffectSessionPoll::Terminal(EffectSessionEnd::Cancelled)
            ));
            assert_eq!(editor.observations.get(), 1);
            assert_eq!(editor.writes.get(), usize::from(delivered));
            assert_eq!(
                editor.field.borrow().value,
                if delivered { "edited" } else { "original" }
            );
        });
    }
}

fn invalid_replacement(change: impl FnOnce(&mut WidgetSnapshot), reason: &str) {
    let editor = Editor::new();
    host::with_host(&editor, |host, scope| {
        let lease = scope.lease();
        let old = host.owner(0).reference(&lease).unwrap();
        let sibling = host.owner(1).reference(&lease).unwrap();
        let mut replacement = editor.snapshot(0).unwrap();
        replacement.identity += 1;
        change(&mut replacement);
        editor.replacement.replace(Some(replacement));

        let result = host.replace(0);
        assert!(
            matches!(result, Err(Error::Native(message)) if message == reason),
            "{result:?}"
        );
        assert_eq!(
            host.owner(0).validate(&old, &lease),
            Err(ObjectError::Retired)
        );
        assert!(matches!(
            host.owner(0).reference(&lease),
            Err(ObjectError::Retired)
        ));
        assert_eq!(host.refresh(1).unwrap(), editor.sibling);
        host.owner(1).validate(&sibling, &lease).unwrap();
    })
    .unwrap();
}

#[test]
fn replacing_a_slot_cannot_rename_its_registered_identifier() {
    invalid_replacement(
        |snapshot| snapshot.identifier = "subject".into(),
        "registered object slot changed its identifier",
    );
}

#[test]
fn replacing_a_slot_requires_a_valid_native_snapshot() {
    invalid_replacement(
        |snapshot| snapshot.identity = 0,
        "invalid native snapshot metadata",
    );
    invalid_replacement(
        |snapshot| snapshot.title = "x".repeat(4097),
        "snapshot text exceeds language bounds",
    );
}

#[test]
fn a_valid_replacement_resets_the_revision_and_can_be_observed_as_the_same_registration() {
    let editor = Editor::new();
    host::with_host(&editor, |host, scope| {
        let lease = scope.lease();
        let old = host.owner(0).reference(&lease).unwrap();
        editor.field.borrow_mut().value = "external edit".into();
        host.refresh(0).unwrap();
        assert!(host.owner(0).revision() > 0);

        host.replace(0).unwrap();
        assert_eq!(
            host.owner(0).validate(&old, &lease),
            Err(ObjectError::StaleGeneration)
        );
        let replacement = host.owner(0).reference(&lease).unwrap();
        let snapshot = host.refresh(0).unwrap();
        assert_eq!(snapshot.identifier, "draft");
        assert_eq!(snapshot.identity, 18);
        assert_eq!(host.owner(0).revision(), 0);
        host.owner(0).validate(&replacement, &lease).unwrap();
    })
    .unwrap();
}
