//! Typed state-wait contracts through public APIs; no native GUI or scheduler.
use leselang_macos_ax_host::{
    Error,
    controls::ControlInfo,
    driver,
    host::{Host, ProfileReceipt, ProfileReply, STACK, Work},
    observation_wait::{self, ObservationWait, WaitError, WaitStep},
    scope::{AuxiliaryGrant, AuxiliaryProcessSpec},
    widgets::{
        self, CheckState, WidgetAction, WidgetCapabilities, WidgetInfo, WidgetScope,
        WidgetSnapshot, WidgetsBackend,
    },
};
use leselang_vm::native::{
    HostWorkOutcome, LocalVm,
    core::{Fuel, host_protocol::Observation},
    hir::effect_session::EffectSessionPoll,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

#[derive(Clone, Copy, Debug)]
enum Kind {
    Checked,
    Selected,
    Position,
}
const KINDS: [Kind; 3] = [Kind::Checked, Kind::Selected, Kind::Position];
impl Kind {
    fn operation(self) -> &'static str {
        match self {
            Self::Checked => "gui.wait_checked",
            Self::Selected => "gui.wait_selected",
            Self::Position => "gui.wait_position",
        }
    }
    fn target(self) -> u64 {
        match self {
            Self::Checked => 2,
            Self::Selected => 3,
            Self::Position => 4,
        }
    }
    fn expected(self, high: bool) -> &'static str {
        match (self, high) {
            (Self::Position, true) => "1000",
            (Self::Position, false) => "0",
            (_, true) => "true",
            (_, false) => "false",
        }
    }
    fn call(self, expected: &str) -> String {
        call(self.operation(), expected)
    }
    fn program(self, high: bool) -> String {
        program(self.target(), &self.call(self.expected(high)))
    }
    fn set(self, backend: &Window, high: bool) {
        backend.edit(self.target(), |widget| match self {
            Self::Checked => {
                widget.checked = Some(if high {
                    CheckState::Checked
                } else {
                    CheckState::Unchecked
                })
            }
            Self::Selected => widget.selected = Some(high),
            Self::Position => widget.position = Some(if high { 1000 } else { 0 }),
        });
    }
    fn remove(self, backend: &Window) {
        backend.edit(self.target(), |widget| match self {
            Self::Checked => widget.checked = None,
            Self::Selected => widget.selected = None,
            Self::Position => widget.position = None,
        });
    }
}
fn call(operation: &str, expected: &str) -> String {
    format!(
        "{operation}(target: target, expected: {expected}, max_attempts: 4, interval_ms: 10, timeout_ms: 50)"
    )
}
fn program(target: u64, call: &str) -> String {
    format!(
        "fn main() = bind(target: gui.find(key: \"fixture/{target}\"), body: bind(after: {call}, body: true))"
    )
}
fn widget(identity: u64, role: &str) -> WidgetInfo {
    WidgetInfo {
        control: ControlInfo {
            identity,
            key: format!("fixture/{identity}"),
            role: role.into(),
            identifier: String::new(),
            label: String::new(),
            value: Some("same".into()),
            enabled: true,
            editable: false,
            pressable: false,
        },
        checked: None,
        selected: None,
        position: None,
        orientation: None,
        capabilities: WidgetCapabilities::default(),
        parent: None,
    }
}
struct Window {
    state: RefCell<WidgetScope>,
    reads: Cell<usize>,
    permitted: Cell<bool>,
    grant: Option<AuxiliaryGrant>,
    actions: RefCell<Vec<WidgetAction>>,
}
impl Window {
    fn new() -> Self {
        let mut check = widget(2, "AXCheckBox");
        check.checked = Some(CheckState::Unchecked);
        check.capabilities.set_checked = true;
        // A checkbox can also be an observed item of a collection. This lets
        // cross-operation tests keep the exact same native target and payload.
        check.selected = Some(false);
        check.parent = Some(10);
        let mut row = widget(3, "AXRow");
        row.selected = Some(false);
        row.parent = Some(10);
        let mut scroll = widget(4, "AXScrollBar");
        scroll.position = Some(0);
        scroll.orientation = Some("vertical".into());
        scroll.capabilities.scroll = true;
        let mut list = widget(10, "AXList");
        list.capabilities.select_item = true;
        Self {
            state: RefCell::new(WidgetScope {
                identity: 1,
                revision: 1,
                widgets: vec![check, row, scroll, list, widget(20, "AXList")],
            }),
            reads: Cell::new(0),
            permitted: Cell::new(true),
            grant: None,
            actions: RefCell::new(Vec::new()),
        }
    }
    fn edit(&self, identity: u64, edit: impl FnOnce(&mut WidgetInfo)) {
        let mut state = self.state.borrow_mut();
        edit(
            state
                .widgets
                .iter_mut()
                .find(|widget| widget.control.identity == identity)
                .unwrap(),
        );
        state.revision += 1;
    }
}
impl WidgetsBackend for Window {
    fn snapshot(&self) -> Result<WidgetScope, Error> {
        self.check_permission()?;
        self.reads.set(self.reads.get() + 1);
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
        if &*self.state.borrow() != expected {
            return Err(Error::Stale);
        }
        expected.admit(target, action)?;
        // Delivery and visible state completion are independent events.
        self.actions.borrow_mut().push(action.clone());
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
    widgets::with_state_wait_host(backend, |host, scope| {
        let node = host.compile(source).unwrap();
        let mut vm = LocalVm::new(scope, &node, Vec::new(), Fuel::new(30_000), STACK);
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
fn prior_catalogs_reject_all_state_waits_in_active_and_cold_code() {
    let backend = Window::new();
    for kind in KINDS {
        let normal = kind.program(true);
        let cold = normal.replace(
            &kind.call(kind.expected(true)),
            &format!(
                "choose(when: true, then: gui.observe(target: target), otherwise: {})",
                kind.call(kind.expected(true))
            ),
        );
        let unused = format!(
            "fn unused() = {}\nfn main() = true",
            normal.strip_prefix("fn main() = ").unwrap()
        );
        for source in [&normal, &cold, &unused] {
            widgets::with_host(&backend, |host, _| {
                assert!(host.compile(source).is_err());
            })
            .unwrap();
            widgets::with_wait_host(&backend, |host, _| {
                assert!(host.compile(source).is_err());
            })
            .unwrap();
            widgets::with_state_wait_host(&backend, |host, _| {
                let reads = backend.reads.get();
                host.compile(source).unwrap();
                assert_eq!(backend.reads.get(), reads);
            })
            .unwrap();
        }
    }
    assert!(backend.actions.borrow().is_empty());
}

#[test]
fn expected_types_and_five_named_arguments_are_strict() {
    let backend = Window::new();
    widgets::with_state_wait_host(&backend, |host, _| {
        for kind in KINDS {
            for expected in [kind.expected(false), kind.expected(true)] {
                host.compile(&program(kind.target(), &kind.call(expected)))
                    .unwrap();
            }
            let wrong = if matches!(kind, Kind::Position) {
                ["false", "\"0\"", "optional_string(value: \"0\")"]
            } else {
                ["0", "\"false\"", "optional_string(value: \"false\")"]
            };
            for expected in wrong {
                assert!(
                    host.compile(&program(kind.target(), &kind.call(expected)))
                        .is_err()
                );
            }
            for (from, to) in [
                ("target: target", "target: \"fixture/2\""),
                ("max_attempts: 4", "max_attempts: false"),
                (", timeout_ms: 50", ""),
                ("timeout_ms: 50", "timeout_ms: 50, timeout_ms: 50"),
                ("timeout_ms: 50", "timeout_ms: 50, retry: true"),
            ] {
                assert!(host.compile(&kind.program(true).replace(from, to)).is_err());
            }
        }
    })
    .unwrap();
}

#[test]
fn mixed_checkbox_matches_neither_boolean_but_can_later_converge() {
    for expected in [false, true] {
        let backend = Window::new();
        backend.edit(2, |widget| widget.checked = Some(CheckState::Mixed));
        with_work(
            &backend,
            &Kind::Checked.program(expected),
            1,
            |host, vm, work, _| {
                let wait = waiting(
                    observation_wait::begin_source(vm, host, work, &mut || Duration::ZERO).unwrap(),
                );
                let wait = waiting(
                    observation_wait::step(vm, host, wait, &mut || Duration::from_millis(10))
                        .unwrap(),
                );
                assert_eq!(wait.status().attempts, 2);
                Kind::Checked.set(&backend, expected);
                let receipt = ready(
                    observation_wait::step(vm, host, wait, &mut || Duration::from_millis(20))
                        .unwrap(),
                );
                driver::accept(vm, host, receipt).unwrap();
                assert!(matches!(
                    vm.poll(host).unwrap(),
                    EffectSessionPoll::Value(_)
                ));
            },
        );
    }
}

#[test]
fn missing_state_is_unsupported_instead_of_false_or_zero() {
    for kind in KINDS {
        let backend = Window::new();
        kind.remove(&backend);
        with_work(&backend, &kind.program(false), 1, |host, vm, work, _| {
            error(
                observation_wait::begin_source(vm, host, work, &mut || Duration::ZERO),
                |error| matches!(error, WaitError::Host(Error::Invalid(_))),
            );
            assert!(matches!(
                vm.poll(host).unwrap(),
                EffectSessionPoll::Terminal(_)
            ));
        });
    }
}

#[test]
fn waiting_checks_current_availability_even_before_a_sample_is_due() {
    for kind in KINDS {
        for step_at in [0, 10] {
            let backend = Window::new();
            with_work(&backend, &kind.program(true), 1, |host, vm, work, _| {
                let wait = waiting(
                    observation_wait::begin_source(vm, host, work, &mut || Duration::ZERO).unwrap(),
                );
                kind.remove(&backend);
                error(
                    observation_wait::step(vm, host, wait, &mut || Duration::from_millis(step_at)),
                    |error| matches!(error, WaitError::Host(Error::Invalid(_))),
                );
                assert!(matches!(
                    vm.poll(host).unwrap(),
                    EffectSessionPoll::Terminal(_)
                ));
            });
        }
    }
}

#[test]
fn ready_preserves_historical_state_even_if_its_availability_later_disappears() {
    for kind in KINDS {
        let backend = Window::new();
        with_work(&backend, &kind.program(false), 1, |host, vm, work, _| {
            let receipt = ready(
                observation_wait::begin_source(vm, host, work, &mut || Duration::ZERO).unwrap(),
            );
            let ProfileReply::Snapshot(observed) = &receipt.reply else {
                panic!("snapshot")
            };
            let observed = observed.clone();
            kind.remove(&backend);
            driver::accept(vm, host, receipt).unwrap();
            assert!(matches!(
                vm.poll(host).unwrap(),
                EffectSessionPoll::Value(_)
            ));
            let original = observed.data().scope.widget(kind.target()).unwrap();
            match kind {
                Kind::Checked => assert_eq!(original.checked, Some(CheckState::Unchecked)),
                Kind::Selected => assert_eq!(original.selected, Some(false)),
                Kind::Position => assert_eq!(original.position, Some(0)),
            }
        });
    }
}

#[test]
fn observation_waits_do_not_require_enabled_mutation_capabilities() {
    for kind in KINDS {
        let backend = Window::new();
        backend.edit(kind.target(), |widget| {
            widget.control.enabled = false;
            widget.capabilities = WidgetCapabilities::default();
        });
        with_work(&backend, &kind.program(false), 1, |host, vm, work, _| {
            let receipt = ready(
                observation_wait::begin_source(vm, host, work, &mut || Duration::ZERO).unwrap(),
            );
            driver::accept(vm, host, receipt).unwrap();
        });
        assert!(backend.actions.borrow().is_empty());
    }
}

#[test]
fn selected_state_requires_a_collection_and_waits_pin_that_membership() {
    for missing_parent in [false, true] {
        let backend = Window::new();
        backend.edit(3, |widget| {
            widget.parent = if missing_parent { None } else { Some(4) }
        });
        assert!(
            widgets::with_state_wait_host(&backend, |_, _| panic!("invalid collection admitted"))
                .is_err()
        );
    }
    for step_at in [0, 10] {
        let backend = Window::new();
        with_work(
            &backend,
            &Kind::Selected.program(true),
            1,
            |host, vm, work, _| {
                let wait = waiting(
                    observation_wait::begin_source(vm, host, work, &mut || Duration::ZERO).unwrap(),
                );
                backend.edit(3, |widget| {
                    widget.parent = Some(20);
                    widget.selected = Some(true);
                });
                error(
                    observation_wait::step(vm, host, wait, &mut || Duration::from_millis(step_at)),
                    |error| matches!(error, WaitError::Host(Error::Stale)),
                );
            },
        );
    }
}

#[test]
fn normalized_position_bounds_are_checked_before_sampling() {
    for expected in ["1001", "65536", "4294967296"] {
        let backend = Window::new();
        with_work(
            &backend,
            &program(4, &Kind::Position.call(expected)),
            1,
            |host, vm, work, _| {
                let reads = backend.reads.get();
                error(
                    observation_wait::begin_source(vm, host, work, &mut || Duration::ZERO),
                    |error| matches!(error, WaitError::InvalidCondition),
                );
                assert_eq!(backend.reads.get(), reads);
            },
        );
    }
    let backend = Window::new();
    backend.edit(4, |widget| widget.position = Some(1001));
    assert!(
        widgets::with_state_wait_host(&backend, |_, _| panic!("invalid position admitted"))
            .is_err()
    );
}

#[test]
fn normalized_position_waits_for_exact_equality_including_interior_values() {
    let backend = Window::new();
    backend.edit(4, |widget| widget.position = Some(499));
    with_work(
        &backend,
        &program(4, &Kind::Position.call("500")),
        1,
        |host, vm, work, _| {
            let wait = waiting(
                observation_wait::begin_source(vm, host, work, &mut || Duration::ZERO).unwrap(),
            );
            backend.edit(4, |widget| widget.position = Some(501));
            let wait = waiting(
                observation_wait::step(vm, host, wait, &mut || Duration::from_millis(10)).unwrap(),
            );
            assert_eq!(wait.status().attempts, 2);
            backend.edit(4, |widget| widget.position = Some(500));
            let receipt = ready(
                observation_wait::step(vm, host, wait, &mut || Duration::from_millis(20)).unwrap(),
            );
            driver::accept(vm, host, receipt).unwrap();
        },
    );
}

#[test]
fn ready_historical_state_cannot_survive_native_identity_or_role_replacement() {
    for kind in KINDS {
        for role_change in [false, true] {
            let backend = Window::new();
            with_work(&backend, &kind.program(false), 1, |host, vm, work, _| {
                let receipt = ready(
                    observation_wait::begin_source(vm, host, work, &mut || Duration::ZERO).unwrap(),
                );
                backend.edit(kind.target(), |widget| {
                    if role_change {
                        widget.control.role = "AXStaticText".into();
                        widget.checked = None;
                        widget.position = None;
                        widget.orientation = None;
                        widget.capabilities = WidgetCapabilities::default();
                    } else {
                        widget.control.identity = 99;
                    }
                });
                backend.state.borrow().validate().unwrap();
                assert!(driver::accept(vm, host, receipt).is_err());
            });
        }
    }
}

#[test]
fn ordinary_dispatch_and_historical_matching_snapshots_cannot_complete_state_waits() {
    for kind in KINDS {
        let backend = Window::new();
        with_work(&backend, &kind.program(false), 1, |host, vm, work, _| {
            assert!(driver::dispatch(vm, host, work).is_err());
        });
        let source = kind.program(false).replace(
            "body: bind(after:",
            "body: bind(before: gui.observe(target: target), body: bind(after:",
        ) + ")";
        with_work(&backend, &source, 2, |host, vm, work, history| {
            let old = history[0].clone();
            let identity = work.identity().clone();
            kind.set(&backend, true);
            assert!(
                vm.try_accept(&identity, ProfileReply::Snapshot(old.clone()), host)
                    .is_err()
            );
            let wait = waiting(
                observation_wait::begin_source(vm, host, work, &mut || Duration::ZERO).unwrap(),
            );
            assert!(
                vm.try_accept(&identity, ProfileReply::Snapshot(old.clone()), host)
                    .is_err()
            );
            kind.set(&backend, false);
            let receipt = ready(
                observation_wait::step(vm, host, wait, &mut || Duration::from_millis(10)).unwrap(),
            );
            assert!(
                vm.try_accept(&receipt.identity, ProfileReply::Snapshot(old), host)
                    .is_err()
            );
            driver::accept(vm, host, receipt).unwrap();
            assert!(matches!(
                vm.poll(host).unwrap(),
                EffectSessionPoll::Value(_)
            ));
        });
    }
    let backend = Window::new();
    with_work(
        &backend,
        &Kind::Selected.program(true),
        1,
        |host, vm, work, _| {
            // The original request already identified the observed item's
            // collection. A delayed dispatcher cannot silently retarget it.
            backend.edit(3, |widget| {
                widget.parent = Some(20);
                widget.selected = Some(true);
            });
            error(
                observation_wait::begin_source(vm, host, work, &mut || Duration::ZERO),
                |error| matches!(error, WaitError::Host(Error::Stale)),
            );
        },
    );
}

#[test]
fn ready_proofs_cannot_be_transferred_between_wait_kinds_on_the_same_object() {
    for (target, first_op, first_expected, second_op, second_expected) in [
        (2, "gui.wait_checked", "false", "gui.wait_selected", "false"),
        (2, "gui.wait_selected", "false", "gui.wait_checked", "false"),
        (
            2,
            "gui.wait_value",
            "optional_string(value: \"same\")",
            "gui.wait_checked",
            "false",
        ),
        (
            4,
            "gui.wait_position",
            "0",
            "gui.wait_value",
            "optional_string(value: \"same\")",
        ),
        (
            4,
            "gui.wait_value",
            "optional_string(value: \"same\")",
            "gui.wait_position",
            "0",
        ),
    ] {
        let backend = Window::new();
        let first = call(first_op, first_expected);
        let second = call(second_op, second_expected);
        let source = format!(
            "fn main() = bind(target: gui.find(key: \"fixture/{target}\"), body: bind(first: {first}, body: bind(second: {second}, body: true)))"
        );
        with_work(&backend, &source, 1, |host, vm, work, _| {
            let receipt = ready(
                observation_wait::begin_source(vm, host, work, &mut || Duration::ZERO).unwrap(),
            );
            let ProfileReply::Snapshot(old) = &receipt.reply else {
                panic!("snapshot")
            };
            let old = old.clone();
            driver::accept(vm, host, receipt).unwrap();
            let next = driver::next(vm, host).unwrap();
            assert!(
                vm.try_accept(next.identity(), ProfileReply::Snapshot(old.clone()), host)
                    .is_err()
            );
            let receipt = ready(
                observation_wait::begin_source(vm, host, next, &mut || Duration::ZERO).unwrap(),
            );
            assert!(
                vm.try_accept(&receipt.identity, ProfileReply::Snapshot(old), host)
                    .is_err()
            );
            driver::accept(vm, host, receipt).unwrap();
            assert!(matches!(
                vm.poll(host).unwrap(),
                EffectSessionPoll::Value(_)
            ));
        });
    }
}

#[test]
fn permissions_and_auxiliary_revocation_block_waiting_and_ready_acceptance() {
    for kind in KINDS {
        for ready_first in [false, true] {
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
                with_work(
                    &backend,
                    &kind.program(!ready_first),
                    1,
                    |host, vm, work, _| {
                        let step =
                            observation_wait::begin_source(vm, host, work, &mut || Duration::ZERO)
                                .unwrap();
                        match authority {
                            0 => host.set_granted(false),
                            1 => backend.permitted.set(false),
                            _ => {
                                grant.revoke();
                            }
                        }
                        if ready_first {
                            assert!(driver::accept(vm, host, ready(step)).is_err());
                        } else {
                            error(
                                observation_wait::step(vm, host, waiting(step), &mut || {
                                    Duration::ZERO
                                }),
                                |error| matches!(error, WaitError::Host(Error::PermissionDenied)),
                            );
                        }
                    },
                );
            }
        }
    }
}

#[test]
fn cancellation_blocks_pending_steps_and_late_ready_replies() {
    for kind in KINDS {
        for ready_first in [false, true] {
            let backend = Window::new();
            with_work(
                &backend,
                &kind.program(!ready_first),
                1,
                |host, vm, work, _| {
                    let step =
                        observation_wait::begin_source(vm, host, work, &mut || Duration::ZERO)
                            .unwrap();
                    assert!(vm.cancel());
                    if ready_first {
                        assert!(driver::accept(vm, host, ready(step)).is_err());
                    } else {
                        let reads = backend.reads.get();
                        let fuel = vm.fuel_remaining();
                        error(
                            observation_wait::step(vm, host, waiting(step), &mut || {
                                panic!("cancelled wait read clock")
                            }),
                            |error| matches!(error, WaitError::InvalidState),
                        );
                        assert_eq!(backend.reads.get(), reads);
                        assert_eq!(vm.fuel_remaining(), fuel);
                    }
                },
            );
        }
    }
}

#[test]
fn state_waits_share_original_fuel_and_bounded_attempt_and_time_limits() {
    for kind in KINDS {
        for failure in 0..3 {
            let backend = Window::new();
            let source = kind
                .program(true)
                .replace("max_attempts: 4", "max_attempts: 2");
            with_work(&backend, &source, 1, |host, vm, work, _| {
                let wait = waiting(
                    observation_wait::begin_source(vm, host, work, &mut || Duration::ZERO).unwrap(),
                );
                if failure == 0 {
                    vm.with_metered_host_outcome((), |_, fuel| {
                        fuel.charge(fuel.remaining()).unwrap();
                        HostWorkOutcome::Pending(())
                    })
                    .unwrap();
                }
                error(
                    observation_wait::step(vm, host, wait, &mut || {
                        Duration::from_millis(if failure == 2 { 50 } else { 10 })
                    }),
                    |error| match failure {
                        0 => matches!(error, WaitError::Host(Error::Fuel)),
                        1 => matches!(error, WaitError::AttemptsExhausted),
                        _ => matches!(error, WaitError::TimedOut),
                    },
                );
                assert!(matches!(
                    vm.poll(host).unwrap(),
                    EffectSessionPoll::Terminal(_)
                ));
            });
        }
    }
}

#[test]
fn matching_state_after_the_sample_deadline_is_not_ready() {
    for kind in KINDS {
        let backend = Window::new();
        with_work(&backend, &kind.program(true), 1, |host, vm, work, _| {
            let wait = waiting(
                observation_wait::begin_source(vm, host, work, &mut || Duration::ZERO).unwrap(),
            );
            kind.set(&backend, true);
            let mut calls = 0;
            error(
                observation_wait::step(vm, host, wait, &mut || {
                    calls += 1;
                    Duration::from_millis(if calls == 1 { 10 } else { 50 })
                }),
                |error| matches!(error, WaitError::TimedOut),
            );
        });
    }
}

#[test]
fn typed_wait_success_and_failure_preserve_one_prior_native_delivery() {
    for kind in KINDS {
        for success in [false, true] {
            let backend = Window::new();
            let wait = kind.call(kind.expected(true));
            let (source, preceding) = match kind {
                Kind::Selected => (
                    format!(
                        "fn main() = bind(owner: gui.find(key: \"fixture/10\"), body: bind(target: gui.find(key: \"fixture/3\"), body: bind(before: gui.observe(target: owner), body: bind(sent: gui.select_item(target: owner, expected_revision: field(value: before, name: \"revision\"), item: target), body: bind(after: {wait}, body: true)))))"
                    ),
                    4,
                ),
                _ => {
                    let action = if matches!(kind, Kind::Checked) {
                        "gui.set_checked(target: target, expected_revision: field(value: before, name: \"revision\"), checked: true)"
                    } else {
                        "gui.scroll(target: target, expected_revision: field(value: before, name: \"revision\"), position: 1000)"
                    };
                    (
                        format!(
                            "fn main() = bind(target: gui.find(key: \"fixture/{}\"), body: bind(before: gui.observe(target: target), body: bind(sent: {action}, body: bind(after: {wait}, body: true))))",
                            kind.target()
                        ),
                        3,
                    )
                }
            };
            with_work(&backend, &source, preceding, |host, vm, work, _| {
                assert_eq!(backend.actions.borrow().len(), 1);
                let wait = waiting(
                    observation_wait::begin_source(vm, host, work, &mut || Duration::ZERO).unwrap(),
                );
                if success {
                    kind.set(&backend, true);
                    let receipt = ready(
                        observation_wait::step(vm, host, wait, &mut || Duration::from_millis(10))
                            .unwrap(),
                    );
                    driver::accept(vm, host, receipt).unwrap();
                    assert!(matches!(
                        vm.poll(host).unwrap(),
                        EffectSessionPoll::Value(_)
                    ));
                } else {
                    error(
                        observation_wait::step(vm, host, wait, &mut || Duration::from_millis(50)),
                        |error| matches!(error, WaitError::TimedOut),
                    );
                    kind.set(&backend, true);
                }
                assert_eq!(backend.actions.borrow().len(), 1);
            });
        }
    }
}
