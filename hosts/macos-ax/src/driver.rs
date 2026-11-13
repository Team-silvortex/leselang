use crate::{
    Backend, Error, Snapshot,
    host::{
        self, Domain, Host, Identity, Node, Profile, ProfileNativeValue, ProfileReceipt,
        ProfileReply, ProfileSnapshot, STACK, Work,
    },
    observation_wait::{self, WaitStep},
};
use leselang_vm::native::{
    LocalVm,
    core::*,
    hir::{
        effect_reentry::RestoredEffectBindings, effect_session::EffectSessionPoll,
        pure_evaluation::PureValue,
    },
};
use std::time::Duration;

pub type Vm<'ir, 's, S = Snapshot> = LocalVm<
    'ir,
    's,
    Node,
    ProfileNativeValue<S>,
    RestoredEffectBindings<'ir, ProfileNativeValue<S>>,
    Identity,
    Domain<'s, S>,
    ProfileReply<S>,
>;

pub fn next<'ir, 's, S: ProfileSnapshot>(
    vm: &mut Vm<'ir, 's, S>,
    host: &Host<'_, 's, S>,
) -> Result<Work<'s, S>, String> {
    match vm
        .poll(host)
        .map_err(|e| format!("language execution failed: {e:?}"))?
    {
        EffectSessionPoll::Request(work) => Ok(work),
        _ => Err("expected an explicit Effect request".into()),
    }
}
pub fn dispatch<'ir, 's, S: ProfileSnapshot>(
    vm: &mut Vm<'ir, 's, S>,
    host: &Host<'_, 's, S>,
    work: Work<'s, S>,
) -> Result<ProfileReceipt<S>, Error> {
    host.authorize(work.identity())?;
    vm.with_metered_host_outcome(work, |work, fuel| host.dispatch(work, fuel))
        .map_err(|_| Error::Invalid("VM is not awaiting native work"))?
}
pub fn accept<S: ProfileSnapshot>(
    vm: &mut Vm<'_, '_, S>,
    host: &Host<'_, '_, S>,
    receipt: ProfileReceipt<S>,
) -> Result<(), String> {
    vm.try_accept(&receipt.identity, receipt.reply, host)
        .map_err(|e| e.error.to_string())
}
pub fn pump<S: ProfileSnapshot>(
    vm: &mut Vm<'_, '_, S>,
    host: &Host<'_, '_, S>,
    receipt: ProfileReceipt<S>,
) -> Result<ProfileReceipt<S>, Error> {
    vm.with_metered_host_outcome(receipt, |receipt, fuel| host.pump(receipt, fuel))
        .map_err(|_| Error::Invalid("VM is not awaiting queued work"))?
}

/// Concrete TextEdit summary, preserving empty-observation constructor inference.
pub type RunSummary = ProfileRunSummary<Snapshot>;
#[derive(Debug)]
pub struct ProfileRunSummary<S: ProfileSnapshot> {
    pub value: ScalarValue,
    pub effects: usize,
    pub fuel_remaining: u64,
    pub observations: Vec<S>,
}

/// Execute caller-provided source. Demo postconditions do not constrain programs.
pub fn run(backend: &dyn Backend, source: &str) -> Result<RunSummary, String> {
    host::with_host(backend, |host, scope| {
        drive(host, scope, source, 1000, None)
    })
    .map_err(|e| e.to_string())?
}
pub(crate) fn run_profile<S: ProfileSnapshot>(
    backend: &dyn Profile<Snapshot = S>,
    source: &str,
    fuel: u64,
) -> Result<ProfileRunSummary<S>, String> {
    host::with_profile(backend, |host, scope| {
        drive(host, scope, source, fuel, None)
    })
    .map_err(|e| e.to_string())?
}

pub(crate) fn run_profile_task<S: ProfileSnapshot>(
    backend: &dyn Profile<Snapshot = S>,
    source: &str,
    entry: &str,
    arguments: Vec<(String, ScalarValue)>,
    fuel: u64,
) -> Result<ProfileRunSummary<S>, String> {
    host::with_profile(backend, |host, scope| {
        let program = host.compile_task(source, entry)?;
        let arguments = leselang_vm::embedding::bind_entry_arguments(
            &program.entry_parameters,
            arguments,
            STACK.effects.pure.max_bindings,
        )
        .map_err(|error| error.to_string())?;
        let bindings = program
            .entry_parameters
            .iter()
            .zip(arguments)
            .map(|(parameter, (_, value))| (parameter.name.as_str(), PureValue::Scalar(value)))
            .collect();
        drive_node(host, scope, &program.node, bindings, fuel, None)
    })
    .map_err(|e| e.to_string())?
}

pub(crate) fn run_profile_with_waits<S: ProfileSnapshot>(
    backend: &dyn Profile<Snapshot = S>,
    source: &str,
    fuel: u64,
    clock: &mut impl FnMut() -> Duration,
    wait_until: &mut impl FnMut(Duration),
) -> Result<ProfileRunSummary<S>, String> {
    host::with_profile_waits(backend, |host, scope| {
        drive(host, scope, source, fuel, Some((clock, wait_until)))
    })
    .map_err(|e| e.to_string())?
}

pub(crate) fn run_profile_with_state_waits<S: ProfileSnapshot>(
    backend: &dyn Profile<Snapshot = S>,
    source: &str,
    fuel: u64,
    clock: &mut impl FnMut() -> Duration,
    wait_until: &mut impl FnMut(Duration),
) -> Result<ProfileRunSummary<S>, String> {
    host::with_profile_state_waits(backend, |host, scope| {
        drive(host, scope, source, fuel, Some((clock, wait_until)))
    })
    .map_err(|e| e.to_string())?
}

type WaitCallbacks<'a> = (&'a mut dyn FnMut() -> Duration, &'a mut dyn FnMut(Duration));

fn drive<S: ProfileSnapshot>(
    host: &Host<'_, '_, S>,
    scope: LocalExecutionScope,
    source: &str,
    budget: u64,
    waiting: Option<WaitCallbacks<'_>>,
) -> Result<ProfileRunSummary<S>, String> {
    let node = host.compile(source)?;
    drive_node(host, scope, &node, Vec::new(), budget, waiting)
}

fn drive_node<'ir, S: ProfileSnapshot>(
    host: &Host<'_, '_, S>,
    scope: LocalExecutionScope,
    node: &'ir Node,
    bindings: RestoredEffectBindings<'ir, ProfileNativeValue<S>>,
    budget: u64,
    mut waiting: Option<WaitCallbacks<'_>>,
) -> Result<ProfileRunSummary<S>, String> {
    let mut vm = LocalVm::new(scope, node, bindings, Fuel::new(budget), STACK);
    let mut effects = 0;
    let mut observations = Vec::new();
    loop {
        match vm
            .poll(host)
            .map_err(|e| format!("language execution failed: {e:?}"))?
        {
            EffectSessionPoll::Request(work) => {
                effects += 1;
                let operation = work.operation();
                println!("poll -> authorize -> dispatch: {operation}");
                let receipt = if work.is_source_wait() {
                    let (clock, wait_until) = waiting.as_mut().ok_or(
                        "source wait requires explicit host clock and scheduling callbacks",
                    )?;
                    let mut now = || clock();
                    let mut progress =
                        observation_wait::begin_source(&mut vm, host, work, &mut now)
                            .map_err(|error| error.to_string())?;
                    loop {
                        match progress {
                            WaitStep::Ready(receipt) => break receipt,
                            WaitStep::Waiting(wait) => {
                                let status = wait.status();
                                println!(
                                    "wait: attempts={}, next_at={:?}, deadline={:?}",
                                    status.attempts, status.next_at, status.deadline
                                );
                                wait_until(status.next_at);
                                progress = observation_wait::step(&mut vm, host, wait, &mut now)
                                    .map_err(|error| error.to_string())?;
                            }
                        }
                    }
                } else {
                    dispatch(&mut vm, host, work).map_err(|e| e.to_string())?
                };
                if let ProfileReply::Snapshot(snapshot) = &receipt.reply {
                    println!(
                        "observe: {}, revision={}",
                        snapshot.data().describe(),
                        snapshot.revision()
                    );
                    observations.push(snapshot.data().clone());
                }
                let receipt = if receipt.reply.is_submission() {
                    println!("submitted: native work remains queued; explicit pump");
                    pump(&mut vm, host, receipt).map_err(|e| e.to_string())?
                } else {
                    receipt
                };
                accept(&mut vm, host, receipt)?;
                println!("accept: {operation}");
            }
            EffectSessionPoll::Value(PureValue::Scalar(value)) => {
                return Ok(ProfileRunSummary {
                    value,
                    effects,
                    fuel_remaining: vm.fuel_remaining(),
                    observations,
                });
            }
            EffectSessionPoll::Value(PureValue::Result(_)) => {
                return Err("program returned a session-bound native value; project scalar fields before returning".into());
            }
            _ => return Err("unexpected VM session boundary".into()),
        }
    }
}
