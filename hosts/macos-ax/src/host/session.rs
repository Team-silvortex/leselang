//! Runtime Effect boundaries: prepare, correlate, authorize, dispatch, explicitly
//! pump, and accept. Native identity and current authority are rechecked here;
//! an acknowledged mutation still needs a separate observation.
use super::*;
use call_evaluation::{
    CallEvaluationError, CallEvaluationLimits, ValueCallEvaluationError,
    evaluate_value_call_arguments_in_scope,
};
use effect_evaluation::EffectEvaluationEnvironment;
use effect_reentry::{EffectReentryEnvironment, RestoredEffectBindings};
use effect_session::{CorrelatedEffectRequest, EffectCorrelation, EffectSessionEnvironment};
use leselang_vm::native::HostWorkOutcome;
use pure_evaluation::{PureEvaluationEnvironment, PureValue, scalar_copy_cost};

fn scalar_string<'a, S: ProfileSnapshot>(
    call: &'a Prepared<'_, S>,
    index: usize,
) -> Result<&'a str, Error> {
    match call.arguments().get(index).map(|argument| &argument.value) {
        Some(PureValue::Scalar(ScalarValue::String(value))) => Ok(value),
        _ => Err(Error::Invalid("missing string argument")),
    }
}
fn source_wait_parameters<S: ProfileSnapshot>(
    call: &Prepared<'_, S>,
    fuel: &mut Fuel,
) -> Result<SourceWaitParameters, Error> {
    let scalar = |index: usize| match call.arguments().get(index).map(|arg| &arg.value) {
        Some(PureValue::Scalar(value)) => Ok(value),
        _ => Err(Error::Invalid("missing source wait argument")),
    };
    let kind = SourceWaitKind::from_operation(call.schema().key)
        .ok_or(Error::Invalid("unknown source wait operation"))?;
    let expected = scalar(1)?;
    if expected.scalar_type() != kind.expected_type() || !expected.is_bounded() {
        return Err(Error::Invalid(
            "source wait requires its declared bounded expected type",
        ));
    }
    let integer = |index| match scalar(index)? {
        ScalarValue::Integer(value) => Ok(*value),
        _ => Err(Error::Invalid("source wait budget must be an integer")),
    };
    fuel.charge(1 + scalar_copy_cost(expected))
        .map_err(|_| Error::Fuel)?;
    Ok(SourceWaitParameters {
        kind,
        expected: expected.clone(),
        max_attempts: integer(2)?,
        interval_ms: integer(3)?,
        timeout_ms: integer(4)?,
    })
}
impl<'s, S: ProfileSnapshot> Host<'_, 's, S> {
    pub fn owner(&self) -> &LocalObjectOwner {
        &self.tracked.owners[0]
    }
    pub fn lease(&self) -> LocalExecutionLease {
        self.lease.clone()
    }
    pub fn set_granted(&self, granted: bool) {
        self.granted.set(granted);
    }
    pub fn refresh(&self) -> Result<S, Error> {
        self.tracked.refresh()
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
    pub(crate) fn observation_wait_scope_matches(
        &self,
        receipt: &ProfileReceipt<S>,
        lease: &LocalExecutionLease,
    ) -> bool {
        self.lease.same_scope(lease) && receipt.identity.lease.same_scope(lease)
    }
    pub(crate) fn source_wait_scope_matches(
        &self,
        work: &Work<'_, S>,
        lease: &LocalExecutionLease,
    ) -> bool {
        self.lease.same_scope(lease) && work.identity.lease.same_scope(lease)
    }
    pub(crate) fn source_wait_arguments(
        &self,
        work: &Work<'_, S>,
        fuel: &mut Fuel,
    ) -> Result<SourceWaitParameters, Error> {
        if !self.source_waits.contains(work.operation())
            || work.identity.serial != self.serial.get()
            || self.dispatched.get() == work.identity.serial
            || !std::ptr::eq(self.schema(work.operation())?, work.call.schema())
        {
            return Err(Error::Invalid(
                "source wait requires its original undispatched request",
            ));
        }
        let request = work.call.schema().result.source_wait.borrow();
        let request = request
            .as_ref()
            .filter(|request| request.serial == work.identity.serial)
            .ok_or(Error::Invalid("source wait request is no longer current"))?;
        let parameters = &request.parameters;
        fuel.charge(1 + scalar_copy_cost(&parameters.expected))
            .map_err(|_| Error::Fuel)?;
        Ok(SourceWaitParameters {
            kind: parameters.kind,
            expected: parameters.expected.clone(),
            max_attempts: parameters.max_attempts,
            interval_ms: parameters.interval_ms,
            timeout_ms: parameters.timeout_ms,
        })
    }
    pub(crate) fn begin_source_observation(
        &self,
        work: Work<'_, S>,
        fuel: &mut Fuel,
    ) -> Result<ProfileReceipt<S>, Error> {
        self.authorize(&work.identity)?;
        if !self.source_waits.contains(work.operation())
            || self.dispatched.get() == work.identity.serial
            || !std::ptr::eq(self.schema(work.operation())?, work.call.schema())
        {
            return Err(Error::Invalid("stale or foreign source wait dispatch"));
        }
        self.dispatched.set(work.identity.serial);
        let reply = self.observation_reply(&work.identity, fuel)?;
        Ok(ProfileReceipt {
            identity: work.identity,
            reply,
        })
    }
    pub(crate) fn validate_source_wait_receipt(
        &self,
        receipt: &ProfileReceipt<S>,
        fuel: &mut Fuel,
    ) -> Result<(), Error> {
        if !self.source_waits.contains(receipt.identity.operation)
            || self.dispatched.get() != receipt.identity.serial
        {
            return Err(Error::Invalid(
                "source wait requires a dispatched wait request",
            ));
        }
        let ProfileReply::Snapshot(observed) = &receipt.reply else {
            return Err(Error::Invalid("source wait requires an observation"));
        };
        self.authorize(&receipt.identity)?;
        let domain = &self.schema(receipt.identity.operation)?.result;
        if !domain.source_wait.borrow().as_ref().is_some_and(|request| {
            request.serial == receipt.identity.serial && request.ready.is_none()
        }) {
            return Err(Error::Invalid("source wait is no longer pending"));
        }
        // Pending samples are private to the move-only wait. Public reply
        // admission additionally requires the completion proof below.
        domain.validate_snapshot(observed)?;
        if receipt.identity.operation != SourceWaitKind::Value.operation() {
            // State availability and the selection owner are live requirements
            // while Waiting, including before the next scheduled attempt.
            // A Ready reply still admits its historical payload in Domain.
            let slot = self.resolve(observed.object())?;
            let current = self.tracked.refresh_slot(slot)?;
            self.project_source_wait(&receipt.identity, &current, fuel)?;
        }
        Ok(())
    }
    fn project_source_wait(
        &self,
        identity: &Identity,
        snapshot: &S,
        fuel: &mut Fuel,
    ) -> Result<Option<ScalarValue>, Error> {
        let domain = &self.schema(identity.operation)?.result;
        let request = domain.source_wait.borrow();
        let request = request
            .as_ref()
            .filter(|request| request.serial == identity.serial && request.ready.is_none())
            .ok_or(Error::Invalid("source wait is no longer pending"))?;
        fuel.charge(snapshot.copy_cost().checked_add(1).ok_or(Error::Bounds)?)
            .map_err(|_| Error::Fuel)?;
        let state = self
            .tracked
            .backend
            .source_wait_state(snapshot, request.parameters.kind)?;
        if request.parameters.kind == SourceWaitKind::Selected {
            let (identity, key) = state
                .selection_parent
                .ok_or(Error::Invalid("selected state has no collection owner"))?;
            fuel.charge(1 + key.len() as u64).map_err(|_| Error::Fuel)?;
            match &request.selection_parent {
                Some((original_identity, original_key))
                    if *original_identity == identity && original_key == key => {}
                _ => return Err(Error::Stale),
            }
        }
        if state
            .value
            .as_ref()
            .is_some_and(|value| !request.parameters.kind.admits_expected(value))
        {
            return Err(Error::Invalid("source wait returned an incompatible state"));
        }
        Ok(state.value)
    }
    fn source_wait_parent(
        &self,
        slot: usize,
        fuel: &mut Fuel,
    ) -> Result<Option<(u64, String)>, Error> {
        // resolve already refreshed this entry while correlating the request.
        // Pin the collection here without an extra native read, so moving the
        // item between poll and begin_source cannot establish a new owner.
        let entries = self.tracked.entries.borrow();
        let snapshot = entries
            .get(slot)
            .and_then(|entry| entry.previous.as_ref())
            .ok_or(Error::Stale)?;
        fuel.charge(snapshot.copy_cost().checked_add(1).ok_or(Error::Bounds)?)
            .map_err(|_| Error::Fuel)?;
        let state = match self
            .tracked
            .backend
            .source_wait_state(snapshot, SourceWaitKind::Selected)
        {
            Ok(state) => state,
            // Preserve dispatch as the state-availability failure boundary;
            // an unavailable initial relation never grants a new owner later.
            Err(Error::Invalid(_)) => return Ok(None),
            Err(error) => return Err(error),
        };
        let Some((identity, key)) = state.selection_parent else {
            return Ok(None);
        };
        fuel.charge(1 + key.len() as u64).map_err(|_| Error::Fuel)?;
        Ok(Some((identity, key.to_owned())))
    }
    pub(crate) fn source_wait_state(
        &self,
        receipt: &ProfileReceipt<S>,
        fuel: &mut Fuel,
    ) -> Result<Option<ScalarValue>, Error> {
        let ProfileReply::Snapshot(observed) = &receipt.reply else {
            return Err(Error::Invalid("source wait requires an observation"));
        };
        self.project_source_wait(&receipt.identity, observed.data(), fuel)
    }
    pub(crate) fn refresh_source_wait_receipt(
        &self,
        receipt: ProfileReceipt<S>,
        fuel: &mut Fuel,
    ) -> Result<ProfileReceipt<S>, Error> {
        self.validate_source_wait_receipt(&receipt, fuel)?;
        let reply = self.observation_reply(&receipt.identity, fuel)?;
        Ok(ProfileReceipt {
            identity: receipt.identity,
            reply,
        })
    }
    pub(crate) fn complete_source_wait(&self, receipt: &ProfileReceipt<S>) -> Result<(), Error> {
        if !self.source_waits.contains(receipt.identity.operation)
            || receipt.identity.serial != self.serial.get()
            || receipt.identity.serial != self.dispatched.get()
        {
            return Err(Error::Invalid(
                "source wait completion is no longer current",
            ));
        }
        let ProfileReply::Snapshot(observed) = &receipt.reply else {
            return Err(Error::Invalid("source wait requires an observation"));
        };
        let mut request = self
            .schema(receipt.identity.operation)?
            .result
            .source_wait
            .borrow_mut();
        let request = request
            .as_mut()
            .filter(|request| request.serial == receipt.identity.serial && request.ready.is_none())
            .ok_or(Error::Invalid("source wait completion already published"))?;
        request.ready = Some(Rc::clone(observed));
        Ok(())
    }
    pub(crate) fn validate_observation_receipt(
        &self,
        receipt: &ProfileReceipt<S>,
    ) -> Result<(), Error> {
        if receipt.identity.operation != "gui.observe"
            || self.dispatched.get() != receipt.identity.serial
            || !matches!(receipt.reply, ProfileReply::Snapshot(_))
        {
            return Err(Error::Invalid(
                "observation wait requires a dispatched observe receipt",
            ));
        }
        self.authorize(&receipt.identity)?;
        self.schema("gui.observe")?
            .result
            .validate_value(&receipt.reply)
    }
    pub(crate) fn claim_observation_receipt(
        &self,
        receipt: &ProfileReceipt<S>,
    ) -> Result<(), Error> {
        if self.observation_wait_serial.get() == receipt.identity.serial {
            return Err(Error::Invalid(
                "this observation request already had a wait",
            ));
        }
        self.validate_observation_receipt(receipt)?;
        self.observation_wait_serial.set(receipt.identity.serial);
        Ok(())
    }
    pub(crate) fn refresh_observation_receipt(
        &self,
        receipt: ProfileReceipt<S>,
        fuel: &mut Fuel,
    ) -> Result<ProfileReceipt<S>, Error> {
        self.validate_observation_receipt(&receipt)?;
        let reply = self.observation_reply(&receipt.identity, fuel)?;
        Ok(ProfileReceipt {
            identity: receipt.identity,
            reply,
        })
    }
    fn observation_reply(
        &self,
        identity: &Identity,
        fuel: &mut Fuel,
    ) -> Result<ProfileReply<S>, Error> {
        let target = identity
            .target
            .as_ref()
            .ok_or(Error::Invalid("missing target"))?;
        fuel.charge(1).map_err(|_| Error::Fuel)?;
        let slot = self.resolve(target)?;
        let data = self.tracked.refresh_slot(slot)?;
        fuel.charge(data.copy_cost()).map_err(|_| Error::Fuel)?;
        Ok(ProfileReply::Snapshot(Rc::new(
            self.tracked.owners[slot].observe(target, &self.lease, data)?,
        )))
    }
    pub fn dispatch(
        &self,
        work: Work<'s, S>,
        fuel: &mut Fuel,
    ) -> HostWorkOutcome<Result<ProfileReceipt<S>, Error>> {
        match self.invoke(work, fuel) {
            Ok(value) => HostWorkOutcome::Pending(Ok(value)),
            Err(error) => HostWorkOutcome::Failed(Err(error)),
        }
    }
    fn invoke(&self, work: Work<'s, S>, fuel: &mut Fuel) -> Result<ProfileReceipt<S>, Error> {
        if work.is_source_wait() {
            return Err(Error::Invalid(
                "source waits require observation_wait::begin_source",
            ));
        }
        self.authorize(&work.identity)?;
        if self.dispatched.get() == work.identity.serial
            || !std::ptr::eq(self.schema(work.operation())?, work.call.schema())
        {
            return Err(Error::Invalid("stale or foreign dispatch"));
        }
        self.dispatched.set(work.identity.serial);
        let reply = match work.operation() {
            "gui.list" if S::CONTROLS => {
                let role = scalar_string(&work.call, 0)?;
                fuel.charge(1 + role.len() as u64)
                    .map_err(|_| Error::Fuel)?;
                let snapshots = self.tracked.refresh_all()?;
                let keys = StringListValue(
                    snapshots
                        .iter()
                        .filter(|(_, snapshot)| snapshot.selector_matches(role, "", ""))
                        .map(|(_, snapshot)| snapshot.key().to_owned())
                        .collect(),
                );
                if !keys.is_bounded() {
                    return Err(Error::Bounds);
                }
                fuel.charge(1 + keys.0.iter().map(|key| key.len() as u64).sum::<u64>())
                    .map_err(|_| Error::Fuel)?;
                ProfileReply::Submitted(S::ScalarReply::keys_value(keys)?)
            }
            "gui.find" | "gui.select" | "gui.select_value" => {
                let snapshots = self.tracked.refresh_all()?;
                let selected = if work.operation() == "gui.find" {
                    let key = scalar_string(&work.call, 0)?;
                    fuel.charge(1 + key.len() as u64).map_err(|_| Error::Fuel)?;
                    snapshots
                        .iter()
                        .find(|(_, snapshot)| snapshot.key() == key)
                        .map(|(slot, _)| *slot)
                } else if S::WIDGETS && work.operation() == "gui.select_value" {
                    let role = scalar_string(&work.call, 0)?;
                    let value = scalar_string(&work.call, 1)?;
                    fuel.charge(1 + (role.len() + value.len()) as u64)
                        .map_err(|_| Error::Fuel)?;
                    if role.is_empty() {
                        return Err(Error::Invalid("a value selector requires an exact role"));
                    }
                    let mut matches = snapshots
                        .iter()
                        .filter(|(_, snapshot)| snapshot.value_selector_matches(role, value));
                    let selected = matches.next().map(|(slot, _)| *slot);
                    if matches.next().is_some() {
                        return Err(Error::Invalid("selector is ambiguous"));
                    }
                    selected
                } else if S::CONTROLS {
                    let role = scalar_string(&work.call, 0)?;
                    let identifier = scalar_string(&work.call, 1)?;
                    let label = scalar_string(&work.call, 2)?;
                    fuel.charge(1 + (role.len() + identifier.len() + label.len()) as u64)
                        .map_err(|_| Error::Fuel)?;
                    if role.is_empty() && identifier.is_empty() && label.is_empty() {
                        return Err(Error::Invalid("an unconstrained selector is not allowed"));
                    }
                    let mut matches = snapshots
                        .iter()
                        .filter(|(_, snapshot)| snapshot.selector_matches(role, identifier, label));
                    let selected = matches.next().map(|(slot, _)| *slot);
                    if matches.next().is_some() {
                        return Err(Error::Invalid("selector is ambiguous"));
                    }
                    selected
                } else {
                    return Err(Error::Invalid("unknown operation"));
                };
                let slot =
                    selected.ok_or(Error::Unavailable("no scoped control matches the selector"))?;
                let object = self.tracked.owners[slot].reference(&self.lease)?;
                *work.call.schema().result.expected.borrow_mut() = Some(object.clone());
                ProfileReply::Object(object)
            }
            "gui.observe" => self.observation_reply(&work.identity, fuel)?,
            operation if S::is_mutation(operation) => {
                let target = work
                    .identity
                    .target
                    .as_ref()
                    .ok_or(Error::Invalid("missing target"))?;
                let slot = self.resolve(target)?;
                let data = self.tracked.refresh_slot(slot)?;
                let Some(PureValue::Scalar(ScalarValue::Integer(expected))) =
                    work.call.arguments().get(1).map(|arg| &arg.value)
                else {
                    return Err(Error::Invalid("missing revision"));
                };
                let argument = if S::WIDGETS
                    && matches!(operation, "gui.choose_menu_item" | "gui.select_item")
                {
                    let Some(PureValue::Result(ProfileNativeValue::Object(reference))) =
                        work.call.arguments().get(2).map(|arg| &arg.value)
                    else {
                        return Err(Error::Invalid("missing native item argument"));
                    };
                    let item_slot = self.resolve(reference)?;
                    let item = self.tracked.refresh_slot(item_slot)?;
                    let identity = item
                        .native_identity()
                        .ok_or(Error::Invalid("item has no native widget identity"))?;
                    ActionArgument::Item {
                        reference: reference.clone(),
                        identity,
                    }
                } else if S::WIDGETS && operation == "gui.set_checked" {
                    let Some(PureValue::Scalar(ScalarValue::Boolean(value))) =
                        work.call.arguments().get(2).map(|arg| &arg.value)
                    else {
                        return Err(Error::Invalid("missing checked boolean"));
                    };
                    ActionArgument::Boolean(*value)
                } else if S::WIDGETS && operation == "gui.scroll" {
                    let Some(PureValue::Scalar(ScalarValue::Integer(value))) =
                        work.call.arguments().get(2).map(|arg| &arg.value)
                    else {
                        return Err(Error::Invalid("missing scroll position"));
                    };
                    ActionArgument::Integer(*value)
                } else if (S::CONTROLS && operation == "gui.press")
                    || (S::WIDGETS && matches!(operation, "gui.open_menu" | "gui.cancel_menu"))
                {
                    ActionArgument::None
                } else {
                    let text = scalar_string(&work.call, 2)?;
                    // Admit and charge borrowed text before allocating its queued copy.
                    data.admit_operation(operation, text)?;
                    fuel.charge(1 + text.len() as u64)
                        .map_err(|_| Error::Fuel)?;
                    ActionArgument::Text(text.to_owned())
                };
                data.admit_typed_operation(operation, &argument)?;
                if !matches!(argument, ActionArgument::Text(_)) {
                    fuel.charge(argument.cost()).map_err(|_| Error::Fuel)?;
                }
                let expected = *expected;
                let owner = &self.tracked.owners[slot];
                let attempt = owner.with_current(target, &self.lease, expected, || {
                    let mut pending = self.pending.borrow_mut();
                    if pending.is_some() {
                        return Err(Error::Invalid("host action queue is full"));
                    }
                    *pending = Some(PendingAction {
                        identity: work.identity.clone(),
                        revision: owner.revision(),
                        expected: data,
                        argument,
                    });
                    Ok(())
                })?;
                attempt.into_output()?;
                ProfileReply::Submitted(S::ScalarReply::submitted())
            }
            _ => return Err(Error::Invalid("unknown operation")),
        };
        Ok(ProfileReceipt {
            identity: work.identity,
            reply,
        })
    }
    /// Explicitly attempt the queued AX write. No polling, acceptance, retry or
    /// scheduler is hidden here. The bridge rechecks the snapshot immediately
    /// before its setter; the remote process can still race that check.
    pub fn pump(
        &self,
        receipt: ProfileReceipt<S>,
        fuel: &mut Fuel,
    ) -> HostWorkOutcome<Result<ProfileReceipt<S>, Error>> {
        let mut entered_backend = false;
        let result = (|| {
            self.authorize(&receipt.identity)?;
            if !S::is_mutation(receipt.identity.operation) || !receipt.reply.is_submission() {
                return Err(Error::Invalid("pump requires original submission receipt"));
            }
            let target = receipt
                .identity
                .target
                .as_ref()
                .ok_or(Error::Invalid("missing target"))?;
            let slot = self.resolve(target)?;
            let data = self.tracked.refresh_slot(slot)?;
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
            if let ActionArgument::Item {
                reference,
                identity,
            } = &update.argument
            {
                let item_slot = self.resolve(reference)?;
                if self.tracked.refresh_slot(item_slot)?.native_identity() != Some(*identity) {
                    return Err(Error::Stale);
                }
            }
            data.admit_typed_operation(receipt.identity.operation, &update.argument)?;
            fuel.charge(update.argument.cost())
                .map_err(|_| Error::Fuel)?;
            let attempt = self.tracked.owners[slot].with_current(
                target,
                &self.lease,
                update.revision,
                || {
                    entered_backend = true;
                    self.tracked.backend.apply_typed_operation(
                        &update.expected,
                        receipt.identity.operation,
                        &update.argument,
                    )
                },
            )?;
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
impl<S: ProfileSnapshot> ReplyAuthority<Identity> for Host<'_, '_, S> {
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
impl<S: ProfileSnapshot> PureEvaluationEnvironment<Field, &'static str> for Host<'_, '_, S> {
    type Result = ProfileNativeValue<S>;
    type Error = Error;
    fn field(&self, observed: &Self::Result, field: &Field) -> Result<ScalarValue, Error> {
        let ProfileNativeValue::Snapshot(observed) = observed else {
            return Err(Error::Invalid("object has no scalar fields"));
        };
        let slot = self.resolve(observed.object())?;
        self.tracked.validate_observation(slot, observed)?;
        if *field == Field::Revision {
            Ok(ScalarValue::Integer(observed.revision()))
        } else {
            observed.data().field(*field)
        }
    }

    fn member(&self, _: &Self::Result, _: &str, _: &&'static str) -> Result<Self::Result, Error> {
        Err(Error::Invalid("no native groups"))
    }
}
impl<'ir, 's, S: ProfileSnapshot> EffectEvaluationEnvironment<'ir, Field, &'static str, (), ()>
    for Host<'_, 's, S>
{
    type Request = Prepared<'s, S>;
    type Capture = RestoredEffectBindings<'ir, ProfileNativeValue<S>>;
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
                (ArgumentDomain::Object, PureValue::Result(ProfileNativeValue::Object(target))) => {
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
impl<'ir, 's, S: ProfileSnapshot>
    EffectReentryEnvironment<
        'ir,
        Field,
        &'static str,
        (),
        (),
        Identity,
        Domain<'s, S>,
        ProfileReply<S>,
    > for Host<'_, 's, S>
{
    fn restore_capture(
        &self,
        identity: &Identity,
        _: &Domain<'s, S>,
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
        domain: &Domain<'s, S>,
        reply: ProfileReply<S>,
        _: &mut Fuel,
    ) -> Result<PureValue<Self::Result>, CalculationFailure<Error>> {
        self.authorize(identity)?;
        if !std::ptr::eq(domain, &self.schema(identity.operation)?.result) {
            return Err(Error::Invalid("foreign result domain").into());
        }
        domain.validate_value(&reply)?;
        Ok(match reply {
            ProfileReply::Object(value) => PureValue::Result(ProfileNativeValue::Object(value)),
            ProfileReply::Snapshot(value) => PureValue::Result(ProfileNativeValue::Snapshot(value)),
            ProfileReply::Submitted(value) => PureValue::Scalar(value.into_scalar()),
        })
    }
}
impl<'ir, 's, S: ProfileSnapshot> EffectSessionEnvironment<'ir, 's, Field, &'static str, (), ()>
    for Host<'_, 's, S>
{
    type Identity = Identity;
    type Declaration = Domain<'s, S>;
    type Reply = ProfileReply<S>;
    type Dispatch = Work<'s, S>;
    fn correlate_request(
        &self,
        request: Self::Request,
        fuel: &mut Fuel,
    ) -> EffectCorrelation<'s, Identity, Domain<'s, S>, Work<'s, S>, Error> {
        let schema = self.schema(request.schema().key)?;
        if !std::ptr::eq(schema, request.schema()) {
            return Err(Error::Invalid("foreign schema").into());
        }
        let (target, target_slot) = if matches!(
            schema.key,
            "gui.find" | "gui.list" | "gui.select" | "gui.select_value"
        ) {
            (None, None)
        } else {
            let Some(PureValue::Result(ProfileNativeValue::Object(target))) =
                request.arguments().first().map(|arg| &arg.value)
            else {
                return Err(Error::Invalid("missing target").into());
            };
            let slot = self.resolve(target)?;
            (Some(target.clone()), Some(slot))
        };
        *schema.result.expected.borrow_mut() = target.clone();
        let serial = self
            .serial
            .get()
            .checked_add(1)
            .ok_or(Error::Invalid("identity exhausted"))?;
        for kind in self.source_waits.kinds() {
            self.schema(kind.operation())?
                .result
                .source_wait
                .borrow_mut()
                .take();
        }
        if self.source_waits.contains(schema.key) {
            let parameters = source_wait_parameters(&request, fuel)?;
            let selection_parent = if parameters.kind == SourceWaitKind::Selected {
                self.source_wait_parent(
                    target_slot.ok_or(Error::Invalid("missing wait target"))?,
                    fuel,
                )?
            } else {
                None
            };
            *schema.result.source_wait.borrow_mut() = Some(SourceWaitRequest {
                serial,
                parameters,
                ready: None,
                selection_parent,
            });
        }
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
