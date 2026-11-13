//! Explicit host-side waiting around one ordinary LeseLang observation.
//! Prepare a dedicated Finder folder in list view with Text size 13 and open
//! its View Options panel. Pass that panel's exact title as the sole argument.
//! `--timeout-check` instead runs a read-only, intentionally failing check.

#[cfg(target_os = "macos")]
fn main() {
    if let Err(error) = run() {
        eprintln!("Finder observation wait: {error}");
        std::process::exit(1);
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("This desktop reference requires macOS and Accessibility access.");
    std::process::exit(1);
}

#[cfg(target_os = "macos")]
fn run() -> Result<(), String> {
    use leselang_macos_ax_host::{
        driver,
        host::{Field, ProfileReply, STACK},
        observation_wait::{self, FieldEquals, WaitPolicy, WaitStep},
        scope::{ApplicationSpec, WindowSelector},
        scope_access::ApplicationAccess,
        widgets,
    };
    use leselang_vm::native::{
        LocalVm,
        core::{Fuel, OptionalStringValue, ScalarValue},
        hir::{effect_session::EffectSessionPoll, pure_evaluation::PureValue},
    };
    use std::time::{Duration, Instant};

    let mut args = std::env::args().skip(1);
    let title = args
        .next()
        .ok_or("usage: finder_wait EXACT_VIEW_OPTIONS_TITLE [--timeout-check]")?;
    let timeout_check = match args.next().as_deref() {
        None => false,
        Some("--timeout-check") => true,
        _ => return Err("only --timeout-check is supported after the window title".into()),
    };
    if args.next().is_some() {
        return Err("unexpected extra arguments".into());
    }
    let application = ApplicationAccess::connect(&ApplicationSpec {
        bundle_path: "/System/Library/CoreServices/Finder.app".into(),
        pid: None,
    })
    .map_err(|error| error.to_string())?;
    let backend = application
        .widgets(&WindowSelector::Exact {
            title: Some(title),
            identifier: Some("ViewOptions".into()),
            document: None,
        })
        .map_err(|error| error.to_string())?;
    let source = if timeout_check {
        r#"fn main() = bind(owner: gui.select(role: "AXPopUpButton", identifier: "Text size:", label: ""), body:
            bind(after: gui.observe(target: owner), body: true))"#
    } else {
        include_str!("widgets_finder_options.lese")
    };
    widgets::with_host(&backend, |host, scope| {
        let node = host.compile(source)?;
        let mut vm = LocalVm::new(scope, &node, Vec::new(), Fuel::new(10_000), STACK);
        let origin = Instant::now();
        let mut clock = || origin.elapsed();
        let mut effects = 0;
        let mut chosen = 0;
        let mut verify_next = timeout_check;
        let mut verified = false;
        loop {
            match vm.poll(host).map_err(|error| format!("{error:?}"))? {
                EffectSessionPoll::Request(work) => {
                    effects += 1;
                    let operation = work.operation();
                    println!("Effect {effects}: poll -> authorize -> dispatch: {operation}");
                    let mut receipt = driver::dispatch(&mut vm, host, work)
                        .map_err(|error| error.to_string())?;
                    if receipt.reply.is_submission() {
                        receipt = driver::pump(&mut vm, host, receipt)
                            .map_err(|error| error.to_string())?;
                        if operation == "gui.choose_menu_item" {
                            chosen += 1;
                            verify_next = true;
                        }
                    } else if operation == "gui.observe" && verify_next {
                        // This reference host selects the postcondition explicitly.
                        // Ordinary gui.observe and its source schema remain unchanged.
                        let condition = FieldEquals {
                            field: Field::Value,
                            expected: ScalarValue::OptionalString(OptionalStringValue(Some(
                                if timeout_check { "not a Finder text size" } else { "16" }.into(),
                            ))),
                        };
                        let policy = if timeout_check {
                            WaitPolicy {
                                max_attempts: 64,
                                interval: Duration::from_millis(50),
                                timeout: Duration::from_millis(250),
                            }
                        } else {
                            WaitPolicy::default()
                        };
                        if let ProfileReply::Snapshot(observed) = &receipt.reply {
                            println!("first observation: {:?}", observed.data().widget().map(|w| &w.control.value));
                        }
                        let mut progress = observation_wait::begin(
                            &mut vm, host, receipt, condition, policy, &mut clock,
                        ).map_err(|error| format!("{error:?}"))?;
                        receipt = loop {
                            match progress {
                                WaitStep::Ready(receipt) => break receipt,
                                WaitStep::Waiting(wait) => {
                                    let status = wait.status();
                                    println!("waiting: attempts={}, next_at={:?}, deadline={:?}",
                                        status.attempts, status.next_at, status.deadline);
                                    // Only this executable chooses to sleep. Embedders
                                    // can return to their own event loop instead.
                                    std::thread::sleep(status.next_at.saturating_sub(clock()));
                                    progress = observation_wait::step(&mut vm, host, wait, &mut clock)
                                        .map_err(|error| format!("{error:?}"))?;
                                }
                            }
                        };
                        if let ProfileReply::Snapshot(observed) = &receipt.reply {
                            println!("matched observation: {:?}, revision={}",
                                observed.data().widget().map(|w| &w.control.value), observed.revision());
                        }
                        verify_next = false;
                        verified = true;
                    }
                    driver::accept(&mut vm, host, receipt)?;
                    println!("accept: {operation}");
                }
                EffectSessionPoll::Value(PureValue::Scalar(ScalarValue::Boolean(true)))
                    if verified && chosen == 1 =>
                {
                    println!("Completed {effects} Effects; menu choices delivered: {chosen}; Fuel remaining: {}; result: true", vm.fuel_remaining());
                    return Ok(());
                }
                EffectSessionPoll::Value(_) => {
                    return Err("reference postcondition failed; no action is automatically retried".into());
                }
                _ => return Err("unexpected VM session boundary".into()),
            }
        }
    }).map_err(|error| error.to_string())?
}
