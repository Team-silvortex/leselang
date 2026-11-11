//! Local object identity and revision fences for explicitly adapted hosts.
//!
//! This draft protocol is independent of any GUI toolkit, command runner or
//! transport. Objects remain native host resources. These types add no object
//! registry, scheduler, callback retry, persistence or VM. Live capabilities,
//! schema admission, observation bounds and actual side effects belong to the
//! adapter; its existing local VM remains the execution and unwind fence.

use std::{
    cell::Cell,
    fmt,
    rc::{Rc, Weak},
};

use crate::LocalExecutionLease;

/// Exact local draft protocol identity, not package version or wire negotiation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProtocolVersion(u32);

impl ProtocolVersion {
    pub const V1: Self = Self(1);

    pub const fn number(self) -> u32 {
        self.0
    }

    /// Accept exactly the supported local protocol version without fallback.
    pub const fn from_number(number: u32) -> Result<Self, UnsupportedProtocolVersion> {
        if number == Self::V1.number() {
            Ok(Self::V1)
        } else {
            Err(UnsupportedProtocolVersion { requested: number })
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnsupportedProtocolVersion {
    pub requested: u32,
}

impl fmt::Display for UnsupportedProtocolVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "unsupported local host protocol version {}",
            self.requested
        )
    }
}

impl std::error::Error for UnsupportedProtocolVersion {}

struct ObjectState {
    generation: Cell<u64>,
    revision: Cell<u64>,
    retired: Cell<bool>,
}

/// Unique lifetime owner for one host-managed resource or replaceable native slot.
///
/// This owns identity metadata, not the native resource. Keep it in that resource's
/// owner, invalidate it before replacement, and retire it before native teardown.
/// Drop retires it permanently. Object references are weak and cannot keep either
/// this owner or an execution scope alive. This local owner is neither Clone nor
/// Send/Sync; it does not affect the auto-traits of the unwrapped VM or host types.
///
/// All adapter-visible changes must advance the revision, including external GUI
/// events. The library cannot observe native mutations through other aliases.
/// A revision is an optimistic concurrency condition, never object identity or
/// permission to act. Capability checks remain explicit at the effect boundary.
///
/// ```compile_fail
/// use leselang_runtime_core::host_protocol::LocalObjectOwner;
/// let owner = LocalObjectOwner::new();
/// let duplicate = owner.clone();
/// ```
#[must_use = "retain the object owner until native retirement"]
pub struct LocalObjectOwner {
    state: Rc<ObjectState>,
}

impl LocalObjectOwner {
    pub fn new() -> Self {
        Self {
            state: Rc::new(ObjectState {
                generation: Cell::new(1),
                revision: Cell::new(0),
                retired: Cell::new(false),
            }),
        }
    }

    /// Issue a reference bound to this generation and one open execution scope.
    /// This does not grant capabilities or reserve any native work.
    pub fn reference(&self, lease: &LocalExecutionLease) -> Result<ObjectRef, ObjectError> {
        self.require_live()?;
        if !lease.is_open() {
            return Err(ObjectError::ClosedExecution);
        }
        Ok(ObjectRef {
            owner: Rc::downgrade(&self.state),
            generation: self.state.generation.get(),
            lease: lease.clone(),
        })
    }

    /// Check exact owner, live object, execution scope and current generation.
    /// No revision check, native callback, mutation or language fuel is implied.
    /// Keep the same execution lease used by the owning LocalVm at every boundary.
    pub fn validate(
        &self,
        object: &ObjectRef,
        current_lease: &LocalExecutionLease,
    ) -> Result<(), ObjectError> {
        if !Weak::ptr_eq(&object.owner, &Rc::downgrade(&self.state)) {
            return Err(ObjectError::ForeignOwner);
        }
        self.require_live()?;
        if !object.lease.same_scope(current_lease) {
            return Err(ObjectError::WrongExecution);
        }
        if !object.lease.is_open() || !current_lease.is_open() {
            return Err(ObjectError::ClosedExecution);
        }
        if object.generation != self.state.generation.get() {
            return Err(ObjectError::StaleGeneration);
        }
        Ok(())
    }

    /// Read concurrency metadata, including after retirement; this is not liveness.
    pub fn revision(&self) -> u64 {
        self.state.revision.get()
    }

    /// Invalidate all existing references before replacing the native resource.
    /// The new generation starts at revision zero. Failure changes nothing, so
    /// the adapter must not perform replacement after counter exhaustion.
    pub fn replace(&self) -> Result<(), ObjectError> {
        self.require_live()?;
        let generation = self
            .state
            .generation
            .get()
            .checked_add(1)
            .ok_or(ObjectError::GenerationExhausted)?;
        self.state.generation.set(generation);
        self.state.revision.set(0);
        Ok(())
    }

    /// Record a relevant state change before exposing the updated state.
    /// Advance before host-controlled mutation; on failure do not perform it.
    /// An external event may already have occurred when the adapter learns of
    /// it. If that event cannot be recorded, retire this owner and refuse further
    /// observations/actions instead of exposing changed state under an old
    /// revision. Failure changes no counter and cannot undo an external event.
    pub fn advance_revision(&self) -> Result<u64, ObjectError> {
        self.require_live()?;
        let revision = self
            .revision()
            .checked_add(1)
            .ok_or(ObjectError::RevisionExhausted)?;
        self.state.revision.set(revision);
        Ok(revision)
    }

    /// Permanently invalidate this owner; repeated retirement is inert.
    /// Native cleanup and cancellation delivery are the host's responsibility.
    pub fn retire(&self) -> bool {
        !self.state.retired.replace(true)
    }

    /// Attach current identity/revision metadata to already captured native data.
    ///
    /// This moves data without copying, reading or validating it. The adapter
    /// captures from the same resource and validates its schema, per-value and
    /// aggregate bounds before calling this method. It must avoid intervening
    /// native changes between capture and stamping. `T` may contain interior
    /// mutability: the host, not this wrapper, ensures a stable historical snapshot.
    /// A refused input is consumed; this is neither an authentic effect receipt nor
    /// a grant to resume a VM. Use the original reply acceptance protocol for that.
    pub fn observe<T>(
        &self,
        object: &ObjectRef,
        current_lease: &LocalExecutionLease,
        data: T,
    ) -> Result<Observation<T>, ObjectError> {
        self.validate(object, current_lease)?;
        Ok(Observation {
            object: object.clone(),
            revision: self.revision(),
            data,
        })
    }

    /// Admit one callback against the current object and expected revision.
    ///
    /// The revision advances **before** entry. A normal error output, partial
    /// side effect or escaping panic cannot reuse the old observation. Refusal
    /// never invokes the callback or changes the revision; no work is retried or
    /// rolled back. Callback outputs move unchanged, without trait bounds.
    ///
    /// This checks no capabilities and adds no scope/unwind fence or fuel meter.
    /// The caller checks current grants and runs this inside LocalVm's existing
    /// host-work boundary. The callback may reenter native code and supersede the
    /// admitted revision; ActionAttempt records only the admission, not external
    /// submission, completion or newly observed state. Observe again for state.
    pub fn with_current<T>(
        &self,
        object: &ObjectRef,
        current_lease: &LocalExecutionLease,
        expected_revision: u64,
        callback: impl FnOnce() -> T,
    ) -> Result<ActionAttempt<T>, ObjectError> {
        self.validate(object, current_lease)?;
        if self.revision() != expected_revision {
            return Err(ObjectError::StaleRevision);
        }
        let revision = self.advance_revision()?;
        let output = callback();
        Ok(ActionAttempt { revision, output })
    }

    fn require_live(&self) -> Result<(), ObjectError> {
        if self.state.retired.get() {
            Err(ObjectError::Retired)
        } else {
            Ok(())
        }
    }
}

impl Default for LocalObjectOwner {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for LocalObjectOwner {
    fn drop(&mut self) {
        self.retire();
    }
}

impl fmt::Debug for LocalObjectOwner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LocalObjectOwner")
            .field("retired", &self.state.retired.get())
            .field("generation", &self.state.generation.get())
            .field("revision", &self.revision())
            .finish()
    }
}

/// Opaque native reference bound to an owner, generation and execution lease.
/// Cloning retains weak identity only, never permission or a native resource.
/// There is no integer/string constructor, address getter, serde or wire codec.
///
/// ```compile_fail
/// use leselang_runtime_core::host_protocol::ObjectRef;
/// let object = ObjectRef::from(7_u64);
/// ```
///
/// ```compile_fail
/// use leselang_runtime_core::{LocalExecutionScope, host_protocol::LocalObjectOwner};
/// let scope = LocalExecutionScope::new();
/// let owner = LocalObjectOwner::new();
/// let object = owner.reference(&scope.lease()).unwrap();
/// let wire = serde_json::to_string(&object).unwrap();
/// ```
#[derive(Clone)]
#[must_use = "validate native identity and live policy before acting"]
pub struct ObjectRef {
    owner: Weak<ObjectState>,
    generation: u64,
    lease: LocalExecutionLease,
}

impl fmt::Debug for ObjectRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ObjectRef")
    }
}

/// Captured data and its observation revision, distinct from action admission.
/// Accessors expose historical data even after mutation, replacement or closure.
/// Live resolution/projection policies belong to the adapter; reading this record
/// is not evidence that the object or its fields still have the same live state.
#[must_use]
pub struct Observation<T> {
    object: ObjectRef,
    revision: u64,
    data: T,
}

impl<T> Observation<T> {
    pub fn object(&self) -> &ObjectRef {
        &self.object
    }
    pub const fn revision(&self) -> u64 {
        self.revision
    }
    pub const fn data(&self) -> &T {
        &self.data
    }
}

impl<T> fmt::Debug for Observation<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Observation")
            .field("revision", &self.revision)
            .finish_non_exhaustive()
    }
}

/// One locally admitted callback and its unchanged output, not an effect receipt.
/// The revision was assigned before callback entry and may already be superseded.
/// Neither constructing this output nor accepting a separate VM reply proves
/// external submission, completion, rollback or a new observation.
#[must_use]
pub struct ActionAttempt<T> {
    revision: u64,
    output: T,
}

impl<T> ActionAttempt<T> {
    pub const fn revision(&self) -> u64 {
        self.revision
    }
    pub fn into_output(self) -> T {
        self.output
    }
}

impl<T> fmt::Debug for ActionAttempt<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ActionAttempt")
            .field("revision", &self.revision)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectError {
    ForeignOwner,
    WrongExecution,
    ClosedExecution,
    Retired,
    StaleGeneration,
    StaleRevision,
    GenerationExhausted,
    RevisionExhausted,
}

impl fmt::Display for ObjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::ForeignOwner => "object reference belongs to another native owner",
            Self::WrongExecution => "object reference belongs to another execution",
            Self::ClosedExecution => "object reference execution lease is closed",
            Self::Retired => "native object owner is retired",
            Self::StaleGeneration => "object reference generation was replaced",
            Self::StaleRevision => "object observation revision is no longer current",
            Self::GenerationExhausted => "native object generation exhausted",
            Self::RevisionExhausted => "native object revision exhausted",
        })
    }
}

impl std::error::Error for ObjectError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LocalExecutionScope;

    #[test]
    fn revision_exhaustion_never_calls_or_wraps() {
        let scope = LocalExecutionScope::new();
        let lease = scope.lease();
        let owner = LocalObjectOwner::new();
        let object = owner.reference(&lease).unwrap();
        owner.state.revision.set(u64::MAX);
        assert_eq!(
            owner.advance_revision(),
            Err(ObjectError::RevisionExhausted)
        );
        let result = owner.with_current(&object, &lease, u64::MAX, || panic!("exhausted revision"));
        assert!(matches!(result, Err(ObjectError::RevisionExhausted)));
        assert_eq!(owner.revision(), u64::MAX);
    }

    #[test]
    fn generation_exhaustion_preserves_the_original_resource_identity() {
        let scope = LocalExecutionScope::new();
        let lease = scope.lease();
        let owner = LocalObjectOwner::new();
        owner.state.generation.set(u64::MAX);
        owner.state.revision.set(7);
        let object = owner.reference(&lease).unwrap();
        assert_eq!(owner.replace(), Err(ObjectError::GenerationExhausted));
        assert_eq!(owner.revision(), 7);
        assert_eq!(owner.validate(&object, &lease), Ok(()));
        assert!(owner.retire());
        assert_eq!(owner.validate(&object, &lease), Err(ObjectError::Retired));
    }

    #[test]
    fn references_cannot_keep_the_object_owner_alive() {
        let scope = LocalExecutionScope::new();
        let object = LocalObjectOwner::new().reference(&scope.lease()).unwrap();
        assert!(object.owner.upgrade().is_none());
        assert!(scope.is_open());
    }
}
