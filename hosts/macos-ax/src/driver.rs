use crate::{
    Backend, Error, Snapshot,
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
        .map_err(|_| Error::Invalid("VM is not awaiting native work"))?
}
pub fn accept(vm: &mut Vm<'_, '_>, host: &Host<'_, '_>, receipt: Receipt) -> Result<(), String> {
    vm.try_accept(&receipt.identity, receipt.reply, host)
        .map_err(|e| e.error.to_string())
}
pub fn pump(vm: &mut Vm<'_, '_>, host: &Host<'_, '_>, receipt: Receipt) -> Result<Receipt, Error> {
    vm.with_metered_host_outcome(receipt, |receipt, fuel| host.pump(receipt, fuel))
        .map_err(|_| Error::Invalid("VM is not awaiting queued work"))?
}

#[derive(Debug)]
pub struct RunSummary {
    pub value: ScalarValue,
    pub effects: usize,
    pub fuel_remaining: u64,
    pub observations: Vec<Snapshot>,
}

/// Execute caller-provided source. Demo postconditions do not constrain programs.
pub fn run(backend: &dyn Backend, source: &str) -> Result<RunSummary, String> {
    host::with_host(backend, |host, scope| {
        let node = host.compile(source)?;
        let mut vm = LocalVm::new(scope, &node, Vec::new(), Fuel::new(1000), STACK);
        let mut effects = 0;
        let mut observations = Vec::new();
        loop {
            match vm.poll(host).map_err(|e| format!("language execution failed: {e:?}"))? {
                EffectSessionPoll::Request(work) => {
                    effects += 1;
                    let operation = work.operation();
                    println!("poll -> authorize -> dispatch: {operation}");
                    let receipt = dispatch(&mut vm, host, work).map_err(|e| e.to_string())?;
                    if let Reply::Snapshot(snapshot) = &receipt.reply {
                        println!("observe: document={:?}, role={:?}, value={:?}, revision={}", snapshot.data().document, snapshot.data().role, snapshot.data().value, snapshot.revision());
                        observations.push(snapshot.data().clone());
                    }
                    let receipt = if matches!(receipt.reply, Reply::Submitted(true)) {
                        println!("submitted: native work remains queued; explicit pump");
                        pump(&mut vm, host, receipt).map_err(|e| e.to_string())?
                    } else { receipt };
                    accept(&mut vm, host, receipt)?;
                    println!("accept: {operation}");
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
