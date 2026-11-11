use std::{
    cell::Cell,
    panic::{AssertUnwindSafe, catch_unwind},
};

use leselang_runtime_core::{
    LocalExecutionScope,
    host_protocol::{LocalObjectOwner, ObjectError, ProtocolVersion},
};

#[test]
fn protocol_version_accepts_only_the_exact_local_draft() {
    assert_eq!(ProtocolVersion::V1.number(), 1);
    assert_eq!(ProtocolVersion::from_number(1), Ok(ProtocolVersion::V1));
    for version in [0, 2, u32::MAX] {
        assert_eq!(
            ProtocolVersion::from_number(version).unwrap_err().requested,
            version
        );
    }
}

#[test]
fn opaque_references_reject_foreign_owners_and_execution_scopes() {
    let execution = LocalExecutionScope::new();
    let sibling = LocalExecutionScope::new();
    let owner = LocalObjectOwner::new();
    let foreign = LocalObjectOwner::new();
    let lease = execution.lease();
    let object = owner.reference(&lease).unwrap();
    assert_eq!(owner.validate(&object.clone(), &lease), Ok(()));
    assert_eq!(
        foreign.validate(&object, &lease),
        Err(ObjectError::ForeignOwner)
    );
    assert_eq!(
        owner.validate(&object, &sibling.lease()),
        Err(ObjectError::WrongExecution)
    );
    let calls = Cell::new(0);
    assert!(
        owner
            .with_current(&object, &sibling.lease(), 0, || calls.set(1))
            .is_err()
    );
    assert_eq!(calls.get(), 0);
    assert_eq!(owner.revision(), 0);
}

#[test]
fn closure_and_drop_revoke_execution_without_affecting_other_scopes() {
    let owner = LocalObjectOwner::new();
    let execution = LocalExecutionScope::new();
    let sibling = LocalExecutionScope::new();
    let lease = execution.lease();
    let object = owner.reference(&lease).unwrap();
    let other = owner.reference(&sibling.lease()).unwrap();
    assert!(execution.close());
    assert_eq!(
        owner.validate(&object, &lease),
        Err(ObjectError::ClosedExecution)
    );
    assert!(matches!(
        owner.reference(&lease),
        Err(ObjectError::ClosedExecution)
    ));
    assert!(
        owner
            .with_current(&object, &lease, 0, || panic!("closed execution"))
            .is_err()
    );
    assert_eq!(owner.validate(&other, &sibling.lease()), Ok(()));
    let sibling_lease = sibling.lease();
    drop(sibling);
    assert_eq!(
        owner.validate(&other, &sibling_lease),
        Err(ObjectError::ClosedExecution)
    );
}

#[test]
fn replacement_invalidates_old_references_even_when_revision_numbers_repeat() {
    let execution = LocalExecutionScope::new();
    let lease = execution.lease();
    let owner = LocalObjectOwner::new();
    let object = owner.reference(&lease).unwrap();
    let snapshot = owner.observe(&object, &lease, "historical").unwrap();
    owner.advance_revision().unwrap();
    owner.replace().unwrap();
    assert_eq!(owner.revision(), 0);
    assert_eq!(
        owner.validate(&object, &lease),
        Err(ObjectError::StaleGeneration)
    );
    assert!(matches!(
        owner.with_current(snapshot.object(), &lease, snapshot.revision(), || panic!(
            "replaced object"
        )),
        Err(ObjectError::StaleGeneration)
    ));
    let replacement = owner.reference(&lease).unwrap();
    assert_eq!(owner.validate(&replacement, &lease), Ok(()));
    assert_eq!(snapshot.data(), &"historical");
}

#[test]
fn retirement_never_revives_and_owner_drop_does_not_transfer_identity() {
    let execution = LocalExecutionScope::new();
    let lease = execution.lease();
    let owner = LocalObjectOwner::new();
    let object = owner.reference(&lease).unwrap();
    assert!(owner.retire());
    assert!(!owner.retire());
    assert_eq!(owner.replace(), Err(ObjectError::Retired));
    assert_eq!(owner.advance_revision(), Err(ObjectError::Retired));
    assert_eq!(owner.validate(&object, &lease), Err(ObjectError::Retired));
    assert!(matches!(owner.reference(&lease), Err(ObjectError::Retired)));
    drop(owner);
    let replacement = LocalObjectOwner::new();
    assert_eq!(
        replacement.validate(&object, &lease),
        Err(ObjectError::ForeignOwner)
    );
    assert!(lease.is_open());
}

#[test]
fn observation_data_moves_without_copying_and_is_historical_after_mutation() {
    let execution = LocalExecutionScope::new();
    let lease = execution.lease();
    let owner = LocalObjectOwner::new();
    let object = owner.reference(&lease).unwrap();
    let data = vec![1_u8, 2, 3].into_boxed_slice();
    let pointer = data.as_ptr();
    let snapshot = owner.observe(&object, &lease, data).unwrap();
    assert_eq!(snapshot.data().as_ptr(), pointer);
    assert_eq!(snapshot.revision(), 0);
    owner.advance_revision().unwrap();
    assert_eq!(snapshot.revision(), 0);
    assert_eq!(snapshot.data().as_ptr(), pointer);
    assert_eq!(owner.validate(snapshot.object(), &lease), Ok(()));
    execution.close();
    owner.retire();
    assert_eq!(snapshot.data().as_ref(), &[1, 2, 3]);
}

#[test]
fn current_revision_enters_once_and_stale_attempt_never_replays() {
    let execution = LocalExecutionScope::new();
    let lease = execution.lease();
    let owner = LocalObjectOwner::new();
    let object = owner.reference(&lease).unwrap();
    let value = Cell::new(7);
    let before = owner.observe(&object, &lease, value.get()).unwrap();
    let attempt = owner
        .with_current(before.object(), &lease, before.revision(), || {
            assert_eq!(owner.revision(), 1, "revision must fence before mutation");
            value.set(9);
            "submitted"
        })
        .unwrap();
    assert_eq!(attempt.revision(), 1);
    assert_eq!(attempt.into_output(), "submitted");
    assert!(matches!(
        owner.with_current(before.object(), &lease, before.revision(), || value.set(42)),
        Err(ObjectError::StaleRevision)
    ));
    assert_eq!(value.get(), 9);
    assert_eq!(owner.revision(), 1);
    assert_eq!(*before.data(), 7);
    let after = owner.observe(&object, &lease, value.get()).unwrap();
    assert_eq!(*after.data(), 9);
    assert_eq!(after.revision(), 1);
}

#[test]
fn callback_failure_and_unwind_preserve_partial_effects_and_advanced_revision() {
    let execution = LocalExecutionScope::new();
    let lease = execution.lease();
    let owner = LocalObjectOwner::new();
    let object = owner.reference(&lease).unwrap();
    let mutations = Cell::new(0);
    let failed = owner
        .with_current(&object, &lease, 0, || {
            mutations.set(1);
            Err::<(), _>("native failure")
        })
        .unwrap();
    assert_eq!(failed.revision(), 1);
    assert_eq!(failed.into_output(), Err("native failure"));
    assert_eq!(mutations.get(), 1);
    assert_eq!(owner.revision(), 1);
    assert!(
        catch_unwind(AssertUnwindSafe(|| owner.with_current(
            &object,
            &lease,
            1,
            || {
                mutations.set(2);
                panic!("after native mutation");
            }
        )))
        .is_err()
    );
    assert_eq!(mutations.get(), 2);
    assert_eq!(owner.revision(), 2);
    assert!(matches!(
        owner.with_current(&object, &lease, 1, || panic!("no replay")),
        Err(ObjectError::StaleRevision)
    ));
    // Only the enclosing VM owns session sealing; this primitive adds no fence.
    assert!(lease.is_open());
}

#[test]
fn external_changes_and_reentrant_callbacks_can_supersede_an_admission() {
    let execution = LocalExecutionScope::new();
    let lease = execution.lease();
    let owner = LocalObjectOwner::new();
    let object = owner.reference(&lease).unwrap();
    let before = owner.observe(&object, &lease, ()).unwrap();
    owner.advance_revision().unwrap();
    assert!(matches!(
        owner.with_current(&object, &lease, before.revision(), || panic!(
            "stale external event"
        )),
        Err(ObjectError::StaleRevision)
    ));
    let attempt = owner
        .with_current(&object, &lease, 1, || {
            owner.advance_revision().unwrap();
            "native output"
        })
        .unwrap();
    assert_eq!(attempt.revision(), 2);
    assert_eq!(owner.revision(), 3);
    assert_eq!(attempt.into_output(), "native output");
}

#[test]
fn metadata_debug_never_requires_or_formats_native_payloads() {
    struct Private<'a>(&'a Cell<usize>);
    impl Drop for Private<'_> {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    let drops = Cell::new(0);
    let execution = LocalExecutionScope::new();
    let lease = execution.lease();
    let owner = LocalObjectOwner::new();
    let object = owner.reference(&lease).unwrap();
    let observed = owner.observe(&object, &lease, Private(&drops)).unwrap();
    let action = owner
        .with_current(&object, &lease, 0, || Private(&drops))
        .unwrap();
    assert_eq!(format!("{object:?}"), "ObjectRef");
    assert_eq!(format!("{observed:?}"), "Observation { revision: 0, .. }");
    assert_eq!(format!("{action:?}"), "ActionAttempt { revision: 1, .. }");
    assert_eq!(drops.get(), 0);
    drop((observed, action.into_output()));
    assert_eq!(drops.get(), 2);
}
