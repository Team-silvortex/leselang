//! The scalar host adapter delegates compilation and evaluation to the native HIR.

use std::cell::Cell;

use crate::native::{
    core::{
        CalculationFailure, Fuel, LocalExecutionLease, ReplyAuthority, ScalarType, ScalarTypeSet,
        ScalarValue, ScopeFrame,
    },
    hir::{
        call_evaluation::{
            CallEvaluationError, CallEvaluationLimits, evaluate_call_arguments_in_scope,
        },
        catalog_source::CatalogSourceEnvironment,
        effect_evaluation::{EffectEvaluationEnvironment, EffectEvaluationLimits},
        effect_reentry::{EffectReentryEnvironment, RestoredEffectBindings},
        effect_session::{CorrelatedEffectRequest, EffectCorrelation, EffectSessionEnvironment},
        flow_typing::CallFlowEnvironment,
        pure_evaluation::{
            PureEvaluationEnvironment, PureEvaluationFault, PureValue, scalar_copy_cost,
        },
        pure_typing::{PureType, PureTypeEnvironment},
    },
};

use super::{Arguments, Error, Host, Identity, Node, Operation, Request};

pub(super) struct Env<'host, 'callback> {
    pub(super) host: &'host Host<'callback>,
    pub(super) lease: LocalExecutionLease,
    pub(super) serial: Cell<u64>,
    pub(super) grants: Vec<String>,
}

pub(super) struct Prepared {
    operation: usize,
    arguments: Arguments,
}

impl<'host, 'callback> Env<'host, 'callback> {
    pub(super) fn check_operation(
        &self,
        index: usize,
    ) -> Result<&'host Operation<'callback>, Error> {
        if !self.lease.is_open() {
            return Err(Error::Closed);
        }
        let operation = self
            .host
            .operations
            .get(index)
            .ok_or(Error::ForeignRequest)?;
        if !self.grants.contains(&operation.capability) {
            return Err(Error::Denied);
        }
        Ok(operation)
    }

    fn operation(&self, name: &str) -> Result<(usize, &'host Operation<'callback>), Error> {
        let index = self
            .host
            .operations
            .iter()
            .position(|operation| operation.name == name)
            .ok_or(Error::Unsupported)?;
        Ok((index, self.check_operation(index)?))
    }
}

impl ReplyAuthority<Identity> for Env<'_, '_> {
    type Error = Error;

    fn authorize(&self, identity: &Identity) -> Result<(), Error> {
        if !self.lease.is_open() {
            return Err(Error::Closed);
        }
        if !identity.lease.same_scope(&self.lease)
            || identity.serial == 0
            || identity.serial != self.serial.get()
        {
            return Err(Error::ForeignRequest);
        }
        self.check_operation(identity.operation)?;
        Ok(())
    }
}

impl PureTypeEnvironment<(), String> for Host<'_> {
    type Result = ();

    fn field_type(&self, _: &(), _: &()) -> Option<ScalarType> {
        None
    }

    fn member_result(&self, _: &(), _: &str, _: &String) -> Option<()> {
        None
    }
}

impl<'schema> CallFlowEnvironment<'schema, (), String, ScalarTypeSet> for Host<'_> {
    fn call_result_type(
        &self,
        name: &String,
        declaration: &'schema ScalarTypeSet,
    ) -> Option<PureType<()>> {
        let operation = self
            .operations
            .iter()
            .find(|operation| operation.name == *name)?;
        (operation.result == *declaration
            && operation.result == ScalarTypeSet::only(operation.result_type))
        .then_some(PureType::Scalar(operation.result_type))
    }
}

impl<'source, 'schema> CatalogSourceEnvironment<'source, 'schema, (), String, ScalarTypeSet>
    for Host<'_>
{
    type Error = Error;

    fn source_operation(&self, name: &String) -> Result<String, Error> {
        self.operations
            .iter()
            .find(|operation| operation.name == *name)
            .map(|operation| operation.name.clone())
            .ok_or(Error::Unsupported)
    }

    fn source_field(&self, _: &(), _: &'source str) -> Result<Option<((), ScalarType)>, Error> {
        Ok(None)
    }

    fn source_member(
        &self,
        _: &(),
        _: &'source str,
        _: &'source str,
    ) -> Result<Option<(String, ())>, Error> {
        Ok(None)
    }
}

impl PureEvaluationEnvironment<(), String> for Env<'_, '_> {
    type Result = ();
    type Error = Error;

    fn field(&self, _: &(), _: &()) -> Result<ScalarValue, Error> {
        Err(Error::Unsupported)
    }

    fn member(&self, _: &(), _: &str, _: &String) -> Result<(), Error> {
        Err(Error::Unsupported)
    }
}

fn preparation_error(error: CallEvaluationError<Error>) -> CalculationFailure<Error> {
    match error {
        CallEvaluationError::Evaluation {
            failure: CalculationFailure::Scalar(error),
            ..
        } => CalculationFailure::Scalar(error),
        CallEvaluationError::Evaluation {
            failure: CalculationFailure::External(PureEvaluationFault::Native(error)),
            ..
        } => error.into(),
        CallEvaluationError::FuelExhausted
        | CallEvaluationError::Evaluation {
            failure: CalculationFailure::External(PureEvaluationFault::FuelExhausted),
            ..
        } => Error::Execution("execution fuel exhausted".to_owned()).into(),
        error => Error::Execution(error.to_string()).into(),
    }
}

impl<'ir> EffectEvaluationEnvironment<'ir, (), String, (), ()> for Env<'_, '_> {
    type Request = Prepared;
    type Capture = RestoredEffectBindings<'ir, ()>;

    fn preflight_effect(&self, node: &'ir Node) -> Result<(), Error> {
        let Node::Call {
            operation,
            arguments,
        } = node
        else {
            return Err(Error::Unsupported);
        };
        let (_, operation) = self.operation(operation)?;
        operation.with_schema(|schema| {
            schema
                .bind_arguments(
                    &arguments
                        .iter()
                        .map(|argument| argument.name.as_str())
                        .collect::<Vec<_>>(),
                )
                .map(|_| ())
                .map_err(|error| Error::Execution(error.to_string()))
        })
    }

    fn prepare_effect(
        &self,
        node: &'ir Node,
        scope: &mut ScopeFrame<'_, 'ir, PureValue<()>>,
        fuel: &mut Fuel,
    ) -> Result<Prepared, CalculationFailure<Error>> {
        let Node::Call {
            operation,
            arguments,
        } = node
        else {
            return Err(Error::Unsupported.into());
        };
        let (index, operation) = self.operation(operation)?;
        operation.with_schema(|schema| {
            let prepared = evaluate_call_arguments_in_scope(
                arguments,
                schema,
                scope,
                self,
                fuel,
                CallEvaluationLimits {
                    pure: self.host.limits.execution.effects.pure,
                    max_arguments: self.host.limits.execution.effects.max_arguments,
                },
            )
            .map_err(preparation_error)?;
            let (_, values) = prepared.into_parts();
            Ok(Prepared {
                operation: index,
                arguments: Arguments {
                    values: values
                        .into_iter()
                        .map(|argument| (argument.name.to_owned(), argument.value))
                        .collect(),
                },
            })
        })
    }

    fn capture(
        &self,
        _: &'ir str,
        _: &'ir Node,
        scope: &ScopeFrame<'_, 'ir, PureValue<()>>,
        fuel: &mut Fuel,
    ) -> Result<Self::Capture, Error> {
        let mut cost = 0_u64;
        for (_, value) in scope.bindings() {
            let PureValue::Scalar(value) = value else {
                return Err(Error::Capture);
            };
            cost = cost
                .checked_add(1 + scalar_copy_cost(value))
                .ok_or(Error::Capture)?;
        }
        fuel.charge(cost).map_err(|_| Error::Capture)?;
        Ok(scope.bindings().to_vec())
    }
}

impl<'ir> EffectReentryEnvironment<'ir, (), String, (), (), Identity, ScalarTypeSet, ScalarValue>
    for Env<'_, '_>
{
    fn restore_capture(
        &self,
        identity: &Identity,
        _: &ScalarTypeSet,
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
        declaration: &ScalarTypeSet,
        reply: ScalarValue,
        _: &mut Fuel,
    ) -> Result<PureValue<()>, CalculationFailure<Error>> {
        self.authorize(identity)?;
        declaration
            .validate(&reply)
            .map_err(|error| Error::Execution(error.to_string()))?;
        Ok(PureValue::Scalar(reply))
    }
}

impl<'ir, 'host> EffectSessionEnvironment<'ir, 'host, (), String, (), ()> for Env<'host, '_> {
    type Identity = Identity;
    type Declaration = ScalarTypeSet;
    type Reply = ScalarValue;
    type Dispatch = Request;

    fn correlate_request(
        &self,
        prepared: Prepared,
        _: &mut Fuel,
    ) -> EffectCorrelation<'host, Identity, ScalarTypeSet, Request, Error> {
        let operation = self.check_operation(prepared.operation)?;
        let serial = self
            .serial
            .get()
            .checked_add(1)
            .ok_or(Error::IdentityExhausted)?;
        self.serial.set(serial);
        let identity = Identity {
            lease: self.lease.clone(),
            serial,
            operation: prepared.operation,
        };
        Ok(CorrelatedEffectRequest {
            identity: identity.clone(),
            declaration: &operation.result,
            dispatch: Request {
                identity,
                arguments: prepared.arguments,
                name: operation.name.clone(),
            },
        })
    }
}
