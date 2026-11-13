//! Explicit, bounded host-side postcondition observation. The caller chooses
//! which dispatched `gui.observe` to hold, or explicitly drives an opt-in
//! `gui.wait_value` whose condition and limits belong to the source request.
//! Nothing sleeps, schedules work, accepts a receipt or retries a mutation.
use crate::{
    Error,
    driver::Vm,
    host::{Field, Host, ProfileReceipt, ProfileReply, ProfileSnapshot, SourceWaitKind, Work},
};
use leselang_vm::native::{
    HostWorkOutcome,
    core::{Fuel, MAX_CLOCK_MS, ScalarValue},
    hir::{effect_session::EffectSessionStatus, pure_evaluation::scalar_copy_cost},
};
use std::{fmt, time::Duration};

pub const MAX_ATTEMPTS: u32 = 64;
pub const MAX_INTERVAL: Duration = Duration::from_secs(1);
pub const MAX_TIMEOUT: Duration = Duration::from_secs(30);
const MIN_DURATION: Duration = Duration::from_millis(1);
const MAX_CLOCK: Duration = Duration::from_millis(MAX_CLOCK_MS);

#[derive(Clone, Debug)]
pub struct FieldEquals {
    pub field: Field,
    pub expected: ScalarValue,
}
impl FieldEquals {
    fn validate<S: ProfileSnapshot>(&self) -> Result<(), WaitError> {
        if !self.expected.is_bounded()
            || S::field_type(self.field) != Some(self.expected.scalar_type())
        {
            return Err(WaitError::InvalidCondition);
        }
        Ok(())
    }
    fn matches<S: ProfileSnapshot>(
        &self,
        receipt: &ProfileReceipt<S>,
        fuel: &mut Fuel,
    ) -> Result<bool, WaitError> {
        let ProfileReply::Snapshot(observed) = &receipt.reply else {
            return Err(WaitError::InvalidCondition);
        };
        // Charge the profile's bounded projection/copy work before asking it
        // for the historical scalar. Comparing the resulting scalar is charged
        // separately; neither projection nor comparison refreshes native state.
        let copy_cost = observed
            .data()
            .copy_cost()
            .checked_add(1)
            .ok_or(WaitError::Host(Error::Bounds))?;
        charge(fuel, copy_cost)?;
        let actual = if self.field == Field::Revision {
            ScalarValue::Integer(observed.revision())
        } else {
            observed.data().field(self.field).map_err(WaitError::Host)?
        };
        if !actual.is_bounded() || actual.scalar_type() != self.expected.scalar_type() {
            return Err(WaitError::InvalidCondition);
        }
        charge(
            fuel,
            1 + scalar_copy_cost(&actual) + scalar_copy_cost(&self.expected),
        )?;
        Ok(actual == self.expected)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WaitPolicy {
    /// Includes the initial observation as attempt one.
    pub max_attempts: u32,
    pub interval: Duration,
    /// Starts at the first clock reading. `begin` follows an original dispatch;
    /// `begin_source` includes the initial source-request observation.
    pub timeout: Duration,
}
impl Default for WaitPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 16,
            interval: Duration::from_millis(50),
            timeout: Duration::from_secs(2),
        }
    }
}
impl WaitPolicy {
    pub fn validate(self) -> Result<(), WaitError> {
        if !(1..=MAX_ATTEMPTS).contains(&self.max_attempts)
            || !(MIN_DURATION..=MAX_INTERVAL).contains(&self.interval)
            || !(MIN_DURATION..=MAX_TIMEOUT).contains(&self.timeout)
            || self.interval > self.timeout
        {
            return Err(WaitError::InvalidPolicy);
        }
        Ok(())
    }
}

/// Read-only progress metadata; it carries no object or execution authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WaitStatus {
    pub attempts: u32,
    pub next_at: Duration,
    pub deadline: Duration,
}

/// A move-only outstanding observation. Dropping this handle discards its
/// receipt; it does not cancel the VM. The caller must explicitly cancel a VM
/// it will no longer drive. Neither this handle nor Ready accepts a reply.
///
/// ```compile_fail
/// use leselang_macos_ax_host::{Snapshot, observation_wait::ObservationWait};
/// fn duplicate(wait: ObservationWait<Snapshot>) { let _ = wait.clone(); }
/// ```
#[must_use = "step the wait or explicitly cancel the original VM"]
pub struct ObservationWait<S: ProfileSnapshot> {
    receipt: ProfileReceipt<S>,
    condition: WaitCondition,
    policy: WaitPolicy,
    status: WaitStatus,
    last_at: Duration,
    origin: WaitOrigin,
}
#[derive(Clone, Copy)]
enum WaitOrigin {
    Host,
    Source,
}
enum WaitCondition {
    Field(FieldEquals),
    State { expected: ScalarValue },
}
impl WaitCondition {
    fn matches<S: ProfileSnapshot>(
        &self,
        host: &Host<'_, '_, S>,
        receipt: &ProfileReceipt<S>,
        fuel: &mut Fuel,
    ) -> Result<bool, WaitError> {
        match self {
            Self::Field(condition) => condition.matches(receipt, fuel),
            Self::State { expected } => {
                let actual = host
                    .source_wait_state(receipt, fuel)
                    .map_err(WaitError::Host)?;
                charge(
                    fuel,
                    1 + scalar_copy_cost(expected) + actual.as_ref().map_or(0, scalar_copy_cost),
                )?;
                Ok(actual.as_ref() == Some(expected))
            }
        }
    }
}
impl<S: ProfileSnapshot> ObservationWait<S> {
    pub fn status(&self) -> WaitStatus {
        self.status
    }
}
impl<S: ProfileSnapshot> fmt::Debug for ObservationWait<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ObservationWait")
            .field("status", &self.status)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
#[must_use = "retain Waiting or explicitly accept the Ready receipt"]
pub enum WaitStep<S: ProfileSnapshot> {
    Waiting(ObservationWait<S>),
    Ready(ProfileReceipt<S>),
}

#[derive(Debug)]
pub enum WaitError {
    InvalidState,
    ForeignSession,
    InvalidPolicy,
    InvalidCondition,
    ClockRegression,
    ClockRange,
    ClockOverflow,
    TimedOut,
    AttemptsExhausted,
    Host(Error),
}
impl fmt::Display for WaitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidState => f.write_str("observation wait requires an awaiting VM"),
            Self::ForeignSession => f.write_str("observation wait belongs to another VM or host"),
            Self::InvalidPolicy => f.write_str("invalid bounded observation wait policy"),
            Self::InvalidCondition => {
                f.write_str("observation condition has an unavailable field or incompatible scalar")
            }
            Self::ClockRegression => f.write_str("observation wait clock moved backwards"),
            Self::ClockRange => f.write_str("observation wait clock exceeds the portable range"),
            Self::ClockOverflow => f.write_str("observation wait time arithmetic overflowed"),
            Self::TimedOut => f.write_str("observation wait deadline expired"),
            Self::AttemptsExhausted => f.write_str("observation wait exhausted its observations"),
            Self::Host(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for WaitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Host(error) => Some(error),
            _ => None,
        }
    }
}

fn charge(fuel: &mut Fuel, cost: u64) -> Result<(), WaitError> {
    fuel.charge(cost).map_err(|_| WaitError::Host(Error::Fuel))
}
fn clock_value(now: Duration, previous: Option<Duration>) -> Result<Duration, WaitError> {
    if now > MAX_CLOCK {
        return Err(WaitError::ClockRange);
    }
    if previous.is_some_and(|previous| now < previous) {
        return Err(WaitError::ClockRegression);
    }
    Ok(now)
}
fn add_time(now: Duration, duration: Duration) -> Result<Duration, WaitError> {
    now.checked_add(duration)
        .filter(|value| *value <= MAX_CLOCK)
        .ok_or(WaitError::ClockOverflow)
}
fn gate<S: ProfileSnapshot>(
    vm: &Vm<'_, '_, S>,
    host: &Host<'_, '_, S>,
    receipt: &ProfileReceipt<S>,
) -> Result<(), WaitError> {
    if !host.observation_wait_scope_matches(receipt, &vm.lease()) {
        return Err(WaitError::ForeignSession);
    }
    if vm.status() != EffectSessionStatus::Awaiting || !vm.lease().is_open() {
        return Err(WaitError::InvalidState);
    }
    Ok(())
}
fn outcome<S: ProfileSnapshot>(
    result: Result<WaitStep<S>, WaitError>,
) -> HostWorkOutcome<Result<WaitStep<S>, WaitError>> {
    match result {
        Ok(value) => HostWorkOutcome::Pending(Ok(value)),
        Err(error @ WaitError::Host(Error::Uncertain(_))) => HostWorkOutcome::Uncertain(Err(error)),
        Err(error) => HostWorkOutcome::Failed(Err(error)),
    }
}
fn finish_attempt<S: ProfileSnapshot>(
    host: &Host<'_, '_, S>,
    mut wait: ObservationWait<S>,
    matches: bool,
    now: Duration,
) -> Result<WaitStep<S>, WaitError> {
    if now >= wait.status.deadline {
        return Err(WaitError::TimedOut);
    }
    if matches {
        if matches!(wait.origin, WaitOrigin::Source) {
            host.complete_source_wait(&wait.receipt)
                .map_err(WaitError::Host)?;
        }
        return Ok(WaitStep::Ready(wait.receipt));
    }
    if wait.status.attempts >= wait.policy.max_attempts {
        return Err(WaitError::AttemptsExhausted);
    }
    wait.last_at = now;
    wait.status.next_at = add_time(now, wait.policy.interval)?.min(wait.status.deadline);
    Ok(WaitStep::Waiting(wait))
}

/// Hold one already-dispatched ordinary gui.observe receipt. Its snapshot is
/// attempt one; it is compared without rereading its field from the application.
/// The clock must be monotonic, share one origin for the whole wait, and return
/// within the portable MAX_CLOCK_MS range. It is read before and after checks.
pub fn begin<S: ProfileSnapshot>(
    vm: &mut Vm<'_, '_, S>,
    host: &Host<'_, '_, S>,
    receipt: ProfileReceipt<S>,
    condition: FieldEquals,
    policy: WaitPolicy,
    clock: &mut impl FnMut() -> Duration,
) -> Result<WaitStep<S>, WaitError> {
    gate(vm, host, &receipt)?;
    vm.with_metered_host_outcome((receipt, condition), |(receipt, condition), fuel| {
        outcome((|| {
            policy.validate()?;
            condition.validate::<S>()?;
            let now = clock_value(clock(), None)?;
            let deadline = add_time(now, policy.timeout)?;
            charge(fuel, 1 + scalar_copy_cost(&condition.expected))?;
            host.claim_observation_receipt(&receipt)
                .map_err(WaitError::Host)?;
            let matches = condition.matches(&receipt, fuel)?;
            let after = clock_value(clock(), Some(now))?;
            finish_attempt(
                host,
                ObservationWait {
                    receipt,
                    condition: WaitCondition::Field(condition),
                    policy,
                    status: WaitStatus {
                        attempts: 1,
                        next_at: now,
                        deadline,
                    },
                    last_at: now,
                    origin: WaitOrigin::Host,
                },
                matches,
                after,
            )
        })())
    })
    .map_err(|_| WaitError::InvalidState)?
}

/// Dispatch one source-declared wait. The condition and bounded
/// policy are taken only from its original Work; the caller supplies time, not
/// a replacement predicate. The deadline starts before the initial read, which
/// counts as attempt one. Ready proves the exact matching observation for this
/// request; it still needs explicit acceptance and live authority checks.
pub fn begin_source<S: ProfileSnapshot>(
    vm: &mut Vm<'_, '_, S>,
    host: &Host<'_, '_, S>,
    work: Work<'_, S>,
    clock: &mut impl FnMut() -> Duration,
) -> Result<WaitStep<S>, WaitError> {
    if !host.source_wait_scope_matches(&work, &vm.lease()) {
        return Err(WaitError::ForeignSession);
    }
    if vm.status() != EffectSessionStatus::Awaiting || !vm.lease().is_open() {
        return Err(WaitError::InvalidState);
    }
    vm.with_metered_host_outcome(work, |work, fuel| {
        outcome((|| {
            let now = clock_value(clock(), None)?;
            let arguments = host
                .source_wait_arguments(&work, fuel)
                .map_err(WaitError::Host)?;
            let policy = WaitPolicy {
                max_attempts: u32::try_from(arguments.max_attempts)
                    .map_err(|_| WaitError::InvalidPolicy)?,
                interval: Duration::from_millis(arguments.interval_ms),
                timeout: Duration::from_millis(arguments.timeout_ms),
            };
            policy.validate()?;
            if !arguments.kind.admits_expected(&arguments.expected) {
                return Err(WaitError::InvalidCondition);
            }
            let condition = if arguments.kind == SourceWaitKind::Value {
                let condition = FieldEquals {
                    field: Field::Value,
                    expected: arguments.expected,
                };
                condition.validate::<S>()?;
                WaitCondition::Field(condition)
            } else {
                WaitCondition::State {
                    expected: arguments.expected,
                }
            };
            let deadline = add_time(now, policy.timeout)?;
            let receipt = host
                .begin_source_observation(work, fuel)
                .map_err(WaitError::Host)?;
            let matches = condition.matches(host, &receipt, fuel)?;
            let after = clock_value(clock(), Some(now))?;
            finish_attempt(
                host,
                ObservationWait {
                    receipt,
                    condition,
                    policy,
                    status: WaitStatus {
                        attempts: 1,
                        next_at: now,
                        deadline,
                    },
                    last_at: now,
                    origin: WaitOrigin::Source,
                },
                matches,
                after,
            )
        })())
    })
    .map_err(|_| WaitError::InvalidState)?
}

/// Attempt at most one due postcondition observation. Even before next_at,
/// live host/auxiliary approval and object authority are checked. Such authority
/// checks may themselves read native metadata; they are not predicate attempts.
/// A read returning after the deadline fails, but this API cannot preempt AX.
pub fn step<S: ProfileSnapshot>(
    vm: &mut Vm<'_, '_, S>,
    host: &Host<'_, '_, S>,
    wait: ObservationWait<S>,
    clock: &mut impl FnMut() -> Duration,
) -> Result<WaitStep<S>, WaitError> {
    gate(vm, host, &wait.receipt)?;
    vm.with_metered_host_outcome(wait, |mut wait, fuel| {
        outcome((|| {
            let now = clock_value(clock(), Some(wait.last_at))?;
            charge(fuel, 1)?;
            if now >= wait.status.deadline || now < wait.status.next_at {
                match wait.origin {
                    WaitOrigin::Host => host.validate_observation_receipt(&wait.receipt),
                    WaitOrigin::Source => host.validate_source_wait_receipt(&wait.receipt, fuel),
                }
                .map_err(WaitError::Host)?;
                let after = clock_value(clock(), Some(now))?;
                if after >= wait.status.deadline {
                    return Err(WaitError::TimedOut);
                }
                wait.last_at = after;
                return Ok(WaitStep::Waiting(wait));
            }
            wait.receipt = match wait.origin {
                WaitOrigin::Host => host.refresh_observation_receipt(wait.receipt, fuel),
                WaitOrigin::Source => host.refresh_source_wait_receipt(wait.receipt, fuel),
            }
            .map_err(WaitError::Host)?;
            wait.status.attempts += 1;
            let matches = wait.condition.matches(host, &wait.receipt, fuel)?;
            let after = clock_value(clock(), Some(now))?;
            finish_attempt(host, wait, matches, after)
        })())
    })
    .map_err(|_| WaitError::InvalidState)?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Backend, Snapshot, driver, host};
    use leselang_vm::native::{
        LocalVm,
        core::{OptionalStringValue, StringListValue},
        hir::{effect_session::EffectSessionPoll, pure_evaluation::PureValue},
    };
    use std::cell::RefCell;

    #[test]
    fn policy_hard_limits_admit_endpoints_and_refuse_unbounded_or_inert_waits() {
        WaitPolicy::default().validate().unwrap();
        for max_attempts in [1, MAX_ATTEMPTS] {
            for (interval, timeout) in [(MIN_DURATION, MIN_DURATION), (MAX_INTERVAL, MAX_TIMEOUT)] {
                WaitPolicy {
                    max_attempts,
                    interval,
                    timeout,
                }
                .validate()
                .unwrap();
            }
        }
        for policy in [
            WaitPolicy {
                max_attempts: 0,
                ..WaitPolicy::default()
            },
            WaitPolicy {
                max_attempts: MAX_ATTEMPTS + 1,
                ..WaitPolicy::default()
            },
            WaitPolicy {
                interval: Duration::ZERO,
                ..WaitPolicy::default()
            },
            WaitPolicy {
                interval: MAX_INTERVAL + MIN_DURATION,
                ..WaitPolicy::default()
            },
            WaitPolicy {
                timeout: Duration::ZERO,
                ..WaitPolicy::default()
            },
            WaitPolicy {
                timeout: MAX_TIMEOUT + MIN_DURATION,
                ..WaitPolicy::default()
            },
            WaitPolicy {
                interval: Duration::from_millis(2),
                timeout: MIN_DURATION,
                ..WaitPolicy::default()
            },
        ] {
            assert!(matches!(policy.validate(), Err(WaitError::InvalidPolicy)));
        }
    }

    #[test]
    fn field_conditions_use_profile_types_without_optional_or_scalar_coercion() {
        use crate::widgets::WidgetSnapshot;
        let string = FieldEquals {
            field: Field::Value,
            expected: ScalarValue::String("16".into()),
        };
        string.validate::<Snapshot>().unwrap();
        assert!(matches!(
            string.validate::<WidgetSnapshot>(),
            Err(WaitError::InvalidCondition)
        ));
        let optional = FieldEquals {
            field: Field::Value,
            expected: ScalarValue::OptionalString(OptionalStringValue(Some("16".into()))),
        };
        optional.validate::<WidgetSnapshot>().unwrap();
        assert!(matches!(
            optional.validate::<Snapshot>(),
            Err(WaitError::InvalidCondition)
        ));
        FieldEquals {
            field: Field::Selected,
            expected: ScalarValue::Boolean(true),
        }
        .validate::<WidgetSnapshot>()
        .unwrap();
        FieldEquals {
            field: Field::Revision,
            expected: ScalarValue::Integer(1),
        }
        .validate::<WidgetSnapshot>()
        .unwrap();
        FieldEquals {
            field: Field::SelectedKeys,
            expected: ScalarValue::StringList(StringListValue(vec!["row".into()])),
        }
        .validate::<WidgetSnapshot>()
        .unwrap();
        for condition in [
            FieldEquals {
                field: Field::Selected,
                expected: ScalarValue::String("true".into()),
            },
            FieldEquals {
                field: Field::Display,
                expected: ScalarValue::String("16".into()),
            },
            FieldEquals {
                field: Field::Value,
                expected: ScalarValue::OptionalString(OptionalStringValue(Some("x".repeat(4097)))),
            },
            FieldEquals {
                field: Field::SelectedKeys,
                expected: ScalarValue::StringList(StringListValue(vec!["row".into(); 65])),
            },
        ] {
            assert!(matches!(
                condition.validate::<WidgetSnapshot>(),
                Err(WaitError::InvalidCondition)
            ));
        }
    }

    #[test]
    fn clock_checks_preserve_submillisecond_precision_and_never_wrap() {
        let now = Duration::from_nanos(1);
        assert_eq!(clock_value(now, Some(now)).unwrap(), now);
        assert_eq!(clock_value(MAX_CLOCK, None).unwrap(), MAX_CLOCK);
        assert!(matches!(
            clock_value(Duration::ZERO, Some(now)),
            Err(WaitError::ClockRegression)
        ));
        assert!(matches!(
            clock_value(MAX_CLOCK + now, None),
            Err(WaitError::ClockRange)
        ));
        assert_eq!(add_time(MAX_CLOCK - now, now).unwrap(), MAX_CLOCK);
        assert!(matches!(
            add_time(MAX_CLOCK, now),
            Err(WaitError::ClockOverflow)
        ));
        assert!(matches!(
            add_time(Duration::MAX, now),
            Err(WaitError::ClockOverflow)
        ));
    }

    #[test]
    fn custom_profile_copy_cost_overflow_refuses_before_projecting_or_charging() {
        use crate::host::ReplyPolicy;
        use leselang_vm::native::core::{
            LocalExecutionScope, ScalarType, host_protocol::LocalObjectOwner,
        };
        use std::rc::Rc;
        #[derive(Clone, Debug, Eq, PartialEq)]
        struct OverflowSnapshot;
        impl ReplyPolicy for OverflowSnapshot {
            type ScalarReply = bool;
        }
        impl ProfileSnapshot for OverflowSnapshot {
            const SELECTOR: &'static str = "overflow";
            const OPERATION: &'static str = "gui.set_text";
            const ARGUMENT: &'static str = "text";
            fn validate(&self) -> Result<(), Error> {
                Ok(())
            }
            fn same_instance(&self, _: &Self) -> bool {
                true
            }
            fn copy_cost(&self) -> u64 {
                u64::MAX
            }
            fn describe(&self) -> String {
                String::new()
            }
            fn field_type(_: Field) -> Option<ScalarType> {
                Some(ScalarType::String)
            }
            fn field(&self, _: Field) -> Result<ScalarValue, Error> {
                panic!("overflow must be rejected before allocating a scalar")
            }
            fn admit_action(&self, _: &str) -> Result<(), Error> {
                Ok(())
            }
        }
        // Reuse a real read identity; only the private scalar-projection helper
        // is under test, without inventing or transferring native authority.
        let backend = ReadBackend::new();
        host::with_host(&backend, |host, scope| {
            let node = host.compile(READ).unwrap();
            let mut vm = LocalVm::new(scope, &node, Vec::new(), Fuel::new(10_000), host::STACK);
            let find = driver::next(&mut vm, host).unwrap();
            let found = driver::dispatch(&mut vm, host, find).unwrap();
            let identity = found.identity.clone();
            driver::accept(&mut vm, host, found).unwrap();
            let local_scope = LocalExecutionScope::new();
            let lease = local_scope.lease();
            let owner = LocalObjectOwner::new();
            let target = owner.reference(&lease).unwrap();
            let observed = owner.observe(&target, &lease, OverflowSnapshot).unwrap();
            let receipt = ProfileReceipt {
                identity,
                reply: ProfileReply::Snapshot(Rc::new(observed)),
            };
            let condition = FieldEquals {
                field: Field::Value,
                expected: ScalarValue::String("value".into()),
            };
            let mut fuel = Fuel::new(100);
            assert!(matches!(
                condition.matches(&receipt, &mut fuel),
                Err(WaitError::Host(Error::Bounds))
            ));
            assert_eq!(fuel.remaining(), 100);
        })
        .unwrap();
    }

    struct ReadBackend(RefCell<Snapshot>);
    impl ReadBackend {
        fn new() -> Self {
            Self(RefCell::new(Snapshot {
                identity: 1,
                document: "file:///wait.txt".into(),
                role: "AXTextArea".into(),
                value: "13".into(),
                editable: true,
            }))
        }
    }
    impl Backend for ReadBackend {
        fn snapshot(&self) -> Result<Snapshot, Error> {
            Ok(self.0.borrow().clone())
        }
        fn check_permission(&self) -> Result<(), Error> {
            Ok(())
        }
        fn set_text(&self, _: &Snapshot, _: &str) -> Result<(), Error> {
            panic!("an observation wait must never mutate")
        }
    }
    const READ: &str = r#"fn main() = bind(target: gui.find(name: "document"), body: bind(state: gui.observe(target: target), body: field(value: state, name: "value")))"#;

    #[test]
    fn initial_receipt_stays_historical_and_only_a_due_step_supplies_a_new_snapshot() {
        let backend = ReadBackend::new();
        host::with_host(&backend, |host, scope| {
            let node = host.compile(READ).unwrap();
            let mut vm = LocalVm::new(scope, &node, Vec::new(), Fuel::new(10_000), host::STACK);
            let find = driver::next(&mut vm, host).unwrap();
            let found = driver::dispatch(&mut vm, host, find).unwrap();
            driver::accept(&mut vm, host, found).unwrap();
            let read = driver::next(&mut vm, host).unwrap();
            let receipt = driver::dispatch(&mut vm, host, read).unwrap();
            let ProfileReply::Snapshot(initial) = &receipt.reply else { panic!() };
            let initial = initial.clone();
            backend.0.borrow_mut().value = "16".into();
            let WaitStep::Waiting(wait) = begin(&mut vm, host, receipt,
                FieldEquals { field: Field::Value, expected: ScalarValue::String("16".into()) },
                WaitPolicy::default(), &mut || Duration::ZERO).unwrap() else { panic!("the original 13 snapshot cannot become a matching 16 snapshot") };
            assert_eq!(wait.status().attempts, 1);
            let WaitStep::Ready(receipt) = step(&mut vm, host, wait, &mut || Duration::from_millis(50)).unwrap() else { panic!() };
            let ProfileReply::Snapshot(after) = &receipt.reply else { panic!() };
            assert_eq!(after.data().value, "16");
            assert!(after.revision() > initial.revision());
            assert_eq!(initial.data().value, "13");
            assert_eq!(vm.status(), EffectSessionStatus::Awaiting);
            driver::accept(&mut vm, host, receipt).unwrap();
            assert!(matches!(vm.poll(host).unwrap(), EffectSessionPoll::Value(PureValue::Scalar(ScalarValue::String(value))) if value == "16"));
        }).unwrap();
    }
}
