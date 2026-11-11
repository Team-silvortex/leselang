//! Reference native adapters used by the conformance suite and runnable example.
//! GUI and CLI models have different layouts; neither invokes an OS toolkit/process.
#![allow(dead_code)]

use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    fmt,
    rc::Rc,
};

use call_evaluation::{
    CallEvaluationError, CallEvaluationLimits, PreparedValueCall, ValueCallEvaluationError,
    evaluate_value_call_arguments_in_scope,
};
use catalog_program::{CatalogProgramLimits, compile_catalog_program};
use catalog_source::{CatalogSourceEnvironment, CatalogSourceLimits};
use control_source::ControlSourceLimits;
use effect_evaluation::{EffectEvaluationEnvironment, EffectEvaluationLimits};
use effect_reentry::{EffectReentryEnvironment, RestoredEffectBindings};
use effect_session::{
    CorrelatedEffectRequest, EffectCorrelation, EffectSessionEnvironment, EffectSessionPoll,
};
use effect_stack::EffectStackLimits;
use flow_typing::CallFlowEnvironment;
use host_protocol::{LocalObjectOwner, ObjectError, ObjectRef, Observation};
use leselang_vm::native::{HostWorkOutcome, LocalVm, core::*, hir::*, syntax::parse};
use pure_evaluation::{
    PureEvaluationEnvironment, PureEvaluationLimits, PureValue, scalar_copy_cost,
};
use pure_typing::{PureType, PureTypeEnvironment};
use source_call::{SourceCallHost, SourceCallLimits};

pub const PROGRAM: &str = r#"
fn main() = bind(
    target: object.find(name: "save"),
    body: bind(
        before: object.observe(target: target),
        body: bind(
            submitted: object.apply(target: target, expected_revision: field(value: before, name: "revision"), text: "ready"),
            body: bind(after: object.observe(target: target), body: field(value: after, name: "text")),
        ),
    ),
)
"#;
pub const STACK: EffectStackLimits = EffectStackLimits {
    effects: EffectEvaluationLimits {
        pure: PureEvaluationLimits {
            max_nodes: 256,
            max_depth: 32,
            max_bindings: 8,
        },
        max_arguments: 8,
        max_branches: 8,
    },
    max_frames: 8,
};
const COMPILATION: CatalogProgramLimits = CatalogProgramLimits {
    expression: CatalogSourceLimits {
        control: ControlSourceLimits {
            source: SourceCallLimits {
                max_source_nodes: 256,
                max_source_depth: 32,
                max_lowered_nodes: 256,
                max_lowered_depth: 32,
                max_arguments: 8,
            },
            max_bindings: 8,
        },
        max_calls: 8,
    },
    max_functions: 8,
    max_parameters: 8,
    max_compilation_nodes: 1024,
    max_expansion_depth: 16,
};

#[derive(Debug)]
pub enum Error {
    Object(ObjectError),
    Native(&'static str),
    Uncertain(&'static str),
    Fuel,
}
impl From<ObjectError> for Error {
    fn from(error: ObjectError) -> Self {
        Self::Object(error)
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Object(error) => error.fmt(f),
            Self::Native(message) | Self::Uncertain(message) => f.write_str(message),
            Self::Fuel => f.write_str("host fuel exhausted"),
        }
    }
}
impl std::error::Error for Error {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FailureMode {
    None,
    Before,
    AfterCommit,
    Uncertain,
    PanicAfterCommit,
}
pub struct PendingUpdate {
    object: ObjectRef,
    lease: LocalExecutionLease,
    revision: u64,
    text: String,
}

pub trait Snapshot {
    fn text(&self) -> Result<&str, Error>;
}

/// Host layouts and event queues are adapted explicitly, not reflected by the VM.
pub trait Adapter: Default {
    type Snapshot: Snapshot;
    fn owner(&self) -> &LocalObjectOwner;
    fn snapshot(&self) -> Self::Snapshot;
    fn text(&self) -> String;
    fn text_len(&self) -> usize;
    fn enqueue(&self, update: PendingUpdate) -> Result<(), Error>;
    fn take_update(&self, lease: &LocalExecutionLease) -> Result<Option<PendingUpdate>, Error>;
    fn commit(&self, text: String);
    fn failure(&self) -> FailureMode;
    fn set_failure(&self, failure: FailureMode);
    fn pump_failure(&self) -> FailureMode;
    fn set_pump_failure(&self, failure: FailureMode);
    fn observed(&self);
    fn submitted(&self);
    fn pumped(&self);
    fn counts(&self) -> (u64, u64, u64);
    fn external_change(&self, text: &str) {
        self.owner().advance_revision().unwrap();
        self.commit(text.to_owned());
    }
}

pub struct GuiSnapshot {
    caption: String,
    visible: bool,
}
impl Snapshot for GuiSnapshot {
    fn text(&self) -> Result<&str, Error> {
        Ok(&self.caption)
    }
}
pub struct GuiAdapter {
    object: LocalObjectOwner,
    widget: RefCell<GuiSnapshot>,
    events: RefCell<VecDeque<PendingUpdate>>,
    failure: Cell<FailureMode>,
    pump_failure: Cell<FailureMode>,
    observations: Cell<u64>,
    submissions: Cell<u64>,
    pumps: Cell<u64>,
}
impl Default for GuiAdapter {
    fn default() -> Self {
        Self {
            object: LocalObjectOwner::new(),
            widget: RefCell::new(GuiSnapshot {
                caption: "draft".into(),
                visible: true,
            }),
            events: RefCell::new(VecDeque::new()),
            failure: Cell::new(FailureMode::None),
            pump_failure: Cell::new(FailureMode::None),
            observations: Cell::new(0),
            submissions: Cell::new(0),
            pumps: Cell::new(0),
        }
    }
}
impl Adapter for GuiAdapter {
    type Snapshot = GuiSnapshot;
    fn owner(&self) -> &LocalObjectOwner {
        &self.object
    }
    fn snapshot(&self) -> GuiSnapshot {
        let widget = self.widget.borrow();
        GuiSnapshot {
            caption: widget.caption.clone(),
            visible: widget.visible,
        }
    }
    fn text(&self) -> String {
        self.widget.borrow().caption.clone()
    }
    fn enqueue(&self, update: PendingUpdate) -> Result<(), Error> {
        let mut queue = self.events.borrow_mut();
        if !queue.is_empty() {
            return Err(Error::Native("widget queue is full"));
        }
        queue.push_back(update);
        Ok(())
    }
    fn text_len(&self) -> usize {
        self.widget.borrow().caption.len()
    }
    fn take_update(&self, lease: &LocalExecutionLease) -> Result<Option<PendingUpdate>, Error> {
        let mut events = self.events.borrow_mut();
        if events
            .front()
            .is_some_and(|update| !update.lease.same_scope(lease))
        {
            return Err(Error::Object(ObjectError::WrongExecution));
        }
        Ok(events.pop_front())
    }
    fn commit(&self, text: String) {
        self.widget.borrow_mut().caption = text;
    }
    fn failure(&self) -> FailureMode {
        self.failure.get()
    }
    fn set_failure(&self, failure: FailureMode) {
        self.failure.set(failure);
    }
    fn pump_failure(&self) -> FailureMode {
        self.pump_failure.get()
    }
    fn set_pump_failure(&self, failure: FailureMode) {
        self.pump_failure.set(failure);
    }
    fn observed(&self) {
        self.observations.set(self.observations.get() + 1);
    }
    fn submitted(&self) {
        self.submissions.set(self.submissions.get() + 1);
    }
    fn pumped(&self) {
        self.pumps.set(self.pumps.get() + 1);
    }
    fn counts(&self) -> (u64, u64, u64) {
        (
            self.observations.get(),
            self.submissions.get(),
            self.pumps.get(),
        )
    }
}

pub struct CliSnapshot {
    stdout: Vec<u8>,
    running: bool,
    exit_status: Option<i32>,
}
impl Snapshot for CliSnapshot {
    fn text(&self) -> Result<&str, Error> {
        if self.running && self.exit_status.is_some() {
            return Err(Error::Native("inconsistent process snapshot"));
        }
        std::str::from_utf8(&self.stdout)
            .map_err(|_| Error::Native("process snapshot is not UTF-8"))
    }
}
struct ProcessModel {
    output: Vec<u8>,
    running: bool,
    exit_status: Option<i32>,
    input: Option<PendingUpdate>,
}
pub struct CliAdapter {
    object: LocalObjectOwner,
    process: RefCell<ProcessModel>,
    failure: Cell<FailureMode>,
    pump_failure: Cell<FailureMode>,
    observations: Cell<u64>,
    submissions: Cell<u64>,
    pumps: Cell<u64>,
}
impl Default for CliAdapter {
    fn default() -> Self {
        Self {
            object: LocalObjectOwner::new(),
            process: RefCell::new(ProcessModel {
                output: b"draft".to_vec(),
                running: true,
                exit_status: None,
                input: None,
            }),
            failure: Cell::new(FailureMode::None),
            pump_failure: Cell::new(FailureMode::None),
            observations: Cell::new(0),
            submissions: Cell::new(0),
            pumps: Cell::new(0),
        }
    }
}
impl Adapter for CliAdapter {
    type Snapshot = CliSnapshot;
    fn owner(&self) -> &LocalObjectOwner {
        &self.object
    }
    fn snapshot(&self) -> CliSnapshot {
        let process = self.process.borrow();
        CliSnapshot {
            stdout: process.output.clone(),
            running: process.running,
            exit_status: process.exit_status,
        }
    }
    fn text(&self) -> String {
        String::from_utf8(self.process.borrow().output.clone()).unwrap()
    }
    fn enqueue(&self, update: PendingUpdate) -> Result<(), Error> {
        let mut process = self.process.borrow_mut();
        if !process.running || process.input.is_some() {
            return Err(Error::Native("process input unavailable"));
        }
        process.input = Some(update);
        Ok(())
    }
    fn text_len(&self) -> usize {
        self.process.borrow().output.len()
    }
    fn take_update(&self, lease: &LocalExecutionLease) -> Result<Option<PendingUpdate>, Error> {
        let mut process = self.process.borrow_mut();
        if process
            .input
            .as_ref()
            .is_some_and(|update| !update.lease.same_scope(lease))
        {
            return Err(Error::Object(ObjectError::WrongExecution));
        }
        Ok(process.input.take())
    }
    fn commit(&self, text: String) {
        self.process.borrow_mut().output = text.into_bytes();
    }
    fn failure(&self) -> FailureMode {
        self.failure.get()
    }
    fn set_failure(&self, failure: FailureMode) {
        self.failure.set(failure);
    }
    fn pump_failure(&self) -> FailureMode {
        self.pump_failure.get()
    }
    fn set_pump_failure(&self, failure: FailureMode) {
        self.pump_failure.set(failure);
    }
    fn observed(&self) {
        self.observations.set(self.observations.get() + 1);
    }
    fn submitted(&self) {
        self.submissions.set(self.submissions.get() + 1);
    }
    fn pumped(&self) {
        self.pumps.set(self.pumps.get() + 1);
    }
    fn counts(&self) -> (u64, u64, u64) {
        (
            self.observations.get(),
            self.submissions.get(),
            self.pumps.get(),
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Field {
    Revision,
    Text,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NativeType {
    Object,
    Snapshot,
}
#[derive(Clone, Copy)]
enum ResultKind {
    Object,
    Snapshot,
    Submitted,
}
pub enum Reply<S> {
    Object(ObjectRef),
    Snapshot(Rc<Observation<S>>),
    Submitted(bool),
}
impl<S> fmt::Debug for Reply<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Object(_) => "Object",
            Self::Snapshot(_) => "Snapshot",
            Self::Submitted(_) => "Submitted",
        })
    }
}
pub struct Domain<'a> {
    owners: [&'a LocalObjectOwner; 2],
    lease: LocalExecutionLease,
    kind: ResultKind,
    expected: RefCell<Option<ObjectRef>>,
}
impl Domain<'_> {
    // A host owns one session and at most one outstanding request. Its catalog
    // declaration stays fixed while value admission follows that exact request.
    fn validate_object(&self, actual: &ObjectRef) -> Result<(), Error> {
        let expected = self.expected.borrow();
        let expected = expected
            .as_ref()
            .ok_or(Error::Native("no discovered or requested target"))?;
        for owner in self.owners {
            match owner.validate(expected, &self.lease) {
                Err(ObjectError::ForeignOwner) => continue,
                result => {
                    result?;
                    return owner.validate(actual, &self.lease).map_err(Error::from);
                }
            }
        }
        Err(ObjectError::ForeignOwner.into())
    }
    pub fn result_name(&self) -> &'static str {
        match self.kind {
            ResultKind::Object => "object",
            ResultKind::Snapshot => "observation",
            ResultKind::Submitted => "submission",
        }
    }
}
impl<S: Snapshot> HostResultDomain<Reply<S>> for Domain<'_> {
    type Error = Error;
    fn matches_type(&self, reply: &Reply<S>) -> bool {
        matches!(
            (self.kind, reply),
            (ResultKind::Object, Reply::Object(_))
                | (ResultKind::Snapshot, Reply::Snapshot(_))
                | (ResultKind::Submitted, Reply::Submitted(_))
        )
    }
    fn validate_value(&self, reply: &Reply<S>) -> Result<(), Error> {
        match (self.kind, reply) {
            (ResultKind::Object, Reply::Object(object)) => self.validate_object(object),
            (ResultKind::Snapshot, Reply::Snapshot(observed)) => {
                self.validate_object(observed.object())?;
                if observed.data().text()?.len() > MAX_SCALAR_STRING_BYTES {
                    return Err(Error::Native("snapshot text exceeds bounds"));
                }
                Ok(())
            }
            (ResultKind::Submitted, Reply::Submitted(true)) => Ok(()),
            _ => Err(Error::Native(
                "reply does not match the original native result domain",
            )),
        }
    }
}
// Native identities stay in the Result branch; copying only aliases bounded metadata.
pub enum NativeValue<S> {
    Object(ObjectRef),
    Snapshot(Rc<Observation<S>>),
}
impl<S> Clone for NativeValue<S> {
    fn clone(&self) -> Self {
        match self {
            Self::Object(value) => Self::Object(value.clone()),
            Self::Snapshot(value) => Self::Snapshot(value.clone()),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ArgumentDomain {
    Scalar(ScalarTypeSet),
    Object,
}
impl ScalarArgumentDomain for ArgumentDomain {
    fn scalar_types(&self) -> ScalarTypeSet {
        match self {
            Self::Scalar(domain) => *domain,
            Self::Object => ScalarTypeSet::empty(),
        }
    }
    fn accepts_literal(&self, value: &ScalarValue) -> bool {
        matches!(self, Self::Scalar(domain) if domain.accepts_literal(value))
    }
}
pub type Node = ir::Computation<Field, &'static str, (), ()>;
type Schema<'s> = OperationSchema<'s, &'static str, &'s str, ArgumentDomain, Domain<'s>, u8>;
type Catalog<'s> = OperationCatalog<'s, &'static str, &'s str, ArgumentDomain, Domain<'s>, u8>;
type Prepared<'s, S> =
    PreparedValueCall<'s, &'static str, ArgumentDomain, Domain<'s>, u8, NativeValue<S>>;

#[derive(Clone)]
pub struct Identity {
    lease: LocalExecutionLease,
    serial: u64,
    operation: &'static str,
    target: Option<ObjectRef>,
}
impl PartialEq for Identity {
    fn eq(&self, other: &Self) -> bool {
        self.lease.same_scope(&other.lease)
            && self.serial == other.serial
            && self.operation == other.operation
    }
}
pub struct Work<'s, S> {
    identity: Identity,
    call: Prepared<'s, S>,
}
impl<S> fmt::Debug for Work<'_, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Work")
    }
}
impl<S> Work<'_, S> {
    pub fn identity(&self) -> &Identity {
        &self.identity
    }
    pub fn operation(&self) -> &'static str {
        self.identity.operation
    }
}
pub struct Receipt<S> {
    pub identity: Identity,
    pub reply: Reply<S>,
}
impl<S> fmt::Debug for Receipt<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.reply.fmt(f)
    }
}
pub struct ProtocolHost<'h, 's, A: Adapter> {
    pub adapter: &'h A,
    other: &'h A,
    catalog: &'h Catalog<'s>,
    lease: LocalExecutionLease,
    granted: Cell<bool>,
    serial: Cell<u64>,
    dispatched: Cell<u64>,
}

pub fn with_host<A: Adapter, T>(
    adapter: &A,
    use_host: impl FnOnce(&ProtocolHost<'_, '_, A>, LocalExecutionScope) -> T,
) -> T {
    with_targets(adapter, &A::default(), use_host)
}

/// Two independent host-owned resources selected by a bounded discovery operation.
pub fn with_targets<A: Adapter, T>(
    adapter: &A,
    other: &A,
    use_host: impl FnOnce(&ProtocolHost<'_, '_, A>, LocalExecutionScope) -> T,
) -> T {
    let scope = LocalExecutionScope::new();
    let lease = scope.lease();
    let find_parameters = [NamedParameter::required(
        "name",
        ArgumentDomain::Scalar(ScalarTypeSet::only(ScalarType::String)),
    )];
    let observe_parameters = [NamedParameter::required("target", ArgumentDomain::Object)];
    let parameters = [
        NamedParameter::required("target", ArgumentDomain::Object),
        NamedParameter::required(
            "expected_revision",
            ArgumentDomain::Scalar(ScalarTypeSet::only(ScalarType::Integer)),
        ),
        NamedParameter::required(
            "text",
            ArgumentDomain::Scalar(ScalarTypeSet::only(ScalarType::String)),
        ),
    ];
    let domain = |kind| Domain {
        owners: [adapter.owner(), other.owner()],
        lease: lease.clone(),
        kind,
        expected: RefCell::new(None),
    };
    let schemas = [
        Schema {
            key: "object.find",
            parameters: &find_parameters,
            required_capability: 1,
            result: domain(ResultKind::Object),
        },
        Schema {
            key: "object.observe",
            parameters: &observe_parameters,
            required_capability: 1,
            result: domain(ResultKind::Snapshot),
        },
        Schema {
            key: "object.apply",
            parameters: &parameters,
            required_capability: 2,
            result: domain(ResultKind::Submitted),
        },
    ];
    let catalog = OperationCatalog::new(
        1,
        &schemas,
        OperationCatalogLimits {
            max_operations: 3,
            max_parameters_per_operation: 3,
        },
    )
    .unwrap();
    let host = ProtocolHost {
        adapter,
        other,
        catalog: &catalog,
        lease,
        granted: Cell::new(true),
        serial: Cell::new(0),
        dispatched: Cell::new(0),
    };
    use_host(&host, scope)
}

impl<'s, A: Adapter> ProtocolHost<'_, 's, A> {
    pub fn catalog(&self) -> &Catalog<'s> {
        self.catalog
    }
    pub fn target(&self) -> ObjectRef {
        self.adapter.owner().reference(&self.lease).unwrap()
    }
    fn resolve(&self, target: &ObjectRef) -> Result<&A, Error> {
        for adapter in [self.adapter, self.other] {
            match adapter.owner().validate(target, &self.lease) {
                Err(ObjectError::ForeignOwner) => continue,
                result => {
                    result?;
                    return Ok(adapter);
                }
            }
        }
        Err(ObjectError::ForeignOwner.into())
    }
    pub fn lease(&self) -> LocalExecutionLease {
        self.lease.clone()
    }
    pub fn set_granted(&self, granted: bool) {
        self.granted.set(granted);
    }
    fn source(&self) -> SourceCallHost<'_, 's, &'static str, ArgumentDomain, Domain<'s>, u8> {
        SourceCallHost {
            catalog: self.catalog,
            version: 1,
            granted: if self.granted.get() { &[1, 2] } else { &[] },
        }
    }
    fn schema(&self, operation: &'static str) -> Result<&'s Schema<'s>, Error> {
        self.catalog
            .authorize(&operation, 1, self.source().granted)
            .map_err(|_| Error::Native("operation denied"))
    }
    pub fn compile(&self, source: &str) -> Result<Node, String> {
        if source.len() > 16 * 1024 {
            return Err("source limit".into());
        }
        let parsed = parse(source);
        if !parsed.diagnostics.is_empty() {
            return Err("syntax rejected".into());
        }
        let declarations: Vec<_> = parsed.function.iter().chain(&parsed.helpers).collect();
        compile_catalog_program::<Field, _, (), (), _, _, _, _>(
            &declarations,
            "main",
            COMPILATION,
            &self.source(),
            self,
        )
        .map(|(node, _)| node)
        .map_err(|error| error.to_string())
    }
    /// The request is still checked here, even if the driver already authorized it.
    pub fn dispatch(
        &self,
        work: Work<'s, A::Snapshot>,
        fuel: &mut Fuel,
    ) -> HostWorkOutcome<Result<Receipt<A::Snapshot>, Error>> {
        match self.invoke(work, fuel) {
            Ok(receipt) => HostWorkOutcome::Pending(Ok(receipt)),
            Err(error @ Error::Uncertain(_)) => HostWorkOutcome::Uncertain(Err(error)),
            Err(error) => HostWorkOutcome::Failed(Err(error)),
        }
    }
    fn invoke(
        &self,
        work: Work<'s, A::Snapshot>,
        fuel: &mut Fuel,
    ) -> Result<Receipt<A::Snapshot>, Error> {
        self.authorize(&work.identity)?;
        if self.dispatched.get() == work.identity.serial
            || !std::ptr::eq(self.schema(work.operation())?, work.call.schema())
        {
            return Err(Error::Native("stale or foreign dispatch"));
        }
        self.dispatched.set(work.identity.serial);
        let operation = work.operation();
        let target = work.identity.target.as_ref();
        let adapter = target.map(|target| self.resolve(target)).transpose()?;
        let reply = match operation {
            "object.find" => {
                let Some(PureValue::Scalar(ScalarValue::String(name))) =
                    work.call.arguments().first().map(|arg| &arg.value)
                else {
                    return Err(Error::Native("missing selector"));
                };
                fuel.charge(1 + name.len() as u64)
                    .map_err(|_| Error::Fuel)?;
                let selected = match name.as_str() {
                    "save" => self.adapter,
                    "other" => self.other,
                    _ => return Err(Error::Native("object not found")),
                };
                let object = selected.owner().reference(&self.lease)?;
                *work.call.schema().result.expected.borrow_mut() = Some(object.clone());
                Reply::Object(object)
            }
            "object.observe" => {
                let adapter = adapter.ok_or(Error::Native("missing target"))?;
                let target = target.ok_or(Error::Native("missing target"))?;
                let bytes = adapter.text_len();
                if bytes > MAX_SCALAR_STRING_BYTES {
                    return Err(Error::Native("snapshot text exceeds bounds"));
                }
                fuel.charge(1 + bytes as u64).map_err(|_| Error::Fuel)?;
                let observed = adapter
                    .owner()
                    .observe(target, &self.lease, adapter.snapshot())?;
                adapter.observed();
                Reply::Snapshot(Rc::new(observed))
            }
            "object.apply" => {
                let adapter = adapter.ok_or(Error::Native("missing target"))?;
                let target = target.ok_or(Error::Native("missing target"))?;
                let (_, arguments) = work.call.into_parts();
                let mut arguments = arguments.into_iter().skip(1);
                let Some(PureValue::Scalar(ScalarValue::Integer(expected))) =
                    arguments.next().map(|arg| arg.value)
                else {
                    return Err(Error::Native("missing revision"));
                };
                let Some(PureValue::Scalar(ScalarValue::String(text))) =
                    arguments.next().map(|arg| arg.value)
                else {
                    return Err(Error::Native("missing text"));
                };
                fuel.charge(1 + text.len() as u64)
                    .map_err(|_| Error::Fuel)?;
                let attempt = adapter.owner().with_current(
                    target,
                    &self.lease,
                    expected,
                    || match adapter.failure() {
                        FailureMode::Before => Err(Error::Native("action refused before mutation")),
                        mode @ (FailureMode::AfterCommit
                        | FailureMode::Uncertain
                        | FailureMode::PanicAfterCommit) => {
                            adapter.commit(text);
                            match mode {
                                FailureMode::PanicAfterCommit => {
                                    panic!("action panic after commit")
                                }
                                FailureMode::Uncertain => Err(Error::Uncertain(
                                    "action delivery uncertain after native mutation",
                                )),
                                _ => Err(Error::Native("action failed after native mutation")),
                            }
                        }
                        FailureMode::None => {
                            adapter.enqueue(PendingUpdate {
                                object: target.clone(),
                                lease: self.lease.clone(),
                                revision: adapter.owner().revision(),
                                text,
                            })?;
                            adapter.submitted();
                            Ok(())
                        }
                    },
                )?;
                attempt.into_output()?;
                // Submitted acknowledges queue admission, never observed state.
                Reply::Submitted(true)
            }
            _ => return Err(Error::Native("unknown operation")),
        };
        Ok(Receipt {
            identity: work.identity,
            reply,
        })
    }
    /// One explicit native event-loop step; it never polls or resumes the language.
    /// Queued work is single-use, and actual mutation rechecks live authority/revision.
    pub fn pump(
        &self,
        receipt: Receipt<A::Snapshot>,
        fuel: &mut Fuel,
    ) -> HostWorkOutcome<Result<Receipt<A::Snapshot>, Error>> {
        let result = (|| {
            self.authorize(&receipt.identity)?;
            if receipt.identity.operation != "object.apply"
                || !matches!(receipt.reply, Reply::Submitted(true))
            {
                return Err(Error::Native(
                    "pump requires the original submission receipt",
                ));
            }
            let target = receipt
                .identity
                .target
                .as_ref()
                .ok_or(Error::Native("missing target"))?;
            let adapter = self.resolve(target)?;
            let update = adapter
                .take_update(&self.lease)?
                .ok_or(Error::Native("no pending update"))?;
            fuel.charge(1 + update.text.len() as u64)
                .map_err(|_| Error::Fuel)?;
            let attempt = adapter.owner().with_current(
                &update.object,
                &self.lease,
                update.revision,
                || {
                    let mode = adapter.pump_failure();
                    if mode == FailureMode::Before {
                        return Err(Error::Native("pump refused before mutation"));
                    }
                    adapter.commit(update.text);
                    adapter.pumped();
                    match mode {
                        FailureMode::None => Ok(()),
                        FailureMode::AfterCommit => {
                            Err(Error::Native("pump failed after mutation"))
                        }
                        FailureMode::Uncertain => {
                            Err(Error::Uncertain("pump delivery uncertain after mutation"))
                        }
                        FailureMode::PanicAfterCommit => panic!("pump panic after commit"),
                        FailureMode::Before => unreachable!(),
                    }
                },
            )?;
            attempt.into_output()?;
            Ok(receipt)
        })();
        match result {
            Ok(receipt) => HostWorkOutcome::Pending(Ok(receipt)),
            Err(error @ Error::Uncertain(_)) => HostWorkOutcome::Uncertain(Err(error)),
            Err(error) => HostWorkOutcome::Failed(Err(error)),
        }
    }
}
impl<A: Adapter> ReplyAuthority<Identity> for ProtocolHost<'_, '_, A> {
    type Error = Error;
    fn authorize(&self, identity: &Identity) -> Result<(), Error> {
        if !self.lease.is_open()
            || !self.lease.same_scope(&identity.lease)
            || identity.serial == 0
            || identity.serial != self.serial.get()
        {
            return Err(Error::Native("foreign or closed execution"));
        }
        self.schema(identity.operation)?;
        if let Some(target) = &identity.target {
            self.resolve(target)?;
        }
        Ok(())
    }
}
impl<A: Adapter> PureTypeEnvironment<Field, &'static str> for ProtocolHost<'_, '_, A> {
    type Result = NativeType;
    fn field_type(&self, result: &NativeType, field: &Field) -> Option<ScalarType> {
        if *result != NativeType::Snapshot {
            return None;
        }
        Some(match field {
            Field::Revision => ScalarType::Integer,
            Field::Text => ScalarType::String,
        })
    }
    fn member_result(&self, _: &NativeType, _: &str, _: &&'static str) -> Option<NativeType> {
        None
    }
}
impl<'s, A: Adapter> CallFlowEnvironment<'s, Field, &'static str, Domain<'s>>
    for ProtocolHost<'_, 's, A>
{
    fn call_native_argument_type(
        &self,
        operation: &&'static str,
        parameter_name: &str,
        result: &NativeType,
    ) -> bool {
        matches!(*operation, "object.observe" | "object.apply")
            && parameter_name == "target"
            && *result == NativeType::Object
    }
    fn call_result_type(
        &self,
        _: &&'static str,
        domain: &'s Domain<'s>,
    ) -> Option<PureType<NativeType>> {
        Some(match domain.kind {
            ResultKind::Object => PureType::Result(NativeType::Object),
            ResultKind::Snapshot => PureType::Result(NativeType::Snapshot),
            ResultKind::Submitted => PureType::Scalar(ScalarType::Boolean),
        })
    }
}
impl<'source, 's, A: Adapter> CatalogSourceEnvironment<'source, 's, Field, &'static str, Domain<'s>>
    for ProtocolHost<'_, 's, A>
{
    type Error = Error;
    fn source_operation(&self, operation: &&'static str) -> Result<&'static str, Error> {
        Ok(*operation)
    }
    fn source_field(
        &self,
        result: &NativeType,
        name: &'source str,
    ) -> Result<Option<(Field, ScalarType)>, Error> {
        if *result != NativeType::Snapshot {
            return Ok(None);
        }
        Ok(match name {
            "revision" => Some((Field::Revision, ScalarType::Integer)),
            "text" => Some((Field::Text, ScalarType::String)),
            _ => None,
        })
    }
    fn source_member(
        &self,
        _: &NativeType,
        _: &'source str,
        _: &'source str,
    ) -> Result<Option<(&'static str, NativeType)>, Error> {
        Ok(None)
    }
}
impl<A: Adapter> PureEvaluationEnvironment<Field, &'static str> for ProtocolHost<'_, '_, A> {
    type Result = NativeValue<A::Snapshot>;
    type Error = Error;
    fn field(&self, observed: &Self::Result, field: &Field) -> Result<ScalarValue, Error> {
        let NativeValue::Snapshot(observed) = observed else {
            return Err(Error::Native("object has no scalar fields"));
        };
        self.resolve(observed.object())?;
        Ok(match field {
            Field::Revision => ScalarValue::Integer(observed.revision()),
            Field::Text => ScalarValue::String(observed.data().text()?.to_owned()),
        })
    }
    fn member(&self, _: &Self::Result, _: &str, _: &&'static str) -> Result<Self::Result, Error> {
        Err(Error::Native("no native groups"))
    }
}
impl<'ir, 's, A: Adapter> EffectEvaluationEnvironment<'ir, Field, &'static str, (), ()>
    for ProtocolHost<'_, 's, A>
{
    type Request = Prepared<'s, A::Snapshot>;
    type Capture = RestoredEffectBindings<'ir, NativeValue<A::Snapshot>>;
    fn preflight_effect(&self, node: &'ir Node) -> Result<(), Error> {
        let Node::Call {
            operation,
            arguments,
        } = node
        else {
            return Err(Error::Native("only atomic operations"));
        };
        self.schema(operation)?
            .bind_arguments(
                &arguments
                    .iter()
                    .map(|arg| arg.name.as_str())
                    .collect::<Vec<_>>(),
            )
            .map_err(|_| Error::Native("invalid call signature"))?;
        Ok(())
    }
    fn prepare_effect(
        &self,
        node: &'ir Node,
        scope: &mut ScopeFrame<'_, 'ir, PureValue<Self::Result>>,
        fuel: &mut Fuel,
    ) -> Result<Self::Request, CalculationFailure<Error>> {
        let Node::Call {
            operation,
            arguments,
        } = node
        else {
            return Err(Error::Native("only atomic operations").into());
        };
        evaluate_value_call_arguments_in_scope(
            arguments,
            self.schema(operation)?,
            scope,
            self,
            fuel,
            CallEvaluationLimits {
                pure: STACK.effects.pure,
                max_arguments: STACK.effects.max_arguments,
            },
            |domain, value, _fuel| match (domain, value) {
                (ArgumentDomain::Object, PureValue::Result(NativeValue::Object(target))) => {
                    self.resolve(target).map(|_| ())
                }
                (ArgumentDomain::Scalar(domain), PureValue::Scalar(value)) => {
                    check_argument_type(domain, ScalarArgumentType::literal(value))
                        .map_err(|_| Error::Native("scalar argument rejected"))
                }
                _ => Err(Error::Native("native argument type rejected")),
            },
        )
        .map_err(|error| match error {
            ValueCallEvaluationError::Preparation(CallEvaluationError::Evaluation {
                failure: CalculationFailure::Scalar(error),
                ..
            }) => CalculationFailure::Scalar(error),
            ValueCallEvaluationError::Preparation(CallEvaluationError::FuelExhausted) => {
                Error::Fuel.into()
            }
            ValueCallEvaluationError::Argument { error, .. } => error.into(),
            _ => Error::Native("call preparation failed").into(),
        })
    }
    fn capture(
        &self,
        _: &'ir str,
        _: &'ir Node,
        scope: &ScopeFrame<'_, 'ir, PureValue<Self::Result>>,
        fuel: &mut Fuel,
    ) -> Result<Self::Capture, Error> {
        let mut cost = 0_u64;
        for (_, value) in scope.bindings() {
            cost = cost
                .checked_add(match value {
                    PureValue::Scalar(value) => 1 + scalar_copy_cost(value),
                    PureValue::Result(_) => 1,
                })
                .ok_or(Error::Native("capture cost overflow"))?;
        }
        fuel.charge(cost).map_err(|_| Error::Fuel)?;
        Ok(scope.bindings().to_vec())
    }
}
impl<'ir, 's, A: Adapter>
    EffectReentryEnvironment<
        'ir,
        Field,
        &'static str,
        (),
        (),
        Identity,
        Domain<'s>,
        Reply<A::Snapshot>,
    > for ProtocolHost<'_, 's, A>
{
    fn restore_capture(
        &self,
        identity: &Identity,
        _: &Domain<'s>,
        capture: Self::Capture,
        _: &mut Fuel,
        _: EffectEvaluationLimits,
    ) -> Result<Self::Capture, CalculationFailure<Error>> {
        self.authorize(identity)?;
        Ok(capture)
    }
    fn bind_reply(
        &self,
        identity: &Identity,
        domain: &Domain<'s>,
        reply: Reply<A::Snapshot>,
        _: &mut Fuel,
    ) -> Result<PureValue<Self::Result>, CalculationFailure<Error>> {
        self.authorize(identity)?;
        if !std::ptr::eq(domain, &self.schema(identity.operation)?.result) {
            return Err(Error::Native("foreign result domain").into());
        }
        domain.validate_value(&reply)?;
        Ok(match reply {
            Reply::Object(value) => PureValue::Result(NativeValue::Object(value)),
            Reply::Snapshot(value) => PureValue::Result(NativeValue::Snapshot(value)),
            Reply::Submitted(value) => PureValue::Scalar(ScalarValue::Boolean(value)),
        })
    }
}
impl<'ir, 's, A: Adapter> EffectSessionEnvironment<'ir, 's, Field, &'static str, (), ()>
    for ProtocolHost<'_, 's, A>
{
    type Identity = Identity;
    type Declaration = Domain<'s>;
    type Reply = Reply<A::Snapshot>;
    type Dispatch = Work<'s, A::Snapshot>;
    fn correlate_request(
        &self,
        request: Self::Request,
        _: &mut Fuel,
    ) -> EffectCorrelation<'s, Identity, Domain<'s>, Work<'s, A::Snapshot>, Error> {
        let schema = self.schema(request.schema().key)?;
        if !std::ptr::eq(schema, request.schema()) {
            return Err(Error::Native("foreign schema").into());
        }
        let target = if schema.key == "object.find" {
            None
        } else {
            let Some(PureValue::Result(NativeValue::Object(target))) =
                request.arguments().first().map(|arg| &arg.value)
            else {
                return Err(Error::Native("missing target").into());
            };
            self.resolve(target)?;
            Some(target.clone())
        };
        *schema.result.expected.borrow_mut() = target.clone();
        let serial = self
            .serial
            .get()
            .checked_add(1)
            .ok_or(Error::Native("identity exhausted"))?;
        self.serial.set(serial);
        let identity = Identity {
            lease: self.lease.clone(),
            serial,
            operation: schema.key,
            target,
        };
        Ok(CorrelatedEffectRequest {
            identity: identity.clone(),
            declaration: &schema.result,
            dispatch: Work {
                identity,
                call: request,
            },
        })
    }
}

#[derive(Debug, PartialEq)]
pub struct Summary {
    pub text: String,
    pub observations: u64,
    pub submissions: u64,
    pub pumps: u64,
    pub fuel_remaining: u64,
}
pub fn run_reference_gui() -> Result<Summary, String> {
    run_reference(&GuiAdapter::default())
}
pub fn run_reference_cli() -> Result<Summary, String> {
    run_reference(&CliAdapter::default())
}
pub fn run_reference<A: Adapter>(adapter: &A) -> Result<Summary, String> {
    with_host(adapter, |host, scope| {
        let node = host.compile(PROGRAM)?;
        let mut vm = LocalVm::new(scope, &node, Vec::new(), Fuel::new(1000), STACK);
        loop {
            match vm.poll(host).map_err(|_| "language execution failed")? {
                EffectSessionPoll::Request(work) => {
                    host.authorize(work.identity())
                        .map_err(|error| error.to_string())?;
                    let submitted = work.operation() == "object.apply";
                    let receipt = vm
                        .with_metered_host_outcome(work, |work, fuel| host.dispatch(work, fuel))
                        .map_err(|_| "session was not awaiting work")?
                        .map_err(|error| error.to_string())?;
                    let receipt = if submitted {
                        vm.with_metered_host_outcome(receipt, |receipt, fuel| {
                            host.pump(receipt, fuel)
                        })
                        .map_err(|_| "session was not awaiting pump")?
                        .map_err(|error| error.to_string())?
                    } else {
                        receipt
                    };
                    vm.try_accept(&receipt.identity, receipt.reply, host)
                        .map_err(|error| error.error.to_string())?;
                }
                EffectSessionPoll::Value(PureValue::Scalar(ScalarValue::String(text))) => {
                    let (observations, submissions, pumps) = adapter.counts();
                    return Ok(Summary {
                        text,
                        observations,
                        submissions,
                        pumps,
                        fuel_remaining: vm.fuel_remaining(),
                    });
                }
                _ => return Err("unexpected session boundary".into()),
            }
        }
    })
}
