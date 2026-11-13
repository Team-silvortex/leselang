//! Runs on main(), because the ordinary Rust test harness uses worker threads.
use crate::driver::{self, Vm};
use host_protocol::{ObjectRef, Observation};
use leselang_appkit_host::{
    Error,
    gui::{Gui, WIDGET_COUNT, WidgetSnapshot},
    host::{self, Host, Receipt, Reply, STACK, Work},
};
use leselang_vm::native::{
    HostWorkOutcome, LocalVm,
    core::*,
    hir::{
        effect_session::{EffectSessionEnd, EffectSessionPoll, EffectSessionStatus},
        pure_evaluation::PureValue,
    },
};
use std::rc::Rc;

type Check = Result<(), String>;
fn session(
    gui: &Gui,
    source: &str,
    body: impl for<'ir, 's> FnOnce(&Host<'_, 's>, &mut Vm<'ir, 's>) -> Check,
) -> Check {
    host::with_host(gui, |host, scope| {
        let node = host.compile(source)?;
        let mut vm = LocalVm::new(scope, &node, Vec::new(), Fuel::new(1000), STACK);
        body(host, &mut vm)
    })
    .map_err(|e| e.to_string())?
}
fn find<'ir, 's>(vm: &mut Vm<'ir, 's>, host: &Host<'_, 's>) -> ObjectRef {
    let work = driver::next(vm, host).unwrap();
    assert_eq!(work.operation(), "gui.find");
    let receipt = driver::dispatch(vm, host, work).unwrap();
    let Reply::Object(object) = &receipt.reply else {
        panic!("expected native object");
    };
    let object = object.clone();
    driver::accept(vm, host, receipt).unwrap();
    object
}
fn observe<'ir, 's>(vm: &mut Vm<'ir, 's>, host: &Host<'_, 's>) -> Rc<Observation<WidgetSnapshot>> {
    let work = driver::next(vm, host).unwrap();
    assert_eq!(work.operation(), "gui.observe");
    let receipt = driver::dispatch(vm, host, work).unwrap();
    let Reply::Snapshot(snapshot) = &receipt.reply else {
        panic!("expected native observation");
    };
    let snapshot = snapshot.clone();
    driver::accept(vm, host, receipt).unwrap();
    snapshot
}
fn ready<'ir, 's>(vm: &mut Vm<'ir, 's>, host: &Host<'_, 's>) -> Work<'s> {
    let _ = find(vm, host);
    observe(vm, host);
    let work = driver::next(vm, host).unwrap();
    assert_eq!(work.operation(), "gui.click");
    host.authorize(work.identity()).unwrap();
    work
}
fn raw_dispatch<'ir, 's>(
    vm: &mut Vm<'ir, 's>,
    host: &Host<'_, 's>,
    work: Work<'s>,
) -> Result<Receipt, Error> {
    vm.with_metered_host_outcome(work, |work, fuel| host.dispatch(work, fuel))
        .unwrap()
}
fn empty_fuel(vm: &mut Vm<'_, '_>) {
    vm.with_metered_host_outcome((), |_, fuel| {
        fuel.charge(fuel.remaining()).unwrap();
        HostWorkOutcome::Pending(())
    })
    .unwrap();
}
fn failed(vm: &Vm<'_, '_>) {
    assert_eq!(
        vm.status(),
        EffectSessionStatus::Terminal(EffectSessionEnd::Failed)
    );
}
fn reset(gui: &Gui) -> Check {
    for slot in (0..WIDGET_COUNT).rev() {
        gui.replace(slot).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn real_controls(gui: &Gui) -> Check {
    assert_eq!(gui.find("save").unwrap(), 0);
    assert_eq!(gui.find("other").unwrap(), 1);
    assert!(gui.find("missing").is_err());
    assert_ne!(
        gui.snapshot(0).unwrap().identity,
        gui.snapshot(1).unwrap().identity
    );
    for selected in ["save", "other"] {
        reset(gui)?;
        let source = host::PROGRAM.replace("name: \"save\"", &format!("name: \"{selected}\""));
        let untouched = if selected == "save" { 1 } else { 0 };
        let before = gui.snapshot(untouched).unwrap();
        let summary = driver::run(gui, &source)?;
        assert_eq!(summary.value, ScalarValue::String("ready".into()));
        assert_eq!(summary.effects, 4);
        assert_eq!(summary.observations.last().unwrap().clicks, 1);
        assert_eq!(gui.snapshot(untouched).unwrap(), before);
    }
    Ok(())
}
fn phases(gui: &Gui) -> Check {
    session(gui, host::PROGRAM, |host, vm| {
        let _ = find(vm, host);
        let before = observe(vm, host);
        let work = driver::next(vm, host)?;
        let receipt = driver::dispatch(vm, host, work).unwrap();
        assert!(host.has_pending());
        assert_eq!(gui.snapshot(0).unwrap().clicks, 0);
        assert!(matches!(
            vm.poll(host).unwrap(),
            EffectSessionPoll::Awaiting
        ));
        let receipt = driver::pump(vm, host, receipt).unwrap();
        assert!(!host.has_pending());
        assert_eq!(gui.snapshot(0).unwrap().clicks, 1);
        assert_eq!(before.data().title, "draft");
        assert_eq!(vm.status(), EffectSessionStatus::Awaiting);
        driver::accept(vm, host, receipt)?;
        let after = observe(vm, host);
        assert!(after.revision() > before.revision());
        assert_eq!(after.data().title, "ready");
        assert!(
            matches!(vm.poll(host).unwrap(), EffectSessionPoll::Value(PureValue::Scalar(ScalarValue::String(value))) if value == "ready")
        );
        assert!(!host.lease().is_open());
        Ok(())
    })
}
fn fence(gui: &Gui, queued: bool, cause: usize) -> Check {
    session(gui, host::PROGRAM, |host, vm| {
        let work = ready(vm, host);
        // Keep the original request alive for either of the explicit fences.
        let mut work = Some(work);
        let receipt = queued.then(|| driver::dispatch(vm, host, work.take().unwrap()).unwrap());
        match cause {
            0 => {
                gui.set_title(0, "external").unwrap();
            }
            1 => {
                gui.set_title(0, "external").unwrap();
                gui.set_title(0, "draft").unwrap();
            }
            2 => {
                host.replace(0).unwrap();
            }
            3 => {
                gui.replace(0).unwrap();
            }
            4 => host.set_granted(false),
            5 => {
                gui.set_enabled(0, false).unwrap();
            }
            _ => unreachable!(),
        }
        let result = if let Some(receipt) = receipt {
            driver::pump(vm, host, receipt)
        } else {
            raw_dispatch(vm, host, work.take().unwrap())
        };
        assert!(result.is_err());
        if cause <= 1 {
            assert!(matches!(
                result,
                Err(Error::Object(host_protocol::ObjectError::StaleRevision))
            ));
        }
        if cause == 2 || cause == 3 {
            assert!(matches!(
                result,
                Err(Error::Object(host_protocol::ObjectError::StaleGeneration))
            ));
        }
        failed(vm);
        assert_eq!(gui.snapshot(0).unwrap().clicks, 0);
        Ok(())
    })
}
fn all_fences(gui: &Gui) -> Check {
    for queued in [false, true] {
        for cause in 0..6 {
            reset(gui)?;
            fence(gui, queued, cause)?;
        }
    }
    Ok(())
}
fn cancellation(gui: &Gui) -> Check {
    session(gui, host::PROGRAM, |host, vm| {
        let work = ready(vm, host);
        let receipt = driver::dispatch(vm, host, work).unwrap();
        assert!(vm.cancel());
        let original = vm
            .with_metered_host_outcome(receipt, |_, _| -> HostWorkOutcome<()> {
                panic!("cancelled native action entered");
            })
            .err()
            .unwrap();
        assert!(matches!(original.reply, Reply::Submitted(true)));
        assert!(
            vm.try_accept(&original.identity, original.reply, host)
                .is_err()
        );
        assert!(host.discard_pending());
        assert_eq!(gui.snapshot(0).unwrap().clicks, 0);
        Ok(())
    })?;
    session(gui, host::PROGRAM, |host, vm| {
        let work = ready(vm, host);
        let receipt = driver::dispatch(vm, host, work).unwrap();
        let receipt = driver::pump(vm, host, receipt).unwrap();
        assert!(vm.cancel());
        let fuel = vm.fuel_remaining();
        assert!(
            vm.try_accept(&receipt.identity, receipt.reply, host)
                .is_err()
        );
        assert_eq!(vm.fuel_remaining(), fuel);
        assert_eq!(gui.snapshot(0).unwrap().clicks, 1);
        assert_eq!(gui.snapshot(0).unwrap().title, "ready");
        Ok(())
    })
}
fn fuel(gui: &Gui) -> Check {
    host::with_host(gui, |host, scope| {
        let node = host.compile(host::PROGRAM).unwrap();
        let mut vm = LocalVm::new(scope, &node, Vec::new(), Fuel::new(0), STACK);
        assert!(vm.poll(host).is_err());
        assert_eq!(gui.snapshot(0).unwrap().clicks, 0);
    })
    .map_err(|e| e.to_string())?;
    session(gui, host::PROGRAM, |host, vm| {
        let work = ready(vm, host);
        let receipt = driver::dispatch(vm, host, work).unwrap();
        empty_fuel(vm);
        assert!(driver::pump(vm, host, receipt).is_err());
        assert_eq!(vm.fuel_remaining(), 0);
        assert_eq!(gui.snapshot(0).unwrap().clicks, 0);
        failed(vm);
        Ok(())
    })
}
fn reply_identity(gui: &Gui) -> Check {
    session(gui, host::PROGRAM, |host, vm| {
        let target = find(vm, host);
        let work = driver::next(vm, host)?;
        let receipt = driver::dispatch(vm, host, work).unwrap();
        for wrong_ref in [true, false] {
            let object = if wrong_ref {
                host.owner(1).reference(&host.lease()).unwrap()
            } else {
                target.clone()
            };
            let owner = if wrong_ref {
                host.owner(1)
            } else {
                host.owner(0)
            };
            let snapshot = Rc::new(
                owner
                    .observe(&object, &host.lease(), gui.snapshot(1).unwrap())
                    .unwrap(),
            );
            let ptr = Rc::as_ptr(&snapshot);
            let fuel = vm.fuel_remaining();
            let rejected = vm
                .try_accept(&receipt.identity, Reply::Snapshot(snapshot), host)
                .err()
                .unwrap();
            let Reply::Snapshot(snapshot) = rejected.reply else {
                panic!("lost original reply");
            };
            assert_eq!(Rc::as_ptr(&snapshot), ptr);
            assert_eq!(vm.fuel_remaining(), fuel);
            assert_eq!(vm.status(), EffectSessionStatus::Awaiting);
        }
        driver::accept(vm, host, receipt)?;
        vm.cancel();
        Ok(())
    })
}
fn partial_effect(gui: &Gui) -> Check {
    for uncertain in [false, true] {
        reset(gui)?;
        session(gui, host::PROGRAM, |host, vm| {
            let work = ready(vm, host);
            let receipt = driver::dispatch(vm, host, work).unwrap();
            let result = vm
                .with_metered_host_outcome(receipt, |receipt, fuel| {
                    let HostWorkOutcome::Pending(Ok(_receipt)) = host.pump(receipt, fuel) else {
                        panic!("real click failed");
                    };
                    // A later native delivery stage fails after the real target/action.
                    if uncertain {
                        HostWorkOutcome::Uncertain(Err::<(), _>(Error::Uncertain(
                            "delivery uncertain after real click",
                        )))
                    } else {
                        HostWorkOutcome::Failed(Err(Error::Native("failure after real click")))
                    }
                })
                .unwrap();
            assert!(result.is_err());
            assert_eq!(gui.snapshot(0).unwrap().clicks, 1);
            assert_eq!(gui.snapshot(0).unwrap().title, "ready");
            assert_eq!(
                vm.status(),
                EffectSessionStatus::Terminal(if uncertain {
                    EffectSessionEnd::HostUncertain
                } else {
                    EffectSessionEnd::Failed
                })
            );
            Ok(())
        })?;
    }
    Ok(())
}
fn native_limits(gui: &Gui) -> Check {
    let before = gui.snapshot(0).unwrap();
    assert!(
        gui.set_title(0, &"x".repeat(MAX_SCALAR_STRING_BYTES + 1))
            .is_err()
    );
    assert_eq!(gui.snapshot(0).unwrap(), before);
    gui.set_title(0, "保存 ✅").unwrap();
    assert_eq!(gui.snapshot(0).unwrap().title, "保存 ✅");
    assert!(std::thread::spawn(|| Gui::new().is_err()).join().unwrap());
    Ok(())
}
fn closed(gui: &Gui) -> Check {
    session(gui, host::PROGRAM, |host, vm| {
        let work = ready(vm, host);
        let receipt = driver::dispatch(vm, host, work).unwrap();
        gui.close().unwrap();
        assert!(driver::pump(vm, host, receipt).is_err());
        failed(vm);
        assert!(!host.lease().is_open());
        Ok(())
    })
}

const TEXT_PROGRAM: &str = r#"
fn main() = bind(target: gui.find(name: "input"), body: bind(
    before: gui.observe(target: target), body: bind(
        submitted: gui.set_text(target: target, expected_revision: field(value: before, name: "revision"), text: "保存 ✓"),
        body: bind(after: gui.observe(target: target), body: field(value: after, name: "value")),
    ),
))
"#;
fn ready_text<'ir, 's>(vm: &mut Vm<'ir, 's>, host: &Host<'_, 's>) -> Work<'s> {
    let _ = find(vm, host);
    observe(vm, host);
    let work = driver::next(vm, host).unwrap();
    assert_eq!(work.operation(), "gui.set_text");
    host.authorize(work.identity()).unwrap();
    work
}
fn form_workflow(gui: &Gui) -> Check {
    let ids = gui.list().unwrap();
    assert_eq!(ids.len(), WIDGET_COUNT);
    for id in ["input", "save", "other"] {
        assert!(ids.iter().any(|name| name == id));
    }
    let summary = driver::run(gui, "fn main() = gui.list()")?;
    assert_eq!(summary.value, ScalarValue::StringList(StringListValue(ids)));
    assert_eq!(summary.effects, 1);
    assert!(summary.observations.is_empty());
    assert_eq!(
        driver::run(gui, "fn main() = add(left: 20, right: 22)")?.value,
        ScalarValue::Integer(42)
    );
    reset(gui)?;
    // Native values cannot outlive this runner's execution scope. Rejecting
    // the final value must still preserve effects already accepted by the VM.
    let native_result = TEXT_PROGRAM.replace("field(value: after, name: \"value\")", "after");
    let error = driver::run(gui, &native_result).unwrap_err();
    assert!(error.contains("program returned a session-bound native value"));
    assert_eq!(gui.snapshot(2).unwrap().value, "保存 ✓");
    assert_eq!(gui.snapshot(0).unwrap().clicks, 0);
    for _ in 0..2 {
        reset(gui)?;
        // Replacing controls changes their position in the real view tree;
        // selection scans identifiers rather than depending on list offsets.
        gui.replace(2).unwrap();
        let untouched = gui.snapshot(1).unwrap();
        let summary = driver::run(gui, host::FORM_PROGRAM)?;
        assert_eq!(summary.value, ScalarValue::String("LeseLang ✓".into()));
        assert_eq!(summary.effects, 9);
        assert_eq!(summary.observations.len(), 4);
        let input = gui.snapshot(2).unwrap();
        let saved = gui.snapshot(0).unwrap();
        assert_eq!(input.role, "text_field");
        assert!(input.editable);
        assert_eq!(saved.role, "button");
        assert_eq!(saved.value, input.value);
        assert_eq!(saved.clicks, 1);
        assert_eq!(gui.snapshot(1).unwrap(), untouched);
    }
    Ok(())
}
fn text_phases(gui: &Gui) -> Check {
    session(gui, TEXT_PROGRAM, |host, vm| {
        let _ = find(vm, host);
        let before = observe(vm, host);
        let work = driver::next(vm, host)?;
        let receipt = driver::dispatch(vm, host, work).unwrap();
        assert_eq!(gui.snapshot(2).unwrap().value, "");
        assert!(host.has_pending());
        let save_before = host.refresh(0).unwrap().mutation;
        let receipt = driver::pump(vm, host, receipt).unwrap();
        assert_eq!(gui.snapshot(2).unwrap().value, "保存 ✓");
        assert!(host.refresh(0).unwrap().mutation > save_before);
        assert_eq!(before.data().value, "");
        assert_eq!(vm.status(), EffectSessionStatus::Awaiting);
        driver::accept(vm, host, receipt)?;
        let after = observe(vm, host);
        assert!(after.revision() > before.revision());
        assert!(
            matches!(vm.poll(host).unwrap(), EffectSessionPoll::Value(PureValue::Scalar(ScalarValue::String(value))) if value == "保存 ✓")
        );
        Ok(())
    })
}
fn text_fences(gui: &Gui) -> Check {
    for queued in [false, true] {
        for cause in 0..7 {
            reset(gui)?;
            session(gui, TEXT_PROGRAM, |host, vm| {
                let mut work = Some(ready_text(vm, host));
                let receipt =
                    queued.then(|| driver::dispatch(vm, host, work.take().unwrap()).unwrap());
                match cause {
                    0 => gui.set_value(2, "external").unwrap(),
                    1 => {
                        gui.set_value(2, "external").unwrap();
                        gui.set_value(2, "").unwrap();
                    }
                    2 => host.replace(2).unwrap(),
                    3 => gui.replace(2).unwrap(),
                    4 => host.set_granted(false),
                    5 => gui.set_enabled(2, false).unwrap(),
                    6 => gui.set_editable(2, false).unwrap(),
                    _ => unreachable!(),
                }
                let result = if let Some(receipt) = receipt {
                    driver::pump(vm, host, receipt)
                } else {
                    raw_dispatch(vm, host, work.take().unwrap())
                };
                assert!(result.is_err());
                assert_ne!(gui.snapshot(2).unwrap().value, "保存 ✓");
                failed(vm);
                Ok(())
            })?;
        }
    }
    Ok(())
}
fn save_dependency(gui: &Gui) -> Check {
    for queued in [false, true] {
        for cause in 0..3 {
            reset(gui)?;
            session(gui, host::PROGRAM, |host, vm| {
                let mut work = Some(ready(vm, host));
                let receipt =
                    queued.then(|| driver::dispatch(vm, host, work.take().unwrap()).unwrap());
                match cause {
                    0 => gui.set_value(2, "changed after save observation").unwrap(),
                    1 => {
                        gui.set_value(2, "external").unwrap();
                        gui.set_value(2, "").unwrap();
                    }
                    2 => gui.replace(2).unwrap(),
                    _ => unreachable!(),
                }
                let result = if let Some(receipt) = receipt {
                    driver::pump(vm, host, receipt)
                } else {
                    raw_dispatch(vm, host, work.take().unwrap())
                };
                assert!(matches!(
                    result,
                    Err(Error::Object(host_protocol::ObjectError::StaleRevision))
                ));
                assert_eq!(gui.snapshot(0).unwrap().clicks, 0);
                assert_eq!(gui.snapshot(0).unwrap().value, "");
                failed(vm);
                Ok(())
            })?;
        }
    }
    Ok(())
}
fn text_cancel_fuel(gui: &Gui) -> Check {
    for phase in 0..3 {
        reset(gui)?;
        session(gui, TEXT_PROGRAM, |host, vm| {
            let work = ready_text(vm, host);
            let receipt = driver::dispatch(vm, host, work).unwrap();
            if phase == 0 {
                empty_fuel(vm);
                assert!(driver::pump(vm, host, receipt).is_err());
                assert_eq!(vm.fuel_remaining(), 0);
                assert_eq!(gui.snapshot(2).unwrap().value, "");
                failed(vm);
            } else {
                let receipt = if phase == 2 {
                    driver::pump(vm, host, receipt).unwrap()
                } else {
                    receipt
                };
                assert!(vm.cancel());
                let fuel = vm.fuel_remaining();
                assert!(
                    vm.try_accept(&receipt.identity, receipt.reply, host)
                        .is_err()
                );
                if phase == 1 {
                    assert!(host.discard_pending());
                }
                assert_eq!(
                    gui.snapshot(2).unwrap().value,
                    if phase == 2 { "保存 ✓" } else { "" }
                );
                assert_eq!(vm.fuel_remaining(), fuel);
            }
            Ok(())
        })?;
    }
    Ok(())
}
fn roles_and_bounds(gui: &Gui) -> Check {
    for source in [
        TEXT_PROGRAM.replace("name: \"input\"", "name: \"save\""),
        host::PROGRAM.replace("name: \"save\"", "name: \"input\""),
    ] {
        reset(gui)?;
        assert!(driver::run(gui, &source).is_err());
        assert_eq!(gui.snapshot(0).unwrap().clicks, 0);
        assert_eq!(gui.snapshot(2).unwrap().value, "");
    }
    reset(gui)?;
    let value = "💾".repeat(MAX_SCALAR_STRING_BYTES / 4);
    gui.set_value(2, &value).unwrap();
    assert_eq!(gui.snapshot(2).unwrap().value, value);
    let before = gui.snapshot(2).unwrap();
    let save = gui.snapshot(0).unwrap();
    assert!(gui.set_value(2, &(value + "x")).is_err());
    assert_eq!(gui.snapshot(2).unwrap(), before);
    assert_eq!(gui.snapshot(0).unwrap(), save);
    reset(gui)?;
    session(gui, "fn main() = gui.list()", |host, vm| {
        let work = driver::next(vm, host)?;
        let receipt = driver::dispatch(vm, host, work).unwrap();
        let rejected = vm
            .try_accept(
                &receipt.identity,
                Reply::Identifiers(StringListValue(vec![
                    "x".repeat(MAX_SCALAR_STRING_BYTES + 1),
                ])),
                host,
            )
            .err()
            .unwrap();
        assert!(matches!(rejected.reply, Reply::Identifiers(_)));
        assert_eq!(vm.status(), EffectSessionStatus::Awaiting);
        driver::accept(vm, host, receipt)?;
        Ok(())
    })
}

fn typed_task(gui: &Gui) -> Check {
    let source = include_str!("../examples/text_task.lese");
    let inputs = |text: &str| {
        vec![
            ("text".into(), ScalarValue::String(text.into())),
            ("name".into(), ScalarValue::String("input".into())),
        ]
    };
    for text in ["typed native helper ✓", "second task input"] {
        let result = driver::run_task(gui, source, "main", inputs(text))?;
        assert_eq!(result.value, ScalarValue::Boolean(true));
        assert_eq!(result.effects, 4);
        assert_eq!(result.observations.len(), 2);
        assert_eq!(gui.snapshot(2).unwrap().value, text);
    }
    let before = gui.snapshot(2).unwrap();
    assert!(driver::run_task(gui, source, "main", vec![]).is_err());
    assert!(driver::run(gui, source).is_err());
    assert_eq!(gui.snapshot(2).unwrap(), before);
    host::with_host(gui, |host, _| {
        for source in [
            "fn bad(target: object) = gui.observe(target: target)\nfn main() = bad(target: \"forged\")",
            "fn bad(observed: snapshot) = field(value: observed, name: \"value\")\nfn main() = bind(target: gui.find(name: \"input\"), body: bad(observed: target))",
        ] {
            assert!(host.compile(source).is_err());
        }
    }).map_err(|error| error.to_string())?;
    Ok(())
}

pub fn run() -> Check {
    let gui = Gui::new().map_err(|e| e.to_string())?;
    gui.show().map_err(|e| e.to_string())?;
    type Case = (&'static str, fn(&Gui) -> Check);
    let cases: &[Case] = &[
        ("real controls and source-selected routing", real_controls),
        (
            "typed scalar task inputs and native helper parameters",
            typed_task,
        ),
        ("submit / pump / accept / observe phases", phases),
        (
            "revision, ABA, replacement, permission and enabled fences",
            all_fences,
        ),
        (
            "cancellation before and after native mutation",
            cancellation,
        ),
        ("original Fuel at start and queued action", fuel),
        (
            "exact native reply identity and retained rejected data",
            reply_identity,
        ),
        (
            "known and uncertain failure preserve real partial effects",
            partial_effect,
        ),
        (
            "UTF-8, native bounds and main-thread refusal",
            native_limits,
        ),
        (
            "bounded discovery and complete source form workflow",
            form_workflow,
        ),
        (
            "text submission, native pump and historical observations",
            text_phases,
        ),
        ("fourteen live text state and authority fences", text_fences),
        (
            "six Save dependency fences after input mutation",
            save_dependency,
        ),
        (
            "text cancellation, late replies and original Fuel",
            text_cancel_fuel,
        ),
        (
            "control roles, discovery bounds and UTF-8 text limits",
            roles_and_bounds,
        ),
        ("window teardown refuses pending click", closed),
    ];
    for (name, case) in cases {
        reset(&gui)?;
        gui.status(&format!("Conformance: {name}"))
            .map_err(|e| e.to_string())?;
        case(&gui).map_err(|e| format!("{name}: {e}"))?;
        println!("PASS {name}");
    }
    println!(
        "AppKit conformance: {} groups passed (including 32 stale-state/authority/dependency cases).",
        cases.len()
    );
    Ok(())
}
