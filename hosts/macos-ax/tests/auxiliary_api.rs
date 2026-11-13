//! Portable approval/session tests. No test queries a desktop or grants OS access.
use leselang_macos_ax_host::{
    Error,
    controls::ControlInfo,
    driver,
    host::STACK,
    scope::{AuxiliaryGrant, AuxiliaryProcessSpec, MAX_AUXILIARY_PROCESSES},
    widgets::{
        self, CheckState, WidgetAction, WidgetCapabilities, WidgetInfo, WidgetScope, WidgetsBackend,
    },
};
use leselang_vm::native::{LocalVm, core::Fuel};
use std::cell::RefCell;

fn process(pid: u32) -> AuxiliaryProcessSpec {
    AuxiliaryProcessSpec {
        pid,
        executable_path: "/System/Library/TestService.xpc/Contents/MacOS/TestService".into(),
        start_seconds: 1_800_000_000,
        start_micros: 42,
    }
}

#[test]
fn explicit_auxiliary_approval_requires_bounded_complete_unique_identities() {
    let spec = process(42);
    spec.validate().unwrap();
    assert!(AuxiliaryGrant::new(Vec::new()).is_err());
    assert!(AuxiliaryGrant::new(vec![spec.clone(), spec.clone()]).is_err());
    assert!(
        AuxiliaryGrant::new(
            (1..=MAX_AUXILIARY_PROCESSES as u32 + 1)
                .map(process)
                .collect()
        )
        .is_err()
    );
    let grant =
        AuxiliaryGrant::new((1..=MAX_AUXILIARY_PROCESSES as u32).map(process).collect()).unwrap();
    assert_eq!(grant.processes().len(), MAX_AUXILIARY_PROCESSES);
    for pid in [0, i32::MAX as u32 + 1, u32::MAX] {
        assert!(
            AuxiliaryGrant::new(vec![AuxiliaryProcessSpec {
                pid,
                ..spec.clone()
            }])
            .is_err()
        );
    }
    for executable_path in ["relative", "", "/bad\0path"] {
        assert!(
            AuxiliaryGrant::new(vec![AuxiliaryProcessSpec {
                executable_path: executable_path.into(),
                ..spec.clone()
            }])
            .is_err()
        );
    }
    assert!(
        AuxiliaryGrant::new(vec![AuxiliaryProcessSpec {
            executable_path: format!("/{}", "x".repeat(4096)).into(),
            ..spec.clone()
        }])
        .is_err()
    );
    assert!(
        AuxiliaryGrant::new(vec![AuxiliaryProcessSpec {
            start_seconds: 0,
            ..spec.clone()
        }])
        .is_err()
    );
    assert!(
        AuxiliaryGrant::new(vec![AuxiliaryProcessSpec {
            start_micros: 1_000_000,
            ..spec
        }])
        .is_err()
    );
}

#[test]
fn auxiliary_grant_clones_share_irreversible_revocation_without_changing_specs() {
    let spec = process(42);
    let grant = AuxiliaryGrant::new(vec![spec.clone()]).unwrap();
    let clone = grant.clone();
    grant.check_active().unwrap();
    assert!(!clone.is_revoked());
    assert!(clone.revoke());
    assert!(!grant.revoke());
    assert!(grant.is_revoked());
    assert!(matches!(grant.check_active(), Err(Error::PermissionDenied)));
    assert_eq!(grant.processes(), std::slice::from_ref(&spec));
    // A new approval is distinct and cannot revive a previously issued one.
    AuxiliaryGrant::new(vec![spec])
        .unwrap()
        .check_active()
        .unwrap();
    assert!(clone.check_active().is_err());
}

struct GrantedBackend {
    grant: AuxiliaryGrant,
    state: RefCell<WidgetScope>,
    writes: RefCell<Vec<bool>>,
}
impl GrantedBackend {
    fn new(grant: AuxiliaryGrant) -> Self {
        Self {
            grant,
            state: RefCell::new(WidgetScope {
                identity: 1,
                revision: 1,
                widgets: vec![WidgetInfo {
                    control: ControlInfo {
                        identity: 2,
                        key: "approved-window/check".into(),
                        role: "AXCheckBox".into(),
                        identifier: "option".into(),
                        label: String::new(),
                        value: None,
                        enabled: true,
                        editable: false,
                        pressable: false,
                    },
                    checked: Some(CheckState::Unchecked),
                    selected: None,
                    position: None,
                    orientation: None,
                    capabilities: WidgetCapabilities {
                        set_checked: true,
                        ..WidgetCapabilities::default()
                    },
                    parent: None,
                }],
            }),
            writes: RefCell::new(Vec::new()),
        }
    }
}
impl WidgetsBackend for GrantedBackend {
    fn snapshot(&self) -> Result<WidgetScope, Error> {
        self.grant.check_active()?;
        Ok(self.state.borrow().clone())
    }
    fn check_permission(&self) -> Result<(), Error> {
        self.grant.check_active()
    }
    fn perform(
        &self,
        expected: &WidgetScope,
        target: u64,
        action: &WidgetAction,
    ) -> Result<(), Error> {
        self.grant.check_active()?;
        let mut state = self.state.borrow_mut();
        if &*state != expected {
            return Err(Error::Stale);
        }
        expected.admit(target, action)?;
        let WidgetAction::SetChecked(value) = action else {
            return Err(Error::Invalid("unexpected action"));
        };
        self.writes.borrow_mut().push(*value);
        state.widgets[0].checked = Some(if *value {
            CheckState::Checked
        } else {
            CheckState::Unchecked
        });
        state.revision += 1;
        Ok(())
    }
}

const CHECK: &str = r#"fn main() = bind(target: gui.select(role: "AXCheckBox", identifier: "option", label: ""), body: bind(before: gui.observe(target: target), body: gui.set_checked(target: target, expected_revision: field(value: before, name: "revision"), checked: true)))"#;

#[test]
fn auxiliary_revocation_blocks_dispatch_queued_delivery_and_accept_without_undoing_effects() {
    for stage in 0..3 {
        let grant = AuxiliaryGrant::new(vec![process(42)]).unwrap();
        let backend = GrantedBackend::new(grant.clone());
        widgets::with_host(&backend, |host, scope| {
            let node = host.compile(CHECK).unwrap();
            let mut vm = LocalVm::new(scope, &node, Vec::new(), Fuel::new(10_000), STACK);
            for _ in 0..2 {
                let work = driver::next(&mut vm, host).unwrap();
                let receipt = driver::dispatch(&mut vm, host, work).unwrap();
                driver::accept(&mut vm, host, receipt).unwrap();
            }
            let work = driver::next(&mut vm, host).unwrap();
            assert_eq!(work.operation(), "gui.set_checked");
            if stage == 0 {
                grant.revoke();
                assert!(driver::dispatch(&mut vm, host, work).is_err());
            } else {
                let receipt = driver::dispatch(&mut vm, host, work).unwrap();
                if stage == 1 {
                    grant.revoke();
                    assert!(driver::pump(&mut vm, host, receipt).is_err());
                } else {
                    let receipt = driver::pump(&mut vm, host, receipt).unwrap();
                    grant.revoke();
                    assert!(driver::accept(&mut vm, host, receipt).is_err());
                }
            }
        })
        .unwrap();
        assert_eq!(backend.writes.borrow().len(), usize::from(stage == 2));
        assert_eq!(
            backend.state.borrow().widgets[0].checked,
            Some(if stage == 2 {
                CheckState::Checked
            } else {
                CheckState::Unchecked
            })
        );
        assert!(widgets::run(&backend, CHECK).is_err());
        assert!(backend.snapshot().is_err());
        let current = backend.state.borrow().clone();
        assert!(
            backend
                .perform(&current, 2, &WidgetAction::SetChecked(false))
                .is_err()
        );
        assert_eq!(backend.writes.borrow().len(), usize::from(stage == 2));
    }
}

#[test]
fn auxiliary_revocation_rejects_targetless_discovery_receipts() {
    let grant = AuxiliaryGrant::new(vec![process(42)]).unwrap();
    let backend = GrantedBackend::new(grant.clone());
    widgets::with_host(&backend, |host, scope| {
        let node = host.compile(CHECK).unwrap();
        let mut vm = LocalVm::new(scope, &node, Vec::new(), Fuel::new(10_000), STACK);
        let work = driver::next(&mut vm, host).unwrap();
        let receipt = driver::dispatch(&mut vm, host, work).unwrap();
        grant.revoke();
        assert!(driver::accept(&mut vm, host, receipt).is_err());
    })
    .unwrap();
    assert!(backend.writes.borrow().is_empty());
}

#[cfg(target_os = "macos")]
#[test]
fn auxiliary_native_entry_points_are_public_without_accessing_any_process() {
    use leselang_macos_ax_host::{
        scope::WindowSelector, scope_access::ApplicationAccess, widgets_access::WidgetsAccess,
    };
    let _: fn(&ApplicationAccess, &WindowSelector) -> Result<Vec<AuxiliaryProcessSpec>, Error> =
        ApplicationAccess::auxiliary_processes;
    let _: fn(
        &ApplicationAccess,
        &WindowSelector,
        &AuxiliaryGrant,
    ) -> Result<WidgetsAccess, Error> = ApplicationAccess::widgets_with_auxiliary;
    let _: fn(&WidgetsAccess) -> Result<(), Error> = WidgetsAccess::revoke_auxiliary;
}
