//! Typed, explicitly driven AppKit host. All native objects stay on the main thread.
use crate::{
    Error,
    gui::{Gui, WIDGET_COUNT, WidgetSnapshot},
};
use std::{
    cell::{Cell, RefCell},
    fmt,
    rc::Rc,
};

use call_evaluation::{
    CallEvaluationError, CallEvaluationLimits, PreparedValueCall, ValueCallEvaluationError,
    evaluate_value_call_arguments_in_scope,
};
use catalog_program::{
    CatalogProgramLimits, CompiledCatalogProgram, compile_catalog_typed_program,
};
use catalog_source::{CatalogSourceEnvironment, CatalogSourceLimits};
use control_source::ControlSourceLimits;
use effect_evaluation::{EffectEvaluationEnvironment, EffectEvaluationLimits};
use effect_reentry::{EffectReentryEnvironment, RestoredEffectBindings};
use effect_session::{CorrelatedEffectRequest, EffectCorrelation, EffectSessionEnvironment};
use effect_stack::EffectStackLimits;
use flow_typing::CallFlowEnvironment;
use host_protocol::{LocalObjectOwner, ObjectError, ObjectRef, Observation};
use leselang_vm::native::{HostWorkOutcome, core::*, hir::*, syntax::parse};
use pure_evaluation::{
    PureEvaluationEnvironment, PureEvaluationLimits, PureValue, scalar_copy_cost,
};
use pure_typing::{PureType, PureTypeEnvironment};
use source_call::{SourceCallHost, SourceCallLimits};

pub const PROGRAM: &str = r#"
fn main() = bind(
    target: gui.find(name: "save"),
    body: bind(
        before: gui.observe(target: target),
        body: bind(
            submitted: gui.click(target: target, expected_revision: field(value: before, name: "revision")),
            body: bind(
                after: gui.observe(target: target),
                body: choose(
                    when: eq(left: field(value: after, name: "clicks"), right: add(left: field(value: before, name: "clicks"), right: 1)),
                    then: field(value: after, name: "title"),
                    otherwise: "verification failed",
                ),
            ),
        ),
    ),
)
"#;
/// Discover identifiers as scalar metadata, then resolve fresh object references.
/// The final value is read from Save's actual submitted-value snapshot.
pub const FORM_PROGRAM: &str = include_str!("../examples/form.lese");
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

struct TrackedGui<'g> {
    gui: &'g Gui,
    owners: [LocalObjectOwner; WIDGET_COUNT],
    previous: RefCell<[WidgetSnapshot; WIDGET_COUNT]>,
}
impl<'g> TrackedGui<'g> {
    fn new(gui: &'g Gui) -> Result<Self, Error> {
        Ok(Self {
            gui,
            owners: std::array::from_fn(|_| LocalObjectOwner::new()),
            previous: RefCell::new([gui.snapshot(0)?, gui.snapshot(1)?, gui.snapshot(2)?]),
        })
    }
    fn require_open(&self) -> Result<(), Error> {
        if !self.gui.is_open()? {
            for owner in &self.owners {
                owner.retire();
            }
            return Err(Error::Native("AppKit window is closed"));
        }
        Ok(())
    }
    fn refresh(&self, slot: usize) -> Result<WidgetSnapshot, Error> {
        self.require_open()?;
        let owner = self
            .owners
            .get(slot)
            .ok_or(Error::Native("unknown widget slot"))?;
        let snapshot = match self.gui.snapshot(slot) {
            Ok(value) => value,
            Err(error) => {
                owner.retire();
                return Err(error);
            }
        };
        if let Err(error) = validate_snapshot(&snapshot) {
            owner.retire();
            return Err(error);
        }
        let mut previous = self.previous.borrow_mut();
        let tracking = if previous[slot].identity != snapshot.identity {
            owner.replace()
        } else if previous[slot] != snapshot {
            owner.advance_revision().map(|_| ())
        } else {
            Ok(())
        };
        if let Err(error) = tracking {
            // A framework event already occurred. Never publish it under old metadata.
            owner.retire();
            return Err(error.into());
        }
        previous[slot] = snapshot.clone();
        Ok(snapshot)
    }
    fn resolve(&self, target: &ObjectRef, lease: &LocalExecutionLease) -> Result<usize, Error> {
        for (slot, owner) in self.owners.iter().enumerate() {
            match owner.validate(target, lease) {
                Err(ObjectError::ForeignOwner) => continue,
                result => {
                    result?;
                    self.refresh(slot)?;
                    owner.validate(target, lease)?;
                    return Ok(slot);
                }
            }
        }
        Err(ObjectError::ForeignOwner.into())
    }
    fn replace(&self, slot: usize) -> Result<(), Error> {
        self.require_open()?;
        let owner = self
            .owners
            .get(slot)
            .ok_or(Error::Native("unknown widget slot"))?;
        owner.replace()?;
        let result = self
            .gui
            .replace(slot)
            .and_then(|()| self.gui.snapshot(slot));
        match result {
            Ok(snapshot) => {
                self.previous.borrow_mut()[slot] = snapshot;
                Ok(())
            }
            Err(error) => {
                owner.retire();
                Err(error)
            }
        }
    }
}

fn validate_snapshot(value: &WidgetSnapshot) -> Result<(), Error> {
    if [&value.title, &value.value, &value.identifier, &value.role]
        .iter()
        .any(|text| text.len() > MAX_SCALAR_STRING_BYTES)
    {
        return Err(Error::Native("snapshot text exceeds language bounds"));
    }
    if value.identity == 0
        || value.identifier.is_empty()
        || !matches!(value.role.as_str(), "button" | "text_field")
    {
        return Err(Error::Native("invalid native snapshot metadata"));
    }
    Ok(())
}
fn snapshot_copy_cost(value: &WidgetSnapshot) -> u64 {
    (value.title.len() + value.value.len() + value.identifier.len() + value.role.len()) as u64
}
fn valid_identifiers(value: &StringListValue) -> bool {
    value.0.len() <= WIDGET_COUNT
        && value.is_bounded()
        && value
            .0
            .iter()
            .enumerate()
            .all(|(index, name)| !name.is_empty() && !value.0[..index].contains(name))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Field {
    Identifier,
    Role,
    Value,
    Editable,
    Revision,
    Title,
    Clicks,
    Enabled,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NativeType {
    Object,
    Snapshot,
}
#[derive(Clone, Copy)]
enum ResultKind {
    Identifiers,
    Object,
    Snapshot,
    Submitted,
}
pub enum Reply {
    Identifiers(StringListValue),
    Object(ObjectRef),
    Snapshot(Rc<Observation<WidgetSnapshot>>),
    Submitted(bool),
}
impl fmt::Debug for Reply {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Identifiers(_) => "Identifiers",
            Self::Object(_) => "Object",
            Self::Snapshot(_) => "Snapshot",
            Self::Submitted(_) => "Submitted",
        })
    }
}
pub struct Domain<'a> {
    tracked: &'a TrackedGui<'a>,
    lease: LocalExecutionLease,
    kind: ResultKind,
    expected: RefCell<Option<ObjectRef>>,
}
impl Domain<'_> {
    fn validate_object(&self, actual: &ObjectRef) -> Result<usize, Error> {
        let expected = self.expected.borrow();
        let expected = expected
            .as_ref()
            .ok_or(Error::Native("no requested native target"))?;
        let slot = self.tracked.resolve(expected, &self.lease)?;
        self.tracked.owners[slot].validate(actual, &self.lease)?;
        Ok(slot)
    }
}
impl HostResultDomain<Reply> for Domain<'_> {
    type Error = Error;
    fn matches_type(&self, reply: &Reply) -> bool {
        matches!(
            (self.kind, reply),
            (ResultKind::Identifiers, Reply::Identifiers(_))
                | (ResultKind::Object, Reply::Object(_))
                | (ResultKind::Snapshot, Reply::Snapshot(_))
                | (ResultKind::Submitted, Reply::Submitted(_))
        )
    }
    fn validate_value(&self, reply: &Reply) -> Result<(), Error> {
        match (self.kind, reply) {
            (ResultKind::Identifiers, Reply::Identifiers(value)) if valid_identifiers(value) => {
                Ok(())
            }
            (ResultKind::Object, Reply::Object(value)) => self.validate_object(value).map(|_| ()),
            (ResultKind::Snapshot, Reply::Snapshot(value)) => {
                let slot = self.validate_object(value.object())?;
                let current = self.tracked.refresh(slot)?;
                if value.data().identity != current.identity
                    || value.data().identifier != current.identifier
                    || value.data().role != current.role
                {
                    return Err(Error::Native(
                        "snapshot payload belongs to another native instance",
                    ));
                }
                validate_snapshot(value.data())
            }
            (ResultKind::Submitted, Reply::Submitted(true)) => Ok(()),
            _ => Err(Error::Native(
                "reply does not match the original result domain",
            )),
        }
    }
}
#[derive(Clone)]
pub enum NativeValue {
    Object(ObjectRef),
    Snapshot(Rc<Observation<WidgetSnapshot>>),
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
enum Action {
    Click,
    SetText(String),
}
impl Action {
    fn validate(&self, data: &WidgetSnapshot) -> Result<(), Error> {
        if !data.enabled {
            return Err(Error::Native("control is disabled"));
        }
        match self {
            Self::Click if data.role == "button" => Ok(()),
            Self::SetText(_) if data.role == "text_field" && data.editable => Ok(()),
            Self::SetText(_) if data.role == "text_field" => {
                Err(Error::Native("text field is not editable"))
            }
            _ => Err(Error::Native(
                "operation is not supported by this control role",
            )),
        }
    }
    fn cost(&self) -> u64 {
        match self {
            Self::Click => 1,
            Self::SetText(text) => 1 + text.len() as u64,
        }
    }
}
struct PendingAction {
    identity: Identity,
    slot: usize,
    revision: u64,
    native_identity: usize,
    action: Action,
}
pub struct Host<'h, 's> {
    tracked: &'h TrackedGui<'h>,
    catalog: &'h Catalog<'s>,
    lease: LocalExecutionLease,
    granted: Cell<bool>,
    serial: Cell<u64>,
    dispatched: Cell<u64>,
    pending: RefCell<Option<PendingAction>>,
}

/// One session and at most one outstanding request own this catalog's reply guards.
pub fn with_host<T>(
    gui: &Gui,
    use_host: impl FnOnce(&Host<'_, '_>, LocalExecutionScope) -> T,
) -> Result<T, Error> {
    let tracked = TrackedGui::new(gui)?;
    let scope = LocalExecutionScope::new();
    let lease = scope.lease();
    let find_parameters = [NamedParameter::required(
        "name",
        ArgumentDomain::Scalar(ScalarTypeSet::only(ScalarType::String)),
    )];
    let observe_parameters = [NamedParameter::required("target", ArgumentDomain::Object)];
    let click_parameters = [
        NamedParameter::required("target", ArgumentDomain::Object),
        NamedParameter::required(
            "expected_revision",
            ArgumentDomain::Scalar(ScalarTypeSet::only(ScalarType::Integer)),
        ),
    ];
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
            key: "gui.list",
            parameters: &[],
            required_capability: 1,
            result: domain(ResultKind::Identifiers),
        },
        Schema {
            key: "gui.set_text",
            parameters: &set_text_parameters,
            required_capability: 2,
            result: domain(ResultKind::Submitted),
        },
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
            key: "gui.click",
            parameters: &click_parameters,
            required_capability: 2,
            result: domain(ResultKind::Submitted),
        },
    ];
    let catalog = OperationCatalog::new(
        1,
        &schemas,
        OperationCatalogLimits {
            max_operations: 5,
            max_parameters_per_operation: 3,
        },
    )
    .map_err(|_| Error::Native("invalid AppKit operation catalog"))?;
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
    pub fn owner(&self, slot: usize) -> &LocalObjectOwner {
        &self.tracked.owners[slot]
    }
    pub fn lease(&self) -> LocalExecutionLease {
        self.lease.clone()
    }
    pub fn set_granted(&self, granted: bool) {
        self.granted.set(granted);
    }
    pub fn refresh(&self, slot: usize) -> Result<WidgetSnapshot, Error> {
        self.tracked.refresh(slot)
    }
    pub fn replace(&self, slot: usize) -> Result<(), Error> {
        self.tracked.replace(slot)
    }
    pub fn discard_pending(&self) -> bool {
        self.pending.borrow_mut().take().is_some()
    }
    pub fn has_pending(&self) -> bool {
        self.pending.borrow().is_some()
    }
    fn resolve(&self, target: &ObjectRef) -> Result<usize, Error> {
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
            .map_err(|_| Error::Native("operation denied"))
    }
    pub fn compile(&self, source: &str) -> Result<Node, String> {
        let program = self.compile_task(source, "main")?;
        if !program.entry_parameters.is_empty() {
            return Err("default entry cannot have parameters; use compile_task".into());
        }
        Ok(program.node)
    }

    /// Compile a task with scalar ingress and explicitly named native helper types.
    /// Native values still originate only from authenticated session replies.
    pub fn compile_task(
        &self,
        source: &str,
        entry: &str,
    ) -> Result<CompiledCatalogProgram<Node, NativeType>, String> {
        if source.len() > 16 * 1024 {
            return Err("source limit".into());
        }
        let parsed = parse(source);
        if !parsed.diagnostics.is_empty() {
            return Err("syntax rejected".into());
        }
        let declarations: Vec<_> = parsed.function.iter().chain(&parsed.helpers).collect();
        compile_catalog_typed_program::<Field, _, (), (), _, _, _, _>(
            &declarations,
            entry,
            COMPILATION,
            &self.source(),
            self,
        )
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
            return Err(Error::Native("stale or foreign dispatch"));
        }
        self.dispatched.set(work.identity.serial);
        let reply = match work.operation() {
            "gui.list" => {
                fuel.charge(1).map_err(|_| Error::Fuel)?;
                let names = self.tracked.gui.list()?;
                if names.len() > WIDGET_COUNT {
                    return Err(Error::Native("discovery exceeds host bounds"));
                }
                let mut list = StringListBuilder::with_capacity(names.len())
                    .map_err(|_| Error::Native("discovery exceeds language bounds"))?;
                for name in names {
                    fuel.charge(1 + name.len() as u64)
                        .map_err(|_| Error::Fuel)?;
                    list.try_push(name)
                        .map_err(|_| Error::Native("discovery exceeds language bounds"))?;
                }
                let value = list.finish();
                if !valid_identifiers(&value) {
                    return Err(Error::Native(
                        "discovery identifiers must be unique and nonempty",
                    ));
                }
                Reply::Identifiers(value)
            }
            "gui.find" => {
                let Some(PureValue::Scalar(ScalarValue::String(name))) =
                    work.call.arguments().first().map(|arg| &arg.value)
                else {
                    return Err(Error::Native("missing selector"));
                };
                fuel.charge(1 + name.len() as u64)
                    .map_err(|_| Error::Fuel)?;
                let slot = self.tracked.gui.find(name)?;
                self.refresh(slot)?;
                let object = self.owner(slot).reference(&self.lease)?;
                *work.call.schema().result.expected.borrow_mut() = Some(object.clone());
                Reply::Object(object)
            }
            "gui.observe" => {
                let target = work
                    .identity
                    .target
                    .as_ref()
                    .ok_or(Error::Native("missing target"))?;
                fuel.charge(1).map_err(|_| Error::Fuel)?;
                let slot = self.resolve(target)?;
                let data = self.refresh(slot)?;
                fuel.charge(snapshot_copy_cost(&data))
                    .map_err(|_| Error::Fuel)?;
                Reply::Snapshot(Rc::new(self.owner(slot).observe(
                    target,
                    &self.lease,
                    data,
                )?))
            }
            "gui.click" | "gui.set_text" => {
                let target = work
                    .identity
                    .target
                    .as_ref()
                    .ok_or(Error::Native("missing target"))?;
                let slot = self.resolve(target)?;
                let data = self.refresh(slot)?;
                let Some(PureValue::Scalar(ScalarValue::Integer(expected))) =
                    work.call.arguments().get(1).map(|arg| &arg.value)
                else {
                    return Err(Error::Native("missing revision"));
                };
                // Admission inspects borrowed operands; the queued String moves only
                // after bounds and the original Fuel debit have succeeded.
                let text = if work.operation() == "gui.set_text" {
                    let Some(PureValue::Scalar(ScalarValue::String(text))) =
                        work.call.arguments().get(2).map(|arg| &arg.value)
                    else {
                        return Err(Error::Native("missing text"));
                    };
                    if text.len() > MAX_SCALAR_STRING_BYTES {
                        return Err(Error::Native("text exceeds language bounds"));
                    }
                    Some(text)
                } else {
                    None
                };
                let cost = 1 + text.map_or(0, |text| text.len() as u64);
                fuel.charge(cost).map_err(|_| Error::Fuel)?;
                let expected = *expected;
                let action = if text.is_some() {
                    let (_, mut arguments) = work.call.into_parts();
                    let PureValue::Scalar(ScalarValue::String(text)) =
                        arguments.pop().ok_or(Error::Native("missing text"))?.value
                    else {
                        return Err(Error::Native("missing text"));
                    };
                    Action::SetText(text)
                } else {
                    Action::Click
                };
                action.validate(&data)?;
                let attempt =
                    self.owner(slot)
                        .with_current(target, &self.lease, expected, || {
                            let mut pending = self.pending.borrow_mut();
                            if pending.is_some() {
                                return Err(Error::Native("host action queue is full"));
                            }
                            *pending = Some(PendingAction {
                                identity: work.identity.clone(),
                                slot,
                                revision: self.owner(slot).revision(),
                                native_identity: data.identity,
                                action,
                            });
                            Ok(())
                        })?;
                attempt.into_output()?;
                Reply::Submitted(true)
            }
            _ => return Err(Error::Native("unknown operation")),
        };
        Ok(Receipt {
            identity: work.identity,
            reply,
        })
    }
    /// Explicit queue processing calls AppKit's real target/action on the owning thread.
    /// No language poll, reply acceptance or automatic retry occurs here.
    pub fn pump(
        &self,
        receipt: Receipt,
        fuel: &mut Fuel,
    ) -> HostWorkOutcome<Result<Receipt, Error>> {
        let mut entered_native = false;
        let result = (|| {
            self.authorize(&receipt.identity)?;
            if !matches!(receipt.identity.operation, "gui.click" | "gui.set_text")
                || !matches!(receipt.reply, Reply::Submitted(true))
            {
                return Err(Error::Native("pump requires original submission receipt"));
            }
            let target = receipt
                .identity
                .target
                .as_ref()
                .ok_or(Error::Native("missing target"))?;
            let slot = self.resolve(target)?;
            let data = self.refresh(slot)?;
            let mut pending = self.pending.borrow_mut();
            if !pending
                .as_ref()
                .is_some_and(|update| update.identity == receipt.identity && update.slot == slot)
            {
                return Err(Error::Native("no matching pending action"));
            }
            let update = pending
                .take()
                .ok_or(Error::Native("pending action vanished"))?;
            drop(pending);
            if update.native_identity != data.identity {
                return Err(Error::Native("native object was replaced"));
            }
            update.action.validate(&data)?;
            fuel.charge(update.action.cost()).map_err(|_| Error::Fuel)?;
            let attempt =
                self.owner(slot)
                    .with_current(target, &self.lease, update.revision, || {
                        entered_native = true;
                        match update.action {
                            Action::Click => self.tracked.gui.click(slot, update.native_identity),
                            Action::SetText(text) => {
                                self.tracked
                                    .gui
                                    .set_text(slot, update.native_identity, &text)
                            }
                        }
                    })?;
            attempt.into_output()?;
            // Capture real post-action state; success alone is not a language observation.
            self.refresh(slot)?;
            Ok(receipt)
        })();
        match result {
            Ok(value) => HostWorkOutcome::Pending(Ok(value)),
            Err(error) if entered_native => HostWorkOutcome::Uncertain(Err(error)),
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
            return Err(Error::Native("foreign or closed execution"));
        }
        self.tracked.require_open()?;
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
            Field::Identifier | Field::Role | Field::Value | Field::Title => ScalarType::String,
            Field::Editable => ScalarType::Boolean,
            Field::Clicks => ScalarType::Integer,
            Field::Enabled => ScalarType::Boolean,
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
        matches!(*operation, "gui.observe" | "gui.click" | "gui.set_text")
            && parameter_name == "target"
            && *result == NativeType::Object
    }
    fn call_result_type(
        &self,
        _: &&'static str,
        domain: &'s Domain<'s>,
    ) -> Option<PureType<NativeType>> {
        Some(match domain.kind {
            ResultKind::Identifiers => PureType::Scalar(ScalarType::StringList),
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
    fn source_type_name(&self, name: &str) -> Option<NativeType> {
        match name {
            "object" => Some(NativeType::Object),
            "snapshot" => Some(NativeType::Snapshot),
            _ => None,
        }
    }
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
            "identifier" => Some((Field::Identifier, ScalarType::String)),
            "role" => Some((Field::Role, ScalarType::String)),
            "value" => Some((Field::Value, ScalarType::String)),
            "editable" => Some((Field::Editable, ScalarType::Boolean)),
            "revision" => Some((Field::Revision, ScalarType::Integer)),
            "title" => Some((Field::Title, ScalarType::String)),
            "clicks" => Some((Field::Clicks, ScalarType::Integer)),
            "enabled" => Some((Field::Enabled, ScalarType::Boolean)),
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
            return Err(Error::Native("object has no scalar fields"));
        };
        let slot = self.resolve(observed.object())?;
        let current = self.refresh(slot)?;
        if observed.data().identity != current.identity
            || observed.data().identifier != current.identifier
            || observed.data().role != current.role
        {
            return Err(Error::Native(
                "snapshot payload belongs to another native instance",
            ));
        }
        validate_snapshot(observed.data())?;
        Ok(match field {
            Field::Identifier => ScalarValue::String(observed.data().identifier.clone()),
            Field::Role => ScalarValue::String(observed.data().role.clone()),
            Field::Value => ScalarValue::String(observed.data().value.clone()),
            Field::Editable => ScalarValue::Boolean(observed.data().editable),
            Field::Revision => ScalarValue::Integer(observed.revision()),
            Field::Title => ScalarValue::String(observed.data().title.clone()),
            Field::Clicks => ScalarValue::Integer(observed.data().clicks),
            Field::Enabled => ScalarValue::Boolean(observed.data().enabled),
        })
    }
    fn member(&self, _: &Self::Result, _: &str, _: &&'static str) -> Result<Self::Result, Error> {
        Err(Error::Native("no native groups"))
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
            return Err(Error::Native("foreign result domain").into());
        }
        domain.validate_value(&reply)?;
        Ok(match reply {
            Reply::Identifiers(value) => PureValue::Scalar(ScalarValue::StringList(value)),
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
            return Err(Error::Native("foreign schema").into());
        }
        let target = if matches!(schema.key, "gui.find" | "gui.list") {
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
