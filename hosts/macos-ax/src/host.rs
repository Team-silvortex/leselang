//! Shared typed local capabilities for one explicitly scoped remote target.
//! Remote snapshots detect observed changes; they cannot provide atomic compare-and-set
//! or detect an external A→B→A change that occurs entirely between observations.
//!
//! This module owns the public protocol, profile contracts and stable object
//! registry. Catalog/source admission and runtime session boundaries are separate
//! private modules; existing callers keep using the same `host` API.
pub(crate) mod catalog;
mod session;
#[cfg(test)]
mod tests;

use crate::{Backend, Error, Snapshot};
use catalog::{Catalog, Prepared};
use effect_evaluation::EffectEvaluationLimits;
use effect_stack::EffectStackLimits;
use host_protocol::{LocalObjectOwner, ObjectRef, Observation};
use leselang_vm::native::{core::*, hir::*};
use pure_evaluation::PureEvaluationLimits;
use std::{
    cell::{Cell, RefCell},
    fmt,
    rc::Rc,
};

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
const MAX_TRACKED_TARGETS: usize = 128;
struct TrackedEntry<S> {
    key: String,
    previous: Option<S>,
}
struct TrackedDocument<'b, S: ProfileSnapshot = Snapshot> {
    backend: &'b dyn Profile<Snapshot = S>,
    // Stable owner addresses survive inventory reordering, removal and additions.
    owners: [LocalObjectOwner; MAX_TRACKED_TARGETS],
    entries: RefCell<Vec<TrackedEntry<S>>>,
}
impl<'b, S: ProfileSnapshot> TrackedDocument<'b, S> {
    fn new(backend: &'b dyn Profile<Snapshot = S>) -> Result<Self, Error> {
        let tracked = Self {
            backend,
            owners: std::array::from_fn(|_| LocalObjectOwner::new()),
            entries: RefCell::new(Vec::new()),
        };
        tracked.refresh_all()?;
        Ok(tracked)
    }
    fn retire_all(&self) {
        for owner in &self.owners {
            owner.retire();
        }
    }
    fn refresh_all(&self) -> Result<Vec<(usize, S)>, Error> {
        let result = (|| {
            self.backend.check_permission()?;
            let snapshots = self.backend.snapshots()?;
            if (!S::CONTROLS && snapshots.len() != 1) || snapshots.len() > 64 {
                return Err(Error::Bounds);
            }
            for (index, snapshot) in snapshots.iter().enumerate() {
                snapshot.validate()?;
                if snapshot.key().is_empty()
                    || snapshot.key().len() > MAX_SCALAR_STRING_BYTES
                    || snapshots[..index]
                        .iter()
                        .any(|old| old.key() == snapshot.key())
                {
                    return Err(Error::Invalid("invalid or duplicate discovered key"));
                }
            }
            let mut entries = self.entries.borrow_mut();
            for (slot, entry) in entries.iter_mut().enumerate() {
                let found = snapshots
                    .iter()
                    .find(|snapshot| snapshot.key() == entry.key);
                match (&entry.previous, found) {
                    (Some(previous), Some(current)) => {
                        if !previous.same_instance(current) {
                            self.owners[slot].retire();
                            return Err(Error::Stale);
                        }
                        if previous != current {
                            self.owners[slot].advance_revision()?;
                        }
                        entry.previous = Some(current.clone());
                    }
                    (Some(_), None) => {
                        self.owners[slot].retire();
                        entry.previous = None;
                    }
                    (None, Some(_)) => {
                        return Err(Error::Invalid("a retired discovery key was reused"));
                    }
                    (None, None) => (),
                }
            }
            for snapshot in snapshots {
                if !entries.iter().any(|entry| entry.key == snapshot.key()) {
                    if entries.len() == MAX_TRACKED_TARGETS {
                        return Err(Error::Bounds);
                    }
                    entries.push(TrackedEntry {
                        key: snapshot.key().into(),
                        previous: Some(snapshot),
                    });
                }
            }
            Ok(entries
                .iter()
                .enumerate()
                .filter_map(|(slot, entry)| {
                    entry
                        .previous
                        .as_ref()
                        .map(|snapshot| (slot, snapshot.clone()))
                })
                .collect())
        })();
        if result.is_err() {
            self.retire_all();
        }
        result
    }
    fn refresh(&self) -> Result<S, Error> {
        self.refresh_slot(0)
    }
    fn refresh_slot(&self, slot: usize) -> Result<S, Error> {
        self.refresh_all()?
            .into_iter()
            .find(|(index, _)| *index == slot)
            .map(|(_, snapshot)| snapshot)
            .ok_or(Error::Unavailable(
                "the target is no longer in the scoped inventory",
            ))
    }
    /// Validate the observed object's identity against one current refresh,
    /// then validate the historical payload without replacing it. Both reply
    /// acceptance and scalar projection use this same authority boundary.
    fn validate_observation(&self, slot: usize, observed: &Observation<S>) -> Result<(), Error> {
        let current = self.refresh_slot(slot)?;
        if !observed.data().same_instance(&current) {
            return Err(Error::Invalid(
                "snapshot payload belongs to another native instance",
            ));
        }
        observed.data().validate()
    }
    fn resolve(&self, target: &ObjectRef, lease: &LocalExecutionLease) -> Result<usize, Error> {
        let count = self.entries.borrow().len();
        for slot in 0..count {
            match self.owners[slot].validate(target, lease) {
                Err(host_protocol::ObjectError::ForeignOwner) => continue,
                result => {
                    result?;
                    self.refresh_slot(slot)?;
                    self.owners[slot].validate(target, lease)?;
                    return Ok(slot);
                }
            }
        }
        Err(host_protocol::ObjectError::ForeignOwner.into())
    }
}
/// Scalar schema and immutable identity checks for one scoped native capability.
/// The external transport still owns its concrete state and immediate recheck.
/// Scalar reply policy for the shared adapter implementation. Backends normally
/// implement `Backend` or `ControlsBackend`, rather than this machinery.
pub trait ReplyPolicy {
    type ScalarReply: ScalarReplyPayload;
}
pub trait ScalarReplyPayload: Sized {
    fn submitted() -> Self;
    fn keys_value(keys: StringListValue) -> Result<Self, Error>;
    fn as_keys(&self) -> Option<&StringListValue>;
    fn is_submitted(&self) -> bool;
    fn into_scalar(self) -> ScalarValue;
}
impl ScalarReplyPayload for bool {
    fn submitted() -> Self {
        true
    }
    fn keys_value(_: StringListValue) -> Result<Self, Error> {
        Err(Error::Invalid("this profile has no discovery list"))
    }
    fn as_keys(&self) -> Option<&StringListValue> {
        None
    }
    fn is_submitted(&self) -> bool {
        *self
    }
    fn into_scalar(self) -> ScalarValue {
        ScalarValue::Boolean(self)
    }
}
impl ReplyPolicy for Snapshot {
    type ScalarReply = bool;
}
impl ReplyPolicy for crate::calculator::CalculatorSnapshot {
    type ScalarReply = bool;
}

/// Typed payload retained between explicit submission and delivery. Native item
/// references are checked before both boundaries and are never wire handles.
#[derive(Clone, Debug)]
pub enum ActionArgument {
    None,
    Text(String),
    Boolean(bool),
    Integer(u64),
    Item { reference: ObjectRef, identity: u64 },
}
impl ActionArgument {
    fn cost(&self) -> u64 {
        match self {
            Self::Text(value) => 1 + value.len() as u64,
            _ => 1,
        }
    }
    fn legacy_text(&self) -> Result<&str, Error> {
        match self {
            Self::None => Ok(""),
            Self::Text(value) => Ok(value),
            _ => Err(Error::Invalid("this operation requires a text payload")),
        }
    }
}

pub trait ProfileSnapshot: Clone + Eq + fmt::Debug + ReplyPolicy + 'static {
    const SELECTOR: &'static str;
    const OPERATION: &'static str;
    const ARGUMENT: &'static str;
    const CONTROLS: bool = false;
    const WIDGETS: bool = false;
    fn native_identity(&self) -> Option<u64> {
        None
    }
    fn is_mutation(operation: &str) -> bool {
        operation == Self::OPERATION
            || (Self::CONTROLS && operation == "gui.press")
            || (Self::WIDGETS
                && matches!(
                    operation,
                    "gui.open_menu"
                        | "gui.choose_menu_item"
                        | "gui.cancel_menu"
                        | "gui.set_checked"
                        | "gui.select_item"
                        | "gui.scroll"
                ))
    }
    fn admit_typed_operation(
        &self,
        operation: &str,
        argument: &ActionArgument,
    ) -> Result<(), Error> {
        self.admit_operation(operation, argument.legacy_text()?)
    }
    fn key(&self) -> &str {
        Self::SELECTOR
    }
    fn selector_matches(&self, _role: &str, _identifier: &str, _label: &str) -> bool {
        false
    }
    fn value_selector_matches(&self, _role: &str, _value: &str) -> bool {
        false
    }
    fn admit_operation(&self, operation: &str, argument: &str) -> Result<(), Error> {
        if operation != Self::OPERATION {
            return Err(Error::Invalid("operation does not belong to this profile"));
        }
        self.admit_action(argument)
    }
    fn validate(&self) -> Result<(), Error>;
    fn same_instance(&self, other: &Self) -> bool;
    fn copy_cost(&self) -> u64;
    fn describe(&self) -> String;
    fn field_type(field: Field) -> Option<ScalarType>;
    fn field(&self, field: Field) -> Result<ScalarValue, Error>;
    fn admit_action(&self, argument: &str) -> Result<(), Error>;
}

pub(crate) trait Profile {
    type Snapshot: ProfileSnapshot;
    fn check_permission(&self) -> Result<(), Error>;
    fn snapshot(&self) -> Result<Self::Snapshot, Error>;
    fn source_wait_state<'a>(
        &self,
        _snapshot: &'a Self::Snapshot,
        _kind: SourceWaitKind,
    ) -> Result<SourceWaitState<'a>, Error> {
        Err(Error::Invalid(
            "typed source wait is unavailable in this profile",
        ))
    }
    fn snapshots(&self) -> Result<Vec<Self::Snapshot>, Error> {
        Ok(vec![self.snapshot()?])
    }
    fn apply_operation(
        &self,
        expected: &Self::Snapshot,
        operation: &str,
        argument: &str,
    ) -> Result<(), Error> {
        if operation != Self::Snapshot::OPERATION {
            return Err(Error::Invalid("operation does not belong to this profile"));
        }
        self.apply(expected, argument)
    }
    fn apply_typed_operation(
        &self,
        expected: &Self::Snapshot,
        operation: &str,
        argument: &ActionArgument,
    ) -> Result<(), Error> {
        self.apply_operation(expected, operation, argument.legacy_text()?)
    }
    fn apply(&self, expected: &Self::Snapshot, argument: &str) -> Result<(), Error>;
}
struct DocumentProfile<'b>(&'b dyn Backend);
impl Profile for DocumentProfile<'_> {
    type Snapshot = Snapshot;
    fn check_permission(&self) -> Result<(), Error> {
        self.0.check_permission()
    }
    fn snapshot(&self) -> Result<Snapshot, Error> {
        self.0.snapshot()
    }
    fn apply(&self, expected: &Snapshot, text: &str) -> Result<(), Error> {
        self.0.set_text(expected, text)
    }
}
impl ProfileSnapshot for Snapshot {
    const SELECTOR: &'static str = "document";
    const OPERATION: &'static str = "gui.set_text";
    const ARGUMENT: &'static str = "text";
    fn validate(&self) -> Result<(), Error> {
        if [&self.document, &self.role, &self.value]
            .iter()
            .any(|s| s.len() > MAX_SCALAR_STRING_BYTES)
        {
            return Err(Error::Bounds);
        }
        if self.identity == 0 || self.document.is_empty() || self.role != "AXTextArea" {
            return Err(Error::Invalid("invalid remote snapshot metadata"));
        }
        Ok(())
    }
    fn same_instance(&self, other: &Self) -> bool {
        self.identity == other.identity
            && self.document == other.document
            && self.role == other.role
    }
    fn copy_cost(&self) -> u64 {
        (self.document.len() + self.role.len() + self.value.len()) as u64
    }
    fn describe(&self) -> String {
        format!(
            "document={:?}, role={:?}, value={:?}",
            self.document, self.role, self.value
        )
    }
    fn field_type(field: Field) -> Option<ScalarType> {
        match field {
            Field::Document | Field::Role | Field::Value => Some(ScalarType::String),
            Field::Editable => Some(ScalarType::Boolean),
            Field::Revision => Some(ScalarType::Integer),
            _ => None,
        }
    }
    fn field(&self, field: Field) -> Result<ScalarValue, Error> {
        Ok(match field {
            Field::Document => ScalarValue::String(self.document.clone()),
            Field::Role => ScalarValue::String(self.role.clone()),
            Field::Value => ScalarValue::String(self.value.clone()),
            Field::Editable => ScalarValue::Boolean(self.editable),
            _ => {
                return Err(Error::Invalid(
                    "field is unavailable in the document profile",
                ));
            }
        })
    }
    fn admit_action(&self, argument: &str) -> Result<(), Error> {
        if argument.len() > MAX_SCALAR_STRING_BYTES {
            return Err(Error::Bounds);
        }
        if !self.editable {
            return Err(Error::NotEditable);
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Field {
    Document,
    Role,
    Value,
    Editable,
    Revision,
    Display,
    Buttons,
    LastExpression,
    Key,
    Identifier,
    Label,
    Enabled,
    Pressable,
    Actions,
    CheckState,
    Selected,
    SelectionAvailable,
    SelectedKeys,
    Position,
    PositionAvailable,
    Orientation,
    ParentKey,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NativeType {
    Object,
    Snapshot,
}
#[derive(Clone, Copy)]
enum ResultKind {
    Keys,
    Object,
    Snapshot,
    WaitSnapshot,
    Submitted,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SourceWaitKind {
    Value,
    Checked,
    Selected,
    Position,
}
impl SourceWaitKind {
    pub(crate) const fn operation(self) -> &'static str {
        match self {
            Self::Value => "gui.wait_value",
            Self::Checked => "gui.wait_checked",
            Self::Selected => "gui.wait_selected",
            Self::Position => "gui.wait_position",
        }
    }
    pub(crate) fn from_operation(operation: &str) -> Option<Self> {
        [Self::Value, Self::Checked, Self::Selected, Self::Position]
            .into_iter()
            .find(|kind| kind.operation() == operation)
    }
    pub(crate) const fn expected_type(self) -> ScalarType {
        match self {
            Self::Value => ScalarType::OptionalString,
            Self::Checked | Self::Selected => ScalarType::Boolean,
            Self::Position => ScalarType::Integer,
        }
    }
    pub(crate) fn admits_expected(self, expected: &ScalarValue) -> bool {
        expected.is_bounded()
            && expected.scalar_type() == self.expected_type()
            && (self != Self::Position || matches!(expected, ScalarValue::Integer(0..=1000)))
    }
}
#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum SourceWaitCatalog {
    None,
    Values,
    States,
}
impl SourceWaitCatalog {
    fn kinds(self) -> &'static [SourceWaitKind] {
        match self {
            Self::None => &[],
            Self::Values => &[SourceWaitKind::Value],
            Self::States => &[
                SourceWaitKind::Value,
                SourceWaitKind::Checked,
                SourceWaitKind::Selected,
                SourceWaitKind::Position,
            ],
        }
    }
    fn contains(self, operation: &str) -> bool {
        self.kinds()
            .iter()
            .any(|kind| kind.operation() == operation)
    }
}
pub(crate) struct SourceWaitState<'a> {
    // Mixed checkbox state has no Boolean value and matches neither Boolean.
    pub(crate) value: Option<ScalarValue>,
    pub(crate) selection_parent: Option<(u64, &'a str)>,
}
pub(crate) struct SourceWaitParameters {
    pub(crate) kind: SourceWaitKind,
    pub(crate) expected: ScalarValue,
    pub(crate) max_attempts: u64,
    pub(crate) interval_ms: u64,
    pub(crate) timeout_ms: u64,
}
struct SourceWaitRequest<S: ProfileSnapshot> {
    serial: u64,
    parameters: SourceWaitParameters,
    // Completion is proof of this exact sampled allocation, not merely a
    // matching historical value that a public receipt could substitute.
    ready: Option<Rc<Observation<S>>>,
    selection_parent: Option<(u64, String)>,
}
/// Concrete TextEdit reply type, preserving constructor inference.
pub type Reply = ProfileReply<Snapshot>;
/// Concrete TextEdit native value type, preserving constructor inference.
pub type NativeValue = ProfileNativeValue<Snapshot>;
/// Concrete TextEdit receipt type.
pub type Receipt = ProfileReceipt<Snapshot>;
pub enum ProfileReply<S: ProfileSnapshot, P: ScalarReplyPayload = <S as ReplyPolicy>::ScalarReply> {
    Object(ObjectRef),
    Snapshot(Rc<Observation<S>>),
    Submitted(P),
}
impl<S: ProfileSnapshot, P: ScalarReplyPayload> ProfileReply<S, P> {
    pub fn is_submission(&self) -> bool {
        matches!(self, Self::Submitted(value) if value.is_submitted())
    }
}
impl<S: ProfileSnapshot, P: ScalarReplyPayload> fmt::Debug for ProfileReply<S, P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Object(_) => "Object",
            Self::Snapshot(_) => "Snapshot",
            Self::Submitted(value) if value.as_keys().is_some() => "Keys",
            Self::Submitted(_) => "Submitted",
        })
    }
}
pub struct Domain<'a, S: ProfileSnapshot = Snapshot> {
    tracked: &'a TrackedDocument<'a, S>,
    lease: LocalExecutionLease,
    kind: ResultKind,
    expected: RefCell<Option<ObjectRef>>,
    source_wait: RefCell<Option<SourceWaitRequest<S>>>,
}
impl<S: ProfileSnapshot> Domain<'_, S> {
    fn validate_snapshot(&self, value: &Observation<S>) -> Result<(), Error> {
        let slot = self.validate_object(value.object())?;
        self.tracked.validate_observation(slot, value)
    }
    fn validate_object(&self, actual: &ObjectRef) -> Result<usize, Error> {
        let expected = self.expected.borrow();
        let expected = expected
            .as_ref()
            .ok_or(Error::Invalid("no requested native target"))?;
        let slot = self.tracked.resolve(expected, &self.lease)?;
        self.tracked.owners[slot].validate(actual, &self.lease)?;
        Ok(slot)
    }
}
impl<S: ProfileSnapshot> HostResultDomain<ProfileReply<S>> for Domain<'_, S> {
    type Error = Error;
    fn matches_type(&self, reply: &ProfileReply<S>) -> bool {
        matches!(
            (self.kind, reply),
            (ResultKind::Keys, ProfileReply::Submitted(_))
                | (ResultKind::Object, ProfileReply::Object(_))
                | (ResultKind::Snapshot, ProfileReply::Snapshot(_))
                | (ResultKind::WaitSnapshot, ProfileReply::Snapshot(_))
                | (ResultKind::Submitted, ProfileReply::Submitted(_))
        )
    }
    fn validate_value(&self, reply: &ProfileReply<S>) -> Result<(), Error> {
        match (self.kind, reply) {
            (ResultKind::Keys, ProfileReply::Submitted(value))
                if S::CONTROLS
                    && value.as_keys().is_some_and(|value| {
                        value.is_bounded()
                            && value.0.len() <= 64
                            && value
                                .0
                                .iter()
                                .enumerate()
                                .all(|(i, key)| !key.is_empty() && !value.0[..i].contains(key))
                    }) =>
            {
                Ok(())
            }
            (ResultKind::Object, ProfileReply::Object(value)) => {
                self.validate_object(value).map(|_| ())
            }
            (ResultKind::Snapshot, ProfileReply::Snapshot(value)) => self.validate_snapshot(value),
            (ResultKind::WaitSnapshot, ProfileReply::Snapshot(value)) => {
                if !self.source_wait.borrow().as_ref().is_some_and(|request| {
                    request
                        .ready
                        .as_ref()
                        .is_some_and(|ready| Rc::ptr_eq(ready, value))
                }) {
                    return Err(Error::Invalid(
                        "source wait has not admitted this observation",
                    ));
                }
                self.validate_snapshot(value)
            }
            (ResultKind::Submitted, ProfileReply::Submitted(value)) if value.is_submitted() => {
                Ok(())
            }
            _ => Err(Error::Invalid(
                "reply does not match original result domain",
            )),
        }
    }
}
#[derive(Clone)]
pub enum ProfileNativeValue<S: ProfileSnapshot> {
    Object(ObjectRef),
    Snapshot(Rc<Observation<S>>),
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
pub struct Work<'s, S: ProfileSnapshot = Snapshot> {
    identity: Identity,
    call: Prepared<'s, S>,
}
impl<S: ProfileSnapshot> fmt::Debug for Work<'_, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Work")
    }
}
impl<S: ProfileSnapshot> Work<'_, S> {
    pub fn identity(&self) -> &Identity {
        &self.identity
    }
    pub fn operation(&self) -> &'static str {
        self.identity.operation
    }
    /// Whether this request needs explicit source-wait dispatch and stepping.
    pub fn is_source_wait(&self) -> bool {
        SourceWaitKind::from_operation(self.operation()).is_some()
    }
}
pub struct ProfileReceipt<S: ProfileSnapshot> {
    pub identity: Identity,
    pub reply: ProfileReply<S>,
}
impl<S: ProfileSnapshot> fmt::Debug for ProfileReceipt<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.reply.fmt(f)
    }
}
struct PendingAction<S: ProfileSnapshot> {
    identity: Identity,
    revision: u64,
    expected: S,
    argument: ActionArgument,
}
pub struct Host<'h, 's, S: ProfileSnapshot = Snapshot> {
    tracked: &'h TrackedDocument<'h, S>,
    catalog: &'h Catalog<'s, S>,
    lease: LocalExecutionLease,
    granted: Cell<bool>,
    serial: Cell<u64>,
    dispatched: Cell<u64>,
    observation_wait_serial: Cell<u64>,
    source_waits: SourceWaitCatalog,
    pending: RefCell<Option<PendingAction<S>>>,
}
/// The catalog and its native reply guards belong to exactly one execution.
pub fn with_host<T>(
    backend: &dyn Backend,
    use_host: impl FnOnce(&Host<'_, '_>, LocalExecutionScope) -> T,
) -> Result<T, Error> {
    with_profile(&DocumentProfile(backend), use_host)
}
pub(crate) fn with_profile<S: ProfileSnapshot, T>(
    backend: &dyn Profile<Snapshot = S>,
    use_host: impl FnOnce(&Host<'_, '_, S>, LocalExecutionScope) -> T,
) -> Result<T, Error> {
    with_profile_options(backend, SourceWaitCatalog::None, use_host)
}
pub(crate) fn with_profile_waits<S: ProfileSnapshot, T>(
    backend: &dyn Profile<Snapshot = S>,
    use_host: impl FnOnce(&Host<'_, '_, S>, LocalExecutionScope) -> T,
) -> Result<T, Error> {
    with_profile_options(backend, SourceWaitCatalog::Values, use_host)
}
pub(crate) fn with_profile_state_waits<S: ProfileSnapshot, T>(
    backend: &dyn Profile<Snapshot = S>,
    use_host: impl FnOnce(&Host<'_, '_, S>, LocalExecutionScope) -> T,
) -> Result<T, Error> {
    with_profile_options(backend, SourceWaitCatalog::States, use_host)
}
fn with_profile_options<S: ProfileSnapshot, T>(
    backend: &dyn Profile<Snapshot = S>,
    source_waits: SourceWaitCatalog,
    use_host: impl FnOnce(&Host<'_, '_, S>, LocalExecutionScope) -> T,
) -> Result<T, Error> {
    if source_waits != SourceWaitCatalog::None && !S::WIDGETS {
        return Err(Error::Invalid("source waits require the widgets profile"));
    }
    let tracked = TrackedDocument::new(backend)?;
    let scope = LocalExecutionScope::new();
    let lease = scope.lease();
    catalog::with_catalog(&tracked, lease.clone(), source_waits, |catalog| {
        let host = Host {
            tracked: &tracked,
            catalog,
            lease,
            granted: Cell::new(true),
            serial: Cell::new(0),
            dispatched: Cell::new(0),
            observation_wait_serial: Cell::new(0),
            source_waits,
            pending: RefCell::new(None),
        };
        use_host(&host, scope)
    })
}
