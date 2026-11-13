//! Typed local capabilities for one explicitly scoped remote document.
//! Remote snapshots detect observed changes; they cannot provide atomic compare-and-set
//! or detect an external A→B→A change that occurs entirely between observations.
use crate::{Backend, Error, Snapshot};
use std::{
    cell::{Cell, RefCell},
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
use effect_session::{CorrelatedEffectRequest, EffectCorrelation, EffectSessionEnvironment};
use effect_stack::EffectStackLimits;
use flow_typing::CallFlowEnvironment;
use host_protocol::{LocalObjectOwner, ObjectRef, Observation};
use leselang_vm::native::{HostWorkOutcome, core::*, hir::*, syntax::parse};
use pure_evaluation::{
    PureEvaluationEnvironment, PureEvaluationLimits, PureValue, scalar_copy_cost,
};
use pure_typing::{PureType, PureTypeEnvironment};
use source_call::{SourceCallHost, SourceCallLimits};

pub const PROGRAM: &str = include_str!("../examples/textedit.lese");
pub const INITIAL_TEXT: &str = "LeseLang AX initial\n";
pub const RESULT_TEXT: &str = "LeseLang cross-process ✓\n";
pub const STACK: EffectStackLimits = EffectStackLimits {
    effects: EffectEvaluationLimits {
        pure: PureEvaluationLimits {
            max_nodes: 512,
            max_depth: 64,
            max_bindings: 16,
        },
        max_arguments: 8,
        max_branches: 8,
    },
    max_frames: 16,
};
const COMPILATION: CatalogProgramLimits = CatalogProgramLimits {
    expression: CatalogSourceLimits {
        control: ControlSourceLimits {
            source: SourceCallLimits {
                max_source_nodes: 512,
                max_source_depth: 64,
                max_lowered_nodes: 512,
                max_lowered_depth: 64,
                max_arguments: 8,
            },
            max_bindings: 16,
        },
        max_calls: 16,
    },
    max_functions: 8,
    max_parameters: 8,
    max_compilation_nodes: 1024,
    max_expansion_depth: 16,
};

struct TrackedDocument<'b> {
    backend: &'b dyn Backend,
    owner: LocalObjectOwner,
    previous: RefCell<Snapshot>,
}
impl<'b> TrackedDocument<'b> {
    fn new(backend: &'b dyn Backend) -> Result<Self, Error> {
        backend.check_permission()?;
        let snapshot = backend.snapshot()?;
        validate_snapshot(&snapshot)?;
        Ok(Self {
            backend,
            owner: LocalObjectOwner::new(),
            previous: RefCell::new(snapshot),
        })
    }
    fn refresh(&self) -> Result<Snapshot, Error> {
        let snapshot = match self
            .backend
            .check_permission()
            .and_then(|()| self.backend.snapshot())
            .and_then(|snapshot| {
                validate_snapshot(&snapshot)?;
                Ok(snapshot)
            }) {
            Ok(value) => value,
            Err(error) => {
                self.owner.retire();
                return Err(error);
            }
        };
        let mut previous = self.previous.borrow_mut();
        // Never adopt a replacement into an existing execution, even if it has
        // the same document URL or visible content.
        if previous.identity != snapshot.identity
            || previous.document != snapshot.document
            || previous.role != snapshot.role
        {
            self.owner.retire();
            return Err(Error::Stale);
        }
        if *previous != snapshot
            && let Err(error) = self.owner.advance_revision()
        {
            self.owner.retire();
            return Err(error.into());
        }
        *previous = snapshot.clone();
        Ok(snapshot)
    }
    fn resolve(&self, target: &ObjectRef, lease: &LocalExecutionLease) -> Result<(), Error> {
        self.owner.validate(target, lease)?;
        self.refresh()?;
        self.owner.validate(target, lease)?;
        Ok(())
    }
}
fn validate_snapshot(value: &Snapshot) -> Result<(), Error> {
    if [&value.document, &value.role, &value.value]
        .iter()
        .any(|s| s.len() > MAX_SCALAR_STRING_BYTES)
    {
        return Err(Error::Bounds);
    }
    if value.identity == 0 || value.document.is_empty() || value.role != "AXTextArea" {
        return Err(Error::Invalid("invalid remote snapshot metadata"));
    }
    Ok(())
}
fn snapshot_copy_cost(value: &Snapshot) -> u64 {
    (value.document.len() + value.role.len() + value.value.len()) as u64
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Field {
    Document,
    Role,
    Value,
    Editable,
    Revision,
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
pub enum Reply {
    Object(ObjectRef),
    Snapshot(Rc<Observation<Snapshot>>),
    Submitted(bool),
}
impl fmt::Debug for Reply {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Object(_) => "Object",
            Self::Snapshot(_) => "Snapshot",
            Self::Submitted(_) => "Submitted",
        })
    }
}
pub struct Domain<'a> {
    tracked: &'a TrackedDocument<'a>,
    lease: LocalExecutionLease,
    kind: ResultKind,
    expected: RefCell<Option<ObjectRef>>,
}
impl Domain<'_> {
    fn validate_object(&self, actual: &ObjectRef) -> Result<(), Error> {
        let expected = self.expected.borrow();
        let expected = expected
            .as_ref()
            .ok_or(Error::Invalid("no requested native target"))?;
        self.tracked.resolve(expected, &self.lease)?;
        self.tracked.owner.validate(actual, &self.lease)?;
        Ok(())
    }
}
impl HostResultDomain<Reply> for Domain<'_> {
    type Error = Error;
    fn matches_type(&self, reply: &Reply) -> bool {
        matches!(
            (self.kind, reply),
            (ResultKind::Object, Reply::Object(_))
                | (ResultKind::Snapshot, Reply::Snapshot(_))
                | (ResultKind::Submitted, Reply::Submitted(_))
        )
    }
    fn validate_value(&self, reply: &Reply) -> Result<(), Error> {
        match (self.kind, reply) {
            (ResultKind::Object, Reply::Object(value)) => self.validate_object(value),
            (ResultKind::Snapshot, Reply::Snapshot(value)) => {
                self.validate_object(value.object())?;
                let current = self.tracked.refresh()?;
                if value.data().identity != current.identity
                    || value.data().document != current.document
                    || value.data().role != current.role
                {
                    return Err(Error::Invalid(
                        "snapshot payload belongs to another native instance",
                    ));
                }
                validate_snapshot(value.data())
            }
            (ResultKind::Submitted, Reply::Submitted(true)) => Ok(()),
            _ => Err(Error::Invalid(
                "reply does not match original result domain",
            )),
        }
    }
}
#[derive(Clone)]
pub enum NativeValue {
    Object(ObjectRef),
    Snapshot(Rc<Observation<Snapshot>>),
}
#[derive(Clone, Copy)]
pub enum ArgumentDomain {
    Scalar(ScalarTypeSet),
    Object,
}
impl ScalarArgumentDomain for ArgumentDomain {
    fn scalar_types(&self) -> ScalarTypeSet {
        match self {
            Self::Scalar(value) => *value,
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
type Prepared<'s> =
    PreparedValueCall<'s, &'static str, ArgumentDomain, Domain<'s>, u8, NativeValue>;
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
pub struct Work<'s> {
    identity: Identity,
    call: Prepared<'s>,
}
impl fmt::Debug for Work<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Work")
    }
}
impl Work<'_> {
    pub fn identity(&self) -> &Identity {
        &self.identity
    }
    pub fn operation(&self) -> &'static str {
        self.identity.operation
    }
}
pub struct Receipt {
    pub identity: Identity,
    pub reply: Reply,
}
impl fmt::Debug for Receipt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.reply.fmt(f)
    }
}
struct PendingAction {
    identity: Identity,
    revision: u64,
    expected: Snapshot,
    text: String,
}
pub struct Host<'h, 's> {
    tracked: &'h TrackedDocument<'h>,
    catalog: &'h Catalog<'s>,
    lease: LocalExecutionLease,
    granted: Cell<bool>,
    serial: Cell<u64>,
    dispatched: Cell<u64>,
    pending: RefCell<Option<PendingAction>>,
}
/// The catalog and its native reply guards belong to exactly one execution.
pub fn with_host<T>(
    backend: &dyn Backend,
    use_host: impl FnOnce(&Host<'_, '_>, LocalExecutionScope) -> T,
) -> Result<T, Error> {
    let tracked = TrackedDocument::new(backend)?;
    let scope = LocalExecutionScope::new();
    let lease = scope.lease();
    let find_parameters = [NamedParameter::required(
        "name",
        ArgumentDomain::Scalar(ScalarTypeSet::only(ScalarType::String)),
    )];
    let observe_parameters = [NamedParameter::required("target", ArgumentDomain::Object)];
    let set_text_parameters = [
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
        tracked: &tracked,
        lease: lease.clone(),
        kind,
        expected: RefCell::new(None),
    };
    let schemas = [
        Schema {
            key: "gui.find",
            parameters: &find_parameters,
            required_capability: 1,
            result: domain(ResultKind::Object),
        },
        Schema {
            key: "gui.observe",
            parameters: &observe_parameters,
            required_capability: 1,
            result: domain(ResultKind::Snapshot),
        },
        Schema {
            key: "gui.set_text",
            parameters: &set_text_parameters,
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
    .map_err(|_| Error::Invalid("invalid AX catalog"))?;
    let host = Host {
        tracked: &tracked,
        catalog: &catalog,
        lease,
        granted: Cell::new(true),
        serial: Cell::new(0),
        dispatched: Cell::new(0),
        pending: RefCell::new(None),
    };
    Ok(use_host(&host, scope))
}
impl<'s> Host<'_, 's> {
    pub fn owner(&self) -> &LocalObjectOwner {
        &self.tracked.owner
    }
    pub fn lease(&self) -> LocalExecutionLease {
        self.lease.clone()
    }
    pub fn set_granted(&self, granted: bool) {
        self.granted.set(granted);
    }
    pub fn refresh(&self) -> Result<Snapshot, Error> {
        self.tracked.refresh()
    }
    pub fn discard_pending(&self) -> bool {
        self.pending.borrow_mut().take().is_some()
    }
    pub fn has_pending(&self) -> bool {
        self.pending.borrow().is_some()
    }
    fn resolve(&self, target: &ObjectRef) -> Result<(), Error> {
        self.tracked.resolve(target, &self.lease)
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
            .map_err(|_| Error::PermissionDenied)
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
    pub fn dispatch(
        &self,
        work: Work<'s>,
        fuel: &mut Fuel,
    ) -> HostWorkOutcome<Result<Receipt, Error>> {
        match self.invoke(work, fuel) {
            Ok(value) => HostWorkOutcome::Pending(Ok(value)),
            Err(error) => HostWorkOutcome::Failed(Err(error)),
        }
    }
    fn invoke(&self, work: Work<'s>, fuel: &mut Fuel) -> Result<Receipt, Error> {
        self.authorize(&work.identity)?;
        if self.dispatched.get() == work.identity.serial
            || !std::ptr::eq(self.schema(work.operation())?, work.call.schema())
        {
            return Err(Error::Invalid("stale or foreign dispatch"));
        }
        self.dispatched.set(work.identity.serial);
        let reply = match work.operation() {
            "gui.find" => {
                let Some(PureValue::Scalar(ScalarValue::String(name))) =
                    work.call.arguments().first().map(|arg| &arg.value)
                else {
                    return Err(Error::Invalid("missing selector"));
                };
                fuel.charge(1 + name.len() as u64)
                    .map_err(|_| Error::Fuel)?;
                if name != "document" {
                    return Err(Error::Invalid("unknown scoped target"));
                }
                self.refresh()?;
                let object = self.owner().reference(&self.lease)?;
                *work.call.schema().result.expected.borrow_mut() = Some(object.clone());
                Reply::Object(object)
            }
            "gui.observe" => {
                let target = work
                    .identity
                    .target
                    .as_ref()
                    .ok_or(Error::Invalid("missing target"))?;
                fuel.charge(1).map_err(|_| Error::Fuel)?;
                self.resolve(target)?;
                let data = self.refresh()?;
                fuel.charge(snapshot_copy_cost(&data))
                    .map_err(|_| Error::Fuel)?;
                Reply::Snapshot(Rc::new(self.owner().observe(target, &self.lease, data)?))
            }
            "gui.set_text" => {
                let target = work
                    .identity
                    .target
                    .as_ref()
                    .ok_or(Error::Invalid("missing target"))?;
                self.resolve(target)?;
                let data = self.refresh()?;
                let Some(PureValue::Scalar(ScalarValue::Integer(expected))) =
                    work.call.arguments().get(1).map(|arg| &arg.value)
                else {
                    return Err(Error::Invalid("missing revision"));
                };
                let Some(PureValue::Scalar(ScalarValue::String(text))) =
                    work.call.arguments().get(2).map(|arg| &arg.value)
                else {
                    return Err(Error::Invalid("missing text"));
                };
                if text.len() > MAX_SCALAR_STRING_BYTES {
                    return Err(Error::Bounds);
                }
                if !data.editable {
                    return Err(Error::NotEditable);
                }
                fuel.charge(1 + text.len() as u64)
                    .map_err(|_| Error::Fuel)?;
                let expected = *expected;
                let (_, mut arguments) = work.call.into_parts();
                let PureValue::Scalar(ScalarValue::String(text)) =
                    arguments.pop().ok_or(Error::Invalid("missing text"))?.value
                else {
                    return Err(Error::Invalid("missing text"));
                };
                let attempt = self
                    .owner()
                    .with_current(target, &self.lease, expected, || {
                        let mut pending = self.pending.borrow_mut();
                        if pending.is_some() {
                            return Err(Error::Invalid("host action queue is full"));
                        }
                        *pending = Some(PendingAction {
                            identity: work.identity.clone(),
                            revision: self.owner().revision(),
                            expected: data,
                            text,
                        });
                        Ok(())
                    })?;
                attempt.into_output()?;
                Reply::Submitted(true)
            }
            _ => return Err(Error::Invalid("unknown operation")),
        };
        Ok(Receipt {
            identity: work.identity,
            reply,
        })
    }
    /// Explicitly attempt the queued AX write. No polling, acceptance, retry or
    /// scheduler is hidden here. The bridge rechecks the snapshot immediately
    /// before its setter; the remote process can still race that check.
    pub fn pump(
        &self,
        receipt: Receipt,
        fuel: &mut Fuel,
    ) -> HostWorkOutcome<Result<Receipt, Error>> {
        let mut entered_backend = false;
        let result = (|| {
            self.authorize(&receipt.identity)?;
            if receipt.identity.operation != "gui.set_text"
                || !matches!(receipt.reply, Reply::Submitted(true))
            {
                return Err(Error::Invalid("pump requires original submission receipt"));
            }
            let target = receipt
                .identity
                .target
                .as_ref()
                .ok_or(Error::Invalid("missing target"))?;
            self.resolve(target)?;
            let data = self.refresh()?;
            let mut pending = self.pending.borrow_mut();
            if !pending
                .as_ref()
                .is_some_and(|update| update.identity == receipt.identity)
            {
                return Err(Error::Invalid("no matching pending action"));
            }
            let update = pending
                .take()
                .ok_or(Error::Invalid("pending action vanished"))?;
            drop(pending);
            if update.expected != data {
                return Err(Error::Stale);
            }
            if !data.editable {
                return Err(Error::NotEditable);
            }
            fuel.charge(1 + update.text.len() as u64)
                .map_err(|_| Error::Fuel)?;
            let attempt =
                self.owner()
                    .with_current(target, &self.lease, update.revision, || {
                        entered_backend = true;
                        self.tracked
                            .backend
                            .set_text(&update.expected, &update.text)
                    })?;
            attempt.into_output()?;
            // A returned setter success is still only an acknowledgement.
            // The program must request a separate observation of the result.
            Ok(receipt)
        })();
        match result {
            Ok(value) => HostWorkOutcome::Pending(Ok(value)),
            // A backend must label failures after entering the remote setter as
            // Uncertain. Known pre-write recheck failures remain Failed.
            Err(error @ Error::Uncertain(_)) if entered_backend => {
                HostWorkOutcome::Uncertain(Err(error))
            }
            Err(error) => HostWorkOutcome::Failed(Err(error)),
        }
    }
}
impl ReplyAuthority<Identity> for Host<'_, '_> {
    type Error = Error;
    fn authorize(&self, identity: &Identity) -> Result<(), Error> {
        if !self.lease.is_open()
            || !self.lease.same_scope(&identity.lease)
            || identity.serial == 0
            || identity.serial != self.serial.get()
        {
            return Err(Error::Invalid("foreign or closed execution"));
        }
        self.tracked.backend.check_permission()?;
        self.schema(identity.operation)?;
        if let Some(target) = &identity.target {
            self.resolve(target)?;
        }
        Ok(())
    }
}
impl PureTypeEnvironment<Field, &'static str> for Host<'_, '_> {
    type Result = NativeType;
    fn field_type(&self, result: &NativeType, field: &Field) -> Option<ScalarType> {
        if *result != NativeType::Snapshot {
            return None;
        }
        Some(match field {
            Field::Revision => ScalarType::Integer,
            Field::Document | Field::Role | Field::Value => ScalarType::String,
            Field::Editable => ScalarType::Boolean,
        })
    }
    fn member_result(&self, _: &NativeType, _: &str, _: &&'static str) -> Option<NativeType> {
        None
    }
}
impl<'s> CallFlowEnvironment<'s, Field, &'static str, Domain<'s>> for Host<'_, 's> {
    fn call_native_argument_type(
        &self,
        operation: &&'static str,
        parameter_name: &str,
        result: &NativeType,
    ) -> bool {
        matches!(*operation, "gui.observe" | "gui.set_text")
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
impl<'source, 's> CatalogSourceEnvironment<'source, 's, Field, &'static str, Domain<'s>>
    for Host<'_, 's>
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
            "document" => Some((Field::Document, ScalarType::String)),
            "role" => Some((Field::Role, ScalarType::String)),
            "value" => Some((Field::Value, ScalarType::String)),
            "editable" => Some((Field::Editable, ScalarType::Boolean)),
            "revision" => Some((Field::Revision, ScalarType::Integer)),
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
impl PureEvaluationEnvironment<Field, &'static str> for Host<'_, '_> {
    type Result = NativeValue;
    type Error = Error;
    fn field(&self, observed: &Self::Result, field: &Field) -> Result<ScalarValue, Error> {
        let NativeValue::Snapshot(observed) = observed else {
            return Err(Error::Invalid("object has no scalar fields"));
        };
        self.resolve(observed.object())?;
        let current = self.refresh()?;
        if observed.data().identity != current.identity
            || observed.data().document != current.document
            || observed.data().role != current.role
        {
            return Err(Error::Invalid(
                "snapshot payload belongs to another native instance",
            ));
        }
        validate_snapshot(observed.data())?;
        Ok(match field {
            Field::Document => ScalarValue::String(observed.data().document.clone()),
            Field::Role => ScalarValue::String(observed.data().role.clone()),
            Field::Value => ScalarValue::String(observed.data().value.clone()),
            Field::Editable => ScalarValue::Boolean(observed.data().editable),
            Field::Revision => ScalarValue::Integer(observed.revision()),
        })
    }
    fn member(&self, _: &Self::Result, _: &str, _: &&'static str) -> Result<Self::Result, Error> {
        Err(Error::Invalid("no native groups"))
    }
}
impl<'ir, 's> EffectEvaluationEnvironment<'ir, Field, &'static str, (), ()> for Host<'_, 's> {
    type Request = Prepared<'s>;
    type Capture = RestoredEffectBindings<'ir, NativeValue>;
    fn preflight_effect(&self, node: &'ir Node) -> Result<(), Error> {
        let Node::Call {
            operation,
            arguments,
        } = node
        else {
            return Err(Error::Invalid("only atomic operations"));
        };
        self.schema(operation)?
            .bind_arguments(
                &arguments
                    .iter()
                    .map(|arg| arg.name.as_str())
                    .collect::<Vec<_>>(),
            )
            .map_err(|_| Error::Invalid("invalid call signature"))?;
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
            return Err(Error::Invalid("only atomic operations").into());
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
                        .map_err(|_| Error::Invalid("scalar argument rejected"))
                }
                _ => Err(Error::Invalid("native argument type rejected")),
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
            _ => Error::Invalid("call preparation failed").into(),
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
                .ok_or(Error::Invalid("capture cost overflow"))?;
        }
        fuel.charge(cost).map_err(|_| Error::Fuel)?;
        Ok(scope.bindings().to_vec())
    }
}
impl<'ir, 's>
    EffectReentryEnvironment<'ir, Field, &'static str, (), (), Identity, Domain<'s>, Reply>
    for Host<'_, 's>
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
        reply: Reply,
        _: &mut Fuel,
    ) -> Result<PureValue<Self::Result>, CalculationFailure<Error>> {
        self.authorize(identity)?;
        if !std::ptr::eq(domain, &self.schema(identity.operation)?.result) {
            return Err(Error::Invalid("foreign result domain").into());
        }
        domain.validate_value(&reply)?;
        Ok(match reply {
            Reply::Object(value) => PureValue::Result(NativeValue::Object(value)),
            Reply::Snapshot(value) => PureValue::Result(NativeValue::Snapshot(value)),
            Reply::Submitted(value) => PureValue::Scalar(ScalarValue::Boolean(value)),
        })
    }
}
impl<'ir, 's> EffectSessionEnvironment<'ir, 's, Field, &'static str, (), ()> for Host<'_, 's> {
    type Identity = Identity;
    type Declaration = Domain<'s>;
    type Reply = Reply;
    type Dispatch = Work<'s>;
    fn correlate_request(
        &self,
        request: Self::Request,
        _: &mut Fuel,
    ) -> EffectCorrelation<'s, Identity, Domain<'s>, Work<'s>, Error> {
        let schema = self.schema(request.schema().key)?;
        if !std::ptr::eq(schema, request.schema()) {
            return Err(Error::Invalid("foreign schema").into());
        }
        let target = if schema.key == "gui.find" {
            None
        } else {
            let Some(PureValue::Result(NativeValue::Object(target))) =
                request.arguments().first().map(|arg| &arg.value)
            else {
                return Err(Error::Invalid("missing target").into());
            };
            self.resolve(target)?;
            Some(target.clone())
        };
        *schema.result.expected.borrow_mut() = target.clone();
        let serial = self
            .serial
            .get()
            .checked_add(1)
            .ok_or(Error::Invalid("identity exhausted"))?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::{self, Vm};
    use leselang_vm::native::{
        LocalVm,
        hir::effect_session::{EffectSessionEnd, EffectSessionPoll, EffectSessionStatus},
    };

    struct Fake {
        state: RefCell<Snapshot>,
        permitted: Cell<bool>,
        available: Cell<bool>,
        writes: Cell<usize>,
        outcome: Cell<u8>,
    }
    impl Fake {
        fn new() -> Self {
            Self {
                state: RefCell::new(Snapshot {
                    identity: 1,
                    document: "file:///tmp/leselang-test.txt".into(),
                    role: "AXTextArea".into(),
                    value: INITIAL_TEXT.into(),
                    editable: true,
                }),
                permitted: Cell::new(true),
                available: Cell::new(true),
                writes: Cell::new(0),
                outcome: Cell::new(0),
            }
        }
        fn value(&self) -> String {
            self.state.borrow().value.clone()
        }
    }
    impl Backend for Fake {
        fn snapshot(&self) -> Result<Snapshot, Error> {
            self.check_permission()?;
            if !self.available.get() {
                return Err(Error::Unavailable("document closed"));
            }
            Ok(self.state.borrow().clone())
        }
        fn check_permission(&self) -> Result<(), Error> {
            if self.permitted.get() {
                Ok(())
            } else {
                Err(Error::PermissionDenied)
            }
        }
        fn set_text(&self, expected: &Snapshot, text: &str) -> Result<(), Error> {
            // Simulate a remote change after the host's last snapshot but before
            // the bridge's immediate comparison.
            if self.outcome.get() == 3 {
                self.state.borrow_mut().value = "external race".into();
            }
            if self.snapshot()? != *expected {
                return Err(Error::Stale);
            }
            if !expected.editable {
                return Err(Error::NotEditable);
            }
            self.writes.set(self.writes.get() + 1);
            if self.outcome.get() != 2 {
                self.state.borrow_mut().value = text.into();
            }
            if self.outcome.get() == 1 {
                Err(Error::Uncertain(-25204))
            } else {
                Ok(())
            }
        }
    }
    fn session(
        fake: &Fake,
        source: &str,
        body: impl for<'ir, 's> FnOnce(&Host<'_, 's>, &mut Vm<'ir, 's>),
    ) {
        with_host(fake, |host, scope| {
            let node = host.compile(source).unwrap();
            let mut vm = LocalVm::new(scope, &node, Vec::new(), Fuel::new(1000), STACK);
            body(host, &mut vm);
        })
        .unwrap();
    }
    fn find<'ir, 's>(host: &Host<'_, 's>, vm: &mut Vm<'ir, 's>) -> ObjectRef {
        let work = driver::next(vm, host).unwrap();
        assert_eq!(work.operation(), "gui.find");
        let receipt = driver::dispatch(vm, host, work).unwrap();
        let Reply::Object(object) = &receipt.reply else {
            panic!("expected object")
        };
        let object = object.clone();
        driver::accept(vm, host, receipt).unwrap();
        object
    }
    fn observe<'ir, 's>(host: &Host<'_, 's>, vm: &mut Vm<'ir, 's>) -> Rc<Observation<Snapshot>> {
        let work = driver::next(vm, host).unwrap();
        assert_eq!(work.operation(), "gui.observe");
        let receipt = driver::dispatch(vm, host, work).unwrap();
        let Reply::Snapshot(snapshot) = &receipt.reply else {
            panic!("expected snapshot")
        };
        let snapshot = snapshot.clone();
        driver::accept(vm, host, receipt).unwrap();
        snapshot
    }
    fn ready<'ir, 's>(host: &Host<'_, 's>, vm: &mut Vm<'ir, 's>) -> Work<'s> {
        let _ = find(host, vm);
        observe(host, vm);
        let work = driver::next(vm, host).unwrap();
        assert_eq!(work.operation(), "gui.set_text");
        work
    }
    fn failed(vm: &Vm<'_, '_>) {
        assert_eq!(
            vm.status(),
            EffectSessionStatus::Terminal(EffectSessionEnd::Failed)
        );
    }
    #[test]
    fn complete_source_workflow_uses_four_effects_and_observed_value() {
        let fake = Fake::new();
        let summary = driver::run(&fake, PROGRAM).unwrap();
        assert_eq!(summary.effects, 4);
        assert_eq!(summary.observations.len(), 2);
        assert_eq!(summary.value, ScalarValue::String(RESULT_TEXT.into()));
        assert_eq!(fake.writes.get(), 1);
        assert_eq!(summary.observations[0].value, INITIAL_TEXT);
        assert_eq!(summary.observations[1].value, RESULT_TEXT);
        assert!(summary.fuel_remaining < 1000);
    }
    #[test]
    fn submission_pump_accept_and_historical_observation_are_separate() {
        let fake = Fake::new();
        session(&fake, PROGRAM, |host, vm| {
            let _ = find(host, vm);
            let before = observe(host, vm);
            let work = driver::next(vm, host).unwrap();
            let receipt = driver::dispatch(vm, host, work).unwrap();
            assert_eq!(fake.value(), INITIAL_TEXT);
            assert!(host.has_pending());
            assert!(matches!(
                vm.poll(host).unwrap(),
                EffectSessionPoll::Awaiting
            ));
            let receipt = driver::pump(vm, host, receipt).unwrap();
            assert_eq!(fake.value(), RESULT_TEXT);
            assert_eq!(fake.writes.get(), 1);
            assert!(!host.has_pending());
            assert_eq!(vm.status(), EffectSessionStatus::Awaiting);
            assert_eq!(before.data().value, INITIAL_TEXT);
            assert_eq!(
                host.field(&NativeValue::Snapshot(before.clone()), &Field::Value)
                    .unwrap(),
                ScalarValue::String(INITIAL_TEXT.into())
            );
            driver::accept(vm, host, receipt).unwrap();
            let after = observe(host, vm);
            assert!(after.revision() > before.revision());
            assert_eq!(after.data().value, RESULT_TEXT);
        });
    }
    #[test]
    fn demo_refuses_to_edit_unexpected_initial_content() {
        let fake = Fake::new();
        fake.state.borrow_mut().value = "unrelated document contents".into();
        let summary = driver::run(&fake, PROGRAM).unwrap();
        assert_eq!(summary.effects, 2);
        assert_eq!(
            summary.value,
            ScalarValue::String("initial content mismatch".into())
        );
        assert_eq!(fake.writes.get(), 0);
    }
    #[test]
    fn observed_state_identity_and_authority_fences_precede_writes() {
        for queued in [false, true] {
            for cause in 0..7 {
                let fake = Fake::new();
                session(&fake, PROGRAM, |host, vm| {
                    let mut work = Some(ready(host, vm));
                    let receipt =
                        queued.then(|| driver::dispatch(vm, host, work.take().unwrap()).unwrap());
                    match cause {
                        0 => fake.state.borrow_mut().value = "external edit".into(),
                        1 => fake.state.borrow_mut().identity += 1,
                        2 => fake.state.borrow_mut().document = "file:///tmp/other.txt".into(),
                        3 => fake.permitted.set(false),
                        4 => host.set_granted(false),
                        5 => fake.state.borrow_mut().editable = false,
                        6 => fake.available.set(false),
                        _ => unreachable!(),
                    }
                    let result = if let Some(receipt) = receipt {
                        driver::pump(vm, host, receipt)
                    } else {
                        vm.with_metered_host_outcome(work.take().unwrap(), |work, fuel| {
                            host.dispatch(work, fuel)
                        })
                        .unwrap()
                    };
                    assert!(result.is_err(), "queued={queued}, cause={cause}");
                    assert_eq!(fake.writes.get(), 0);
                    failed(vm);
                });
            }
        }
    }
    #[test]
    fn changed_then_observed_then_restored_state_invalidates_revision() {
        let fake = Fake::new();
        session(&fake, PROGRAM, |host, vm| {
            let work = ready(host, vm);
            fake.state.borrow_mut().value = "external".into();
            host.refresh().unwrap();
            fake.state.borrow_mut().value = INITIAL_TEXT.into();
            let result = vm
                .with_metered_host_outcome(work, |work, fuel| host.dispatch(work, fuel))
                .unwrap();
            assert!(result.is_err());
            assert_eq!(fake.writes.get(), 0);
            failed(vm);
        });
    }
    #[test]
    fn bridge_recheck_rejects_a_late_external_change_without_overwrite() {
        let fake = Fake::new();
        session(&fake, PROGRAM, |host, vm| {
            let work = ready(host, vm);
            let receipt = driver::dispatch(vm, host, work).unwrap();
            fake.outcome.set(3);
            assert!(matches!(driver::pump(vm, host, receipt), Err(Error::Stale)));
            failed(vm);
            assert_eq!(fake.value(), "external race");
            assert_eq!(fake.writes.get(), 0);
        });
    }
    #[test]
    fn cancellation_and_late_replies_preserve_completed_effects_and_fuel() {
        for delivered in [false, true] {
            let fake = Fake::new();
            session(&fake, PROGRAM, |host, vm| {
                let work = ready(host, vm);
                let receipt = driver::dispatch(vm, host, work).unwrap();
                let receipt = if delivered {
                    driver::pump(vm, host, receipt).unwrap()
                } else {
                    receipt
                };
                assert!(vm.cancel());
                let fuel = vm.fuel_remaining();
                let rejected = vm
                    .try_accept(&receipt.identity, receipt.reply, host)
                    .err()
                    .unwrap();
                assert!(matches!(rejected.reply, Reply::Submitted(true)));
                assert_eq!(vm.fuel_remaining(), fuel);
                assert_eq!(
                    fake.value(),
                    if delivered { RESULT_TEXT } else { INITIAL_TEXT }
                );
                if !delivered {
                    // The closed execution also denies direct delivery of its queue.
                    let receipt = Receipt {
                        identity: receipt.identity,
                        reply: rejected.reply,
                    };
                    assert!(matches!(
                        host.pump(receipt, &mut Fuel::new(100)),
                        HostWorkOutcome::Failed(Err(_))
                    ));
                    assert!(host.discard_pending());
                }
                assert_eq!(fake.writes.get(), usize::from(delivered));
            });
        }
    }
    #[test]
    fn original_fuel_is_required_at_poll_and_at_queued_execution() {
        let fake = Fake::new();
        with_host(&fake, |host, scope| {
            let node = host.compile(PROGRAM).unwrap();
            let mut vm = LocalVm::new(scope, &node, Vec::new(), Fuel::new(0), STACK);
            assert!(vm.poll(host).is_err());
            assert_eq!(vm.fuel_remaining(), 0);
        })
        .unwrap();
        session(&fake, PROGRAM, |host, vm| {
            let work = ready(host, vm);
            let receipt = driver::dispatch(vm, host, work).unwrap();
            vm.with_metered_host_outcome((), |_, fuel| {
                fuel.charge(fuel.remaining()).unwrap();
                HostWorkOutcome::Pending(())
            })
            .unwrap();
            assert!(matches!(driver::pump(vm, host, receipt), Err(Error::Fuel)));
            assert_eq!(vm.fuel_remaining(), 0);
            failed(vm);
        });
        assert_eq!(fake.writes.get(), 0);
    }
    #[test]
    fn uncertain_write_preserves_mutation_and_closes_vm_without_retry() {
        let fake = Fake::new();
        session(&fake, PROGRAM, |host, vm| {
            let work = ready(host, vm);
            let receipt = driver::dispatch(vm, host, work).unwrap();
            fake.outcome.set(1);
            assert!(matches!(
                driver::pump(vm, host, receipt),
                Err(Error::Uncertain(_))
            ));
            assert_eq!(
                vm.status(),
                EffectSessionStatus::Terminal(EffectSessionEnd::HostUncertain)
            );
            assert_eq!(fake.value(), RESULT_TEXT);
            assert_eq!(fake.writes.get(), 1);
            assert!(!host.has_pending());
            assert!(matches!(
                vm.poll(host).unwrap(),
                EffectSessionPoll::Terminal(EffectSessionEnd::HostUncertain)
            ));
            assert_eq!(fake.writes.get(), 1);
        });
    }
    #[test]
    fn a_completed_submission_cannot_be_pumped_twice() {
        let fake = Fake::new();
        session(&fake, PROGRAM, |host, vm| {
            let work = ready(host, vm);
            let receipt = driver::dispatch(vm, host, work).unwrap();
            let receipt = driver::pump(vm, host, receipt).unwrap();
            let replay = Receipt {
                identity: receipt.identity.clone(),
                reply: Reply::Submitted(true),
            };
            assert!(driver::pump(vm, host, replay).is_err());
            failed(vm);
            assert_eq!(fake.writes.get(), 1);
            assert_eq!(fake.value(), RESULT_TEXT);
            assert!(
                vm.try_accept(&receipt.identity, receipt.reply, host)
                    .is_err()
            );
        });
    }
    #[test]
    fn permission_revocation_after_write_rejects_reply_without_undoing_text() {
        let fake = Fake::new();
        session(&fake, PROGRAM, |host, vm| {
            let work = ready(host, vm);
            let receipt = driver::dispatch(vm, host, work).unwrap();
            let receipt = driver::pump(vm, host, receipt).unwrap();
            let fuel = vm.fuel_remaining();
            fake.permitted.set(false);
            let rejected = vm
                .try_accept(&receipt.identity, receipt.reply, host)
                .err()
                .unwrap();
            assert!(matches!(rejected.reply, Reply::Submitted(true)));
            assert_eq!(vm.fuel_remaining(), fuel);
            assert_eq!(vm.status(), EffectSessionStatus::Awaiting);
            assert_eq!(fake.writes.get(), 1);
            assert_eq!(fake.value(), RESULT_TEXT);
            assert!(vm.cancel());
        });
    }
    #[test]
    fn acknowledgement_without_the_observed_postcondition_is_not_success() {
        let fake = Fake::new();
        fake.outcome.set(2);
        let summary = driver::run(&fake, PROGRAM).unwrap();
        assert_eq!(
            summary.value,
            ScalarValue::String("verification failed".into())
        );
        assert_eq!(fake.value(), INITIAL_TEXT);
        assert_eq!(fake.writes.get(), 1);
    }
    #[test]
    fn foreign_object_and_spoofed_snapshot_replies_preserve_pending_request() {
        let fake = Fake::new();
        session(&fake, PROGRAM, |host, vm| {
            let work = driver::next(vm, host).unwrap();
            let receipt = driver::dispatch(vm, host, work).unwrap();
            let foreign_owner = LocalObjectOwner::new();
            let foreign = foreign_owner.reference(&host.lease()).unwrap();
            let fuel = vm.fuel_remaining();
            let rejected = vm
                .try_accept(&receipt.identity, Reply::Object(foreign), host)
                .err()
                .unwrap();
            assert!(matches!(rejected.reply, Reply::Object(_)));
            assert_eq!(vm.status(), EffectSessionStatus::Awaiting);
            assert_eq!(vm.fuel_remaining(), fuel);
            driver::accept(vm, host, receipt).unwrap();
            let work = driver::next(vm, host).unwrap();
            let receipt = driver::dispatch(vm, host, work).unwrap();
            let Reply::Snapshot(original) = &receipt.reply else {
                panic!("snapshot")
            };
            let mut corrupt = original.data().clone();
            corrupt.identity += 1;
            let corrupt = host
                .owner()
                .observe(original.object(), &host.lease(), corrupt)
                .unwrap();
            let fuel = vm.fuel_remaining();
            let rejected = vm
                .try_accept(&receipt.identity, Reply::Snapshot(Rc::new(corrupt)), host)
                .err()
                .unwrap();
            assert!(matches!(rejected.reply, Reply::Snapshot(_)));
            assert_eq!(vm.status(), EffectSessionStatus::Awaiting);
            assert_eq!(vm.fuel_remaining(), fuel);
            driver::accept(vm, host, receipt).unwrap();
        });
    }
    #[test]
    fn scalar_runner_and_native_final_result_preserve_scope_boundary() {
        let fake = Fake::new();
        assert_eq!(
            driver::run(&fake, "fn main() = add(left: 20, right: 22)")
                .unwrap()
                .value,
            ScalarValue::Integer(42)
        );
        assert!(
            driver::run(&fake, "fn main() = gui.find(name: \"document\")")
                .unwrap_err()
                .contains("session-bound native value")
        );
        let source = r#"fn main() = bind(target: gui.find(name: "document"), body: bind(before: gui.observe(target: target), body: bind(submitted: gui.set_text(target: target, expected_revision: field(value: before, name: "revision"), text: "retained effect"), body: gui.observe(target: target))))"#;
        assert!(
            driver::run(&fake, source)
                .unwrap_err()
                .contains("session-bound native value")
        );
        assert_eq!(fake.value(), "retained effect");
        assert_eq!(fake.writes.get(), 1);
    }
    #[test]
    fn malformed_roles_bounds_and_selectors_refuse_before_write() {
        for cause in 0..4 {
            let fake = Fake::new();
            match cause {
                0 => fake.state.borrow_mut().role = "AXButton".into(),
                1 => fake.state.borrow_mut().value = "x".repeat(MAX_SCALAR_STRING_BYTES + 1),
                2 => fake.state.borrow_mut().identity = 0,
                3 => fake.permitted.set(false),
                _ => unreachable!(),
            }
            assert!(driver::run(&fake, PROGRAM).is_err());
            assert_eq!(fake.writes.get(), 0);
        }
        let fake = Fake::new();
        assert!(
            driver::run(
                &fake,
                &PROGRAM.replace("name: \"document\"", "name: \"other\"")
            )
            .is_err()
        );
        let oversized = PROGRAM.replace(
            "LeseLang cross-process ✓\\n",
            &"x".repeat(MAX_SCALAR_STRING_BYTES + 1),
        );
        assert!(driver::run(&fake, &oversized).is_err());
        assert_eq!(fake.writes.get(), 0);
    }
}
