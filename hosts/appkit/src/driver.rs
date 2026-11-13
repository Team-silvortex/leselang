use leselang_appkit_host::{
    Error,
    gui::{Gui, WidgetSnapshot},
    host::{self, Domain, Host, Identity, NativeValue, Node, Receipt, Reply, STACK, Work},
};
use leselang_vm::native::{
    LocalVm,
    core::*,
    hir::{
        effect_reentry::RestoredEffectBindings, effect_session::EffectSessionPoll,
        pure_evaluation::PureValue,
    },
};

pub type Vm<'ir, 's> = LocalVm<
    'ir,
    's,
    Node,
    NativeValue,
    RestoredEffectBindings<'ir, NativeValue>,
    Identity,
    Domain<'s>,
    Reply,
>;

pub fn next<'ir, 's>(vm: &mut Vm<'ir, 's>, host: &Host<'_, 's>) -> Result<Work<'s>, String> {
    match vm
        .poll(host)
        .map_err(|e| format!("language execution failed: {e:?}"))?
    {
        EffectSessionPoll::Request(work) => Ok(work),
        _ => Err("expected an explicit Effect request".into()),
    }
}
pub fn dispatch<'ir, 's>(
    vm: &mut Vm<'ir, 's>,
    host: &Host<'_, 's>,
    work: Work<'s>,
) -> Result<Receipt, Error> {
    host.authorize(work.identity())?;
    vm.with_metered_host_outcome(work, |work, fuel| host.dispatch(work, fuel))
        .map_err(|_| Error::Native("VM is not awaiting native work"))?
}
pub fn accept(vm: &mut Vm<'_, '_>, host: &Host<'_, '_>, receipt: Receipt) -> Result<(), String> {
    vm.try_accept(&receipt.identity, receipt.reply, host)
        .map_err(|e| e.error.to_string())
}
pub fn pump(vm: &mut Vm<'_, '_>, host: &Host<'_, '_>, receipt: Receipt) -> Result<Receipt, Error> {
    vm.with_metered_host_outcome(receipt, |receipt, fuel| host.pump(receipt, fuel))
        .map_err(|_| Error::Native("VM is not awaiting queued work"))?
}

#[derive(Debug)]
pub struct RunSummary {
    pub value: ScalarValue,
    pub effects: usize,
    pub fuel_remaining: u64,
    pub observations: Vec<WidgetSnapshot>,
}

/// Execute caller-provided source. Demo postconditions do not constrain programs.
pub fn run(gui: &Gui, source: &str) -> Result<RunSummary, String> {
    drive(gui, source, None, false)
}

/// Run scalar task inputs through the same explicit AppKit Effect driver.
pub fn run_task(
    gui: &Gui,
    source: &str,
    entry: &str,
    arguments: Vec<(String, ScalarValue)>,
) -> Result<RunSummary, String> {
    drive(gui, source, Some((entry, arguments)), false)
}

type TaskInputs<'a> = (&'a str, Vec<(String, ScalarValue)>);

fn drive(
    gui: &Gui,
    source: &str,
    task: Option<TaskInputs<'_>>,
    report_json: bool,
) -> Result<RunSummary, String> {
    macro_rules! trace {
        ($($argument:tt)*) => {
            if report_json {
                eprintln!($($argument)*);
            } else {
                println!($($argument)*);
            }
        };
    }
    host::with_host(gui, |host, scope| {
        let (node, parameters, arguments) = if let Some((entry, arguments)) = task {
            let program = host.compile_task(source, entry)?;
            let arguments = leselang_vm::embedding::bind_entry_arguments(
                &program.entry_parameters,
                arguments,
                STACK.effects.pure.max_bindings,
            ).map_err(|error| error.to_string())?;
            (program.node, program.entry_parameters, arguments)
        } else {
            (host.compile(source)?, Vec::new(), Vec::new())
        };
        let bindings = parameters.iter().zip(arguments)
            .map(|(parameter, (_, value))| (parameter.name.as_str(), PureValue::Scalar(value)))
            .collect();
        let mut vm = LocalVm::new(scope, &node, bindings, Fuel::new(1000), STACK);
        let mut effects = 0;
        let mut observations = Vec::new();
        loop {
            match vm.poll(host).map_err(|e| format!("language execution failed: {e:?}"))? {
                EffectSessionPoll::Request(work) => {
                    effects += 1;
                    let operation = work.operation();
                    trace!("poll -> authorize -> dispatch: {operation}");
                    let receipt = dispatch(&mut vm, host, work).map_err(|e| e.to_string())?;
                    if let Reply::Snapshot(snapshot) = &receipt.reply {
                        trace!("observe: id={:?}, role={:?}, title={:?}, value={:?}, revision={}", snapshot.data().identifier, snapshot.data().role, snapshot.data().title, snapshot.data().value, snapshot.revision());
                        observations.push(snapshot.data().clone());
                    }
                    let receipt = if matches!(receipt.reply, Reply::Submitted(true)) {
                        trace!("submitted: native work remains queued; explicit pump");
                        pump(&mut vm, host, receipt).map_err(|e| e.to_string())?
                    } else { receipt };
                    accept(&mut vm, host, receipt)?;
                    trace!("accept: {operation}");
                }
                EffectSessionPoll::Value(PureValue::Scalar(value)) => {
                    return Ok(RunSummary { value, effects, fuel_remaining: vm.fuel_remaining(), observations });
                }
                EffectSessionPoll::Value(PureValue::Result(_)) => {
                    return Err("program returned a session-bound native value; project scalar fields before returning".into());
                }
                _ => return Err("unexpected VM session boundary".into()),
            }
        }
    }).map_err(|e| e.to_string())?
}

pub fn main(options: crate::options::Options) -> Result<(), String> {
    use crate::options::Mode;
    if options.mode == Mode::Help {
        println!("{}", crate::options::HELP);
        return Ok(());
    }
    if options.mode == Mode::SelfTest {
        return crate::self_test::run();
    }
    // Bound file ingress before constructing any native GUI resources.
    let source = match &options.mode {
        Mode::Run(path) => crate::options::load_source(path)?,
        Mode::Form => host::FORM_PROGRAM.to_owned(),
        Mode::Buttons => host::PROGRAM.to_owned(),
        Mode::Help | Mode::SelfTest | Mode::Describe => unreachable!(),
    };
    let gui = Gui::new().map_err(|e| e.to_string())?;
    gui.show().map_err(|e| e.to_string())?;
    gui.status("Running LeseLang against the native controls.")
        .map_err(|e| e.to_string())?;
    for _ in 0..16 {
        if !gui.pump_event().map_err(|e| e.to_string())? {
            break;
        }
    }
    let other_before = gui.snapshot(1).map_err(|e| e.to_string())?;
    // These direct samples are host verification evidence, outside the VM's
    // Effects and Fuel. Never export native identities or use them as grants.
    let native_before = if options.report_json {
        Some(native_snapshots(&gui)?)
    } else {
        None
    };
    let task = (options.entry.is_some() || !options.arguments.is_empty()).then_some((
        options.entry.as_deref().unwrap_or("main"),
        options.arguments,
    ));
    let result = drive(&gui, &source, task, options.report_json)?;
    let report = if let Some(before) = native_before {
        let after = native_snapshots(&gui)?;
        Some(crate::report::encode(
            &result.value,
            result.effects,
            result.fuel_remaining,
            &before.each_ref().map(crate::report::Snapshot::from),
            &after.each_ref().map(crate::report::Snapshot::from),
        )?)
    } else {
        None
    };
    let status = match options.mode {
        Mode::Form => {
            let input = gui.snapshot(2).map_err(|e| e.to_string())?;
            let saved = gui.snapshot(0).map_err(|e| e.to_string())?;
            if result.value != ScalarValue::String(input.value.clone())
                || input.value != "LeseLang ✓"
                || saved.value != input.value
                || saved.clicks != 1
                || gui.snapshot(1).map_err(|e| e.to_string())? != other_before
            {
                return Err("form postcondition did not match native controls".into());
            }
            format!(
                "VERIFIED — discovered controls, filled input, submitted Save\nInput: {:?}\nSaved: {:?}; callbacks: {}\nOther unchanged. {} Effects; Fuel remaining: {}",
                input.value, saved.value, saved.clicks, result.effects, result.fuel_remaining
            )
        }
        Mode::Buttons => {
            let saved = gui.snapshot(0).map_err(|e| e.to_string())?;
            if result.value != ScalarValue::String("ready".into())
                || saved.title != "ready"
                || saved.clicks != 1
                || gui.snapshot(1).map_err(|e| e.to_string())? != other_before
            {
                return Err("button postcondition did not match native controls".into());
            }
            format!(
                "VERIFIED — real AppKit target/action\nSave: draft -> ready; clicks: 0 -> 1\nOther unchanged. {} Effects; Fuel remaining: {}",
                result.effects, result.fuel_remaining
            )
        }
        Mode::Run(_) => format!(
            "Program completed. {} Effects; Fuel remaining: {}\nResult: {}",
            result.effects,
            result.fuel_remaining,
            crate::options::render_value(&result.value)
                .chars()
                .take(512)
                .collect::<String>()
        ),
        Mode::Help | Mode::SelfTest | Mode::Describe => unreachable!(),
    };
    gui.status(&status).map_err(|e| e.to_string())?;
    if let Some(report) = report {
        crate::report::write(&mut std::io::stdout().lock(), &report)?;
    } else {
        println!(
            "{status}\nLeseLang result: {}",
            crate::options::render_value(&result.value)
        );
    }
    if options.hold {
        println!("Window remains interactive; close it to exit.");
        while gui.is_open().map_err(|e| e.to_string())? {
            gui.pump_event().map_err(|e| e.to_string())?;
            std::thread::sleep(std::time::Duration::from_millis(16));
        }
    }
    Ok(())
}

fn native_snapshots(gui: &Gui) -> Result<[WidgetSnapshot; 3], String> {
    Ok([
        gui.snapshot(0).map_err(|error| error.to_string())?,
        gui.snapshot(1).map_err(|error| error.to_string())?,
        gui.snapshot(2).map_err(|error| error.to_string())?,
    ])
}
