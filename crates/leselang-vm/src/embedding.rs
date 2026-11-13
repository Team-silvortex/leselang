//! Register scalar host operations and drive the original local VM explicitly.
//!
//! This adapter is optional: [`crate::native`] retains the general catalog, IR,
//! result-domain and session contracts. No executor, thread, timer or implicit
//! dispatch is installed. Each effect requires `poll → authorize → dispatch →
//! accept`; acceptance does not run the continuation. Requests and receipts are
//! local, move-only handles, not durable or wire protocols.

use std::{fmt, rc::Rc};

pub use crate::native::core::host_protocol::ProtocolVersion;
pub use crate::native::core::{Fuel, NamedParameter, ScalarType, ScalarTypeSet, ScalarValue};
pub use crate::native::hir::effect_session::{EffectSessionEnd, EffectSessionStatus};
use crate::native::{
    HostWorkOutcome, LocalVm,
    core::{
        LocalExecutionLease, LocalExecutionScope, OperationCatalog, OperationCatalogError,
        OperationCatalogLimits, OperationSchema, ReplyAuthority,
    },
    hir::{
        catalog_program::{CatalogProgramLimits, compile_catalog_program},
        catalog_source::{CatalogSourceError, CatalogSourceLimits},
        control_source::ControlSourceLimits,
        effect_evaluation::EffectEvaluationLimits,
        effect_reentry::RestoredEffectBindings,
        effect_session::{EffectSessionPoll, EffectSessionReplyError},
        effect_stack::{EffectStackFailure, EffectStackLimits},
        ir,
        pure_evaluation::{PureEvaluationLimits, PureValue},
        source_call::{SourceCallHost, SourceCallLimits},
    },
    syntax::{Diagnostic, parse},
};

pub mod diagnostics;

mod environment;
use environment::Env;

type Node = ir::Computation<(), String, (), ()>;
type Schema<'a> = OperationSchema<'a, String, &'a str, ScalarTypeSet, ScalarTypeSet, String>;
type Catalog<'a> = OperationCatalog<'a, String, &'a str, ScalarTypeSet, ScalarTypeSet, String>;
type Callback<'a> = dyn Fn(&Arguments, &mut Fuel) -> Result<ScalarValue, HostFailure> + 'a;

/// Owned parameter metadata. Optional parameters are omitted, never defaulted.
pub type Parameter = NamedParameter<String, ScalarTypeSet>;

/// A required, single-type parameter; use [`NamedParameter`] for richer domains.
pub fn parameter(name: impl Into<String>, ty: ScalarType) -> Parameter {
    NamedParameter::required(name.into(), ScalarTypeSet::only(ty))
}

/// Explicit compilation and execution budgets, separate from each session's Fuel.
/// Underlying HIR safety ceilings still apply to custom limits.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub max_source_bytes: usize,
    pub catalog: OperationCatalogLimits,
    pub compilation: CatalogProgramLimits,
    pub execution: EffectStackLimits,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_source_bytes: 16 * 1024,
            catalog: OperationCatalogLimits {
                max_operations: 64,
                max_parameters_per_operation: 16,
            },
            compilation: CatalogProgramLimits {
                expression: CatalogSourceLimits {
                    control: ControlSourceLimits {
                        source: SourceCallLimits {
                            max_source_nodes: 256,
                            max_source_depth: 32,
                            max_lowered_nodes: 256,
                            max_lowered_depth: 32,
                            max_arguments: 16,
                        },
                        max_bindings: 16,
                    },
                    max_calls: 32,
                },
                max_functions: 16,
                max_parameters: 8,
                max_compilation_nodes: 4096,
                max_expansion_depth: 16,
            },
            execution: EffectStackLimits {
                effects: EffectEvaluationLimits {
                    pure: PureEvaluationLimits {
                        max_nodes: 256,
                        max_depth: 32,
                        max_bindings: 16,
                    },
                    max_arguments: 16,
                    max_branches: 16,
                },
                max_frames: 16,
            },
        }
    }
}

struct Operation<'a> {
    name: String,
    parameters: Vec<Parameter>,
    result: ScalarTypeSet,
    result_type: ScalarType,
    capability: String,
    callback: Box<Callback<'a>>,
}

impl Operation<'_> {
    fn borrowed_parameters(&self) -> Vec<NamedParameter<&str, ScalarTypeSet>> {
        self.parameters
            .iter()
            .map(|p| NamedParameter {
                name: p.name.as_str(),
                domain: p.domain,
                required: p.required,
            })
            .collect()
    }

    fn with_schema<T>(&self, use_schema: impl FnOnce(&Schema<'_>) -> T) -> T {
        let parameters = self.borrowed_parameters();
        use_schema(&OperationSchema {
            key: self.name.clone(),
            parameters: &parameters,
            result: self.result,
            required_capability: self.capability.clone(),
        })
    }
}

/// Registration is separate from execution; building freezes schemas and callbacks.
/// Callbacks can borrow local state and need neither `Send` nor `'static`.
pub struct HostBuilder<'a> {
    host: Host<'a>,
}

impl Default for HostBuilder<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> HostBuilder<'a> {
    pub fn new() -> Self {
        Self::with_limits(Limits::default())
    }

    pub fn with_limits(limits: Limits) -> Self {
        Self {
            host: Host {
                operations: Vec::new(),
                limits,
                identity: Rc::new(()),
            },
        }
    }

    /// Register a named operation atomically. Failure leaves previous registrations intact.
    /// Native callbacks must bound their own work and charge the supplied original
    /// meter before costly work; fuel cannot preempt blocking or arbitrary Rust code.
    pub fn register(
        &mut self,
        name: &str,
        parameters: Vec<Parameter>,
        result: ScalarType,
        capability: &str,
        callback: impl Fn(&Arguments, &mut Fuel) -> Result<ScalarValue, HostFailure> + 'a,
    ) -> Result<(), RegistrationError> {
        if name.len() > 128
            || !name.split('.').all(valid_name)
            || crate::native::core::BinaryOperator::parse(name).is_some()
            || crate::native::core::UnaryOperator::parse(name).is_some()
            || matches!(
                name,
                "main"
                    | "strings"
                    | "recover"
                    | "bind"
                    | "choose"
                    | "loop"
                    | "fold"
                    | "field"
                    | "member"
                    | "seq"
                    | "all"
                    | "repeat"
            )
            || capability.is_empty()
            || capability.len() > 128
            || parameters
                .iter()
                .any(|p| p.name.len() > 64 || !valid_name(&p.name))
        {
            return Err(RegistrationError::InvalidName);
        }
        self.host.operations.push(Operation {
            name: name.into(),
            parameters,
            result: ScalarTypeSet::only(result),
            result_type: result,
            capability: capability.into(),
            callback: Box::new(callback),
        });
        let checked = self.host.catalog_view(|schemas| {
            OperationCatalog::new(1, schemas, self.host.limits.catalog).map(|_| ())
        });
        if let Err(error) = checked {
            self.host.operations.pop();
            return Err(RegistrationError::Catalog(error));
        }
        Ok(())
    }

    pub fn build(self) -> Host<'a> {
        self.host
    }
}

fn valid_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    name.len() <= 128
        && bytes
            .next()
            .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
        && !matches!(name, "fn" | "true" | "false" | "none")
}

/// An immutable, process-local registry. Programs are pinned to this host's identity.
pub struct Host<'a> {
    operations: Vec<Operation<'a>>,
    limits: Limits,
    identity: Rc<()>,
}

/// Read-only discovery of the scalar embedding profile, not an authority token
/// or a wire schema. The signatures borrow the same immutable host registry used
/// by compilation and dispatch; no operation callback is invoked by discovery.
#[derive(Debug)]
pub struct HostDescription<'a> {
    operations: Vec<OperationDescription<'a>>,
}

impl<'a> HostDescription<'a> {
    pub const fn protocol_version(&self) -> ProtocolVersion {
        ProtocolVersion::V1
    }

    pub fn operations(&self) -> &[OperationDescription<'a>] {
        &self.operations
    }
}

/// Borrowed operation metadata. A required capability describes policy; it does
/// not mean that capability has been granted to the inspecting caller.
#[derive(Clone, Copy, Debug)]
pub struct OperationDescription<'a> {
    name: &'a str,
    parameters: &'a [Parameter],
    result_type: ScalarType,
    required_capability: &'a str,
}

impl<'a> OperationDescription<'a> {
    pub fn name(&self) -> &'a str {
        self.name
    }
    pub fn parameters(&self) -> &'a [Parameter] {
        self.parameters
    }
    pub fn result_type(&self) -> ScalarType {
        self.result_type
    }
    pub fn required_capability(&self) -> &'a str {
        self.required_capability
    }
}

impl<'a> Host<'a> {
    /// Inspect supported operations without compilation, execution or granting
    /// capabilities. Native adapters can use `OperationCatalog::schemas` instead
    /// to expose their own non-scalar domains without conversion or copying.
    pub fn describe(&self) -> HostDescription<'_> {
        HostDescription {
            operations: self
                .operations
                .iter()
                .map(|operation| OperationDescription {
                    name: &operation.name,
                    parameters: &operation.parameters,
                    result_type: operation.result_type,
                    required_capability: &operation.capability,
                })
                .collect(),
        }
    }

    fn catalog_view<T>(&self, use_schemas: impl FnOnce(&[Schema<'_>]) -> T) -> T {
        let parameters: Vec<_> = self
            .operations
            .iter()
            .map(Operation::borrowed_parameters)
            .collect();
        let schemas: Vec<_> = self
            .operations
            .iter()
            .zip(&parameters)
            .map(|(op, params)| OperationSchema {
                key: op.name.clone(),
                parameters: params.as_slice(),
                result: op.result,
                required_capability: op.capability.clone(),
            })
            .collect();
        use_schemas(&schemas)
    }

    fn with_catalog<T>(&self, use_catalog: impl FnOnce(&Catalog<'_>) -> T) -> T {
        self.catalog_view(|schemas| {
            let catalog = OperationCatalog::new(1, schemas, self.limits.catalog)
                .expect("builder validated immutable catalog");
            use_catalog(&catalog)
        })
    }

    /// Compile `main` and cold-check all declarations using explicit compile-time grants.
    /// This calls the native parser/compiler without invoking any registered callback.
    pub fn compile(&self, source: &str, grants: &[&str]) -> Result<Program, CompileError> {
        if source.len() > self.limits.max_source_bytes {
            return Err(CompileError::SourceLimit);
        }
        let parsed = parse(source);
        if !parsed.diagnostics.is_empty() {
            return Err(CompileError::Syntax(parsed.diagnostics));
        }
        let declarations: Vec<_> = parsed.function.iter().chain(&parsed.helpers).collect();
        let grants: Vec<String> = grants.iter().map(|s| (*s).into()).collect();
        let node = self.with_catalog(|catalog| {
            compile_catalog_program::<(), _, (), (), _, _, _, _>(
                &declarations,
                "main",
                self.limits.compilation,
                &SourceCallHost {
                    catalog,
                    version: 1,
                    granted: &grants,
                },
                self,
            )
            .map(|(node, _)| node)
            .map_err(CompileError::Source)
        })?;
        Ok(Program {
            node,
            host: self.identity.clone(),
        })
    }

    /// Start an independent local scope with exactly the supplied fuel and live grants.
    /// The compiled program can be shared by many sessions borrowing this host.
    pub fn start<'h, 'p>(
        &'h self,
        program: &'p Program,
        grants: &[&str],
        fuel: Fuel,
    ) -> Result<Session<'p, 'h, 'a>, Error> {
        if !Rc::ptr_eq(&self.identity, &program.host) {
            return Err(Error::ForeignProgram);
        }
        let scope = LocalExecutionScope::new();
        let environment = Env {
            host: self,
            lease: scope.lease(),
            serial: std::cell::Cell::new(0),
            grants: grants.iter().map(|s| (*s).into()).collect(),
        };
        Ok(Session {
            vm: LocalVm::new(
                scope,
                &program.node,
                Vec::new(),
                fuel,
                self.limits.execution,
            ),
            environment,
            dispatched: None,
        })
    }
}

/// Owned admitted IR, with no source borrow or native side effects.
pub struct Program {
    node: Node,
    host: Rc<()>,
}

/// Declaration-ordered, evaluated scalar operands. Values are read-only to handlers.
#[derive(Debug)]
pub struct Arguments {
    values: Vec<(String, ScalarValue)>,
}
impl Arguments {
    pub fn get(&self, name: &str) -> Option<&ScalarValue> {
        self.values
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value)
    }
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&str, &ScalarValue)> {
        self.values
            .iter()
            .map(|(name, value)| (name.as_str(), value))
    }
}

#[derive(Clone)]
struct Identity {
    lease: LocalExecutionLease,
    serial: u64,
    operation: usize,
}
impl PartialEq for Identity {
    fn eq(&self, other: &Self) -> bool {
        self.lease.same_scope(&other.lease)
            && self.serial == other.serial
            && self.operation == other.operation
    }
}

/// A move-only pending effect. Inspecting it grants no execution authority.
///
/// ```compile_fail
/// use leselang_vm::embedding::Request;
/// fn duplicate(request: Request) { let _ = request.clone(); }
/// ```
#[must_use]
pub struct Request {
    identity: Identity,
    arguments: Arguments,
    name: String,
}
impl Request {
    pub fn operation(&self) -> &str {
        &self.name
    }
    pub fn arguments(&self) -> &Arguments {
        &self.arguments
    }
}
impl fmt::Debug for Request {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Request")
    }
}

/// Authorization is a point-in-time check; dispatch checks live authority again.
///
/// ```compile_fail
/// use leselang_vm::embedding::AuthorizedRequest;
/// fn duplicate(request: AuthorizedRequest) { let _ = request.clone(); }
/// ```
#[must_use]
#[derive(Debug)]
pub struct AuthorizedRequest {
    request: Request,
}

/// The exact local callback result. Only dispatch can create receipts.
///
/// ```compile_fail
/// use leselang_vm::embedding::Receipt;
/// fn duplicate(receipt: Receipt) { let _ = receipt.clone(); }
/// ```
#[must_use]
pub struct Receipt {
    identity: Identity,
    value: ScalarValue,
}
impl Receipt {
    pub fn value(&self) -> &ScalarValue {
        &self.value
    }
    /// Consume a refused/unused receipt without implicitly retrying the operation.
    pub fn into_value(self) -> ScalarValue {
        self.value
    }
}
impl fmt::Debug for Receipt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Receipt")
    }
}

#[must_use]
#[derive(Debug)]
pub enum Poll {
    Request(Request),
    Awaiting,
    Value(ScalarValue),
    Terminal(EffectSessionEnd),
}

type ScalarVm<'p, 'h> =
    LocalVm<'p, 'h, Node, (), RestoredEffectBindings<'p, ()>, Identity, ScalarTypeSet, ScalarValue>;

/// One local execution. Every method is synchronous; no implicit retries or refills.
#[must_use]
pub struct Session<'p, 'h, 'a> {
    vm: ScalarVm<'p, 'h>,
    environment: Env<'h, 'a>,
    dispatched: Option<u64>,
}

impl Session<'_, '_, '_> {
    pub fn status(&self) -> EffectSessionStatus {
        self.vm.status()
    }
    pub fn fuel_remaining(&self) -> u64 {
        self.vm.fuel_remaining()
    }
    pub fn lease(&self) -> LocalExecutionLease {
        self.vm.lease()
    }
    pub fn cancel(&mut self) -> bool {
        self.vm.cancel()
    }

    /// Replace live capability grants. Compiled grants never override this list.
    pub fn set_grants(&mut self, grants: &[&str]) {
        self.environment.grants = grants.iter().map(|s| (*s).into()).collect();
    }

    /// Evaluate until a request/value/terminal boundary; never dispatch host work.
    pub fn poll(&mut self) -> Result<Poll, Error> {
        match self
            .vm
            .poll(&self.environment)
            .map_err(|e| Error::Evaluation(Box::new(e)))?
        {
            EffectSessionPoll::Request(request) => Ok(Poll::Request(request)),
            EffectSessionPoll::Awaiting => Ok(Poll::Awaiting),
            EffectSessionPoll::Value(PureValue::Scalar(value)) => Ok(Poll::Value(value)),
            EffectSessionPoll::Terminal(end) => Ok(Poll::Terminal(end)),
            EffectSessionPoll::Value(_) => Err(Error::Unsupported),
        }
    }

    fn check_request(&self, identity: &Identity) -> Result<(), Error> {
        if self.vm.status() != EffectSessionStatus::Awaiting {
            return Err(Error::InvalidState);
        }
        self.environment.authorize(identity)?;
        if self.dispatched == Some(identity.serial) {
            return Err(Error::AlreadyDispatched);
        }
        Ok(())
    }

    pub fn authorize(&self, request: Request) -> Result<AuthorizedRequest, Rejected<Request>> {
        if let Err(error) = self.check_request(&request.identity) {
            return Err(Rejected {
                error,
                input: request,
            });
        }
        Ok(AuthorizedRequest { request })
    }

    /// Recheck scope, request and current grants, then invoke exactly one callback
    /// inside LocalVm's original metered host-work fence. No reply is accepted here.
    /// A refusal returns the untouched authorization; a callback failure is terminal.
    pub fn dispatch(&mut self, authorized: AuthorizedRequest) -> Result<Receipt, DispatchError> {
        if let Err(error) = self.check_request(&authorized.request.identity) {
            return Err(DispatchError {
                error,
                request: Some(authorized),
            });
        }
        let operation = &self.environment.host.operations[authorized.request.identity.operation];
        self.dispatched = Some(authorized.request.identity.serial);
        let outcome = self
            .vm
            .with_metered_host_outcome(authorized, |authorized, fuel| {
                let request = authorized.request;
                match (operation.callback)(&request.arguments, fuel) {
                    Ok(value) => HostWorkOutcome::Pending(Ok(Receipt {
                        identity: request.identity,
                        value,
                    })),
                    Err(error @ HostFailure::Failed(_)) => HostWorkOutcome::Failed(Err(error)),
                    Err(error @ HostFailure::Uncertain(_)) => {
                        HostWorkOutcome::Uncertain(Err(error))
                    }
                    Err(error @ HostFailure::Cancelled) => HostWorkOutcome::Cancelled(Err(error)),
                }
            });
        match outcome {
            Ok(Ok(receipt)) => Ok(receipt),
            Ok(Err(error)) => Err(DispatchError {
                error: Error::Host(error),
                request: None,
            }),
            Err(request) => Err(DispatchError {
                error: Error::InvalidState,
                request: Some(request),
            }),
        }
    }

    /// Validate identity, current authority, type and value with the original VM.
    /// A rejected receipt is returned intact, and an ordinary refusal stays pending.
    /// Successful acceptance only makes the VM Ready; call poll to resume.
    pub fn accept(&mut self, receipt: Receipt) -> Result<(), Rejected<Receipt>> {
        let Receipt { identity, value } = receipt;
        self.vm
            .try_accept(&identity, value, &self.environment)
            .map_err(|rejected| Rejected {
                error: Error::Acceptance(Box::new(rejected.error)),
                input: Receipt {
                    identity,
                    value: rejected.reply,
                },
            })
    }
}

#[derive(Debug)]
pub enum RegistrationError {
    InvalidName,
    Catalog(OperationCatalogError),
}
impl fmt::Display for RegistrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidName => f.write_str("invalid or oversized registration name"),
            Self::Catalog(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for RegistrationError {}

/// Syntax diagnostics retain source spans; compiler failures retain the native tree.
#[derive(Debug)]
pub enum CompileError {
    SourceLimit,
    Syntax(Vec<Diagnostic>),
    Source(CatalogSourceError<Error>),
}
impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourceLimit => f.write_str("source byte limit exceeded"),
            Self::Syntax(_) => f.write_str("source syntax rejected (see diagnostics)"),
            Self::Source(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for CompileError {}

/// A trusted handler's explicit terminal classification. Prior effects are retained.
#[derive(Debug)]
pub enum HostFailure {
    Failed(String),
    Uncertain(String),
    Cancelled,
}
impl fmt::Display for HostFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Failed(_) => "host operation failed",
            Self::Uncertain(_) => "host outcome uncertain",
            Self::Cancelled => "host cancelled",
        })
    }
}
impl std::error::Error for HostFailure {}

#[derive(Debug)]
pub enum Error {
    Denied,
    Closed,
    ForeignRequest,
    ForeignProgram,
    InvalidState,
    AlreadyDispatched,
    Unsupported,
    IdentityExhausted,
    Capture,
    Execution(String),
    Evaluation(Box<EffectStackFailure<Error>>),
    Acceptance(Box<EffectSessionReplyError<Error, crate::native::core::ScalarContractError>>),
    Host(HostFailure),
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Denied => "capability denied",
            Self::Closed => "execution scope closed",
            Self::ForeignRequest => "request does not belong to this execution",
            Self::ForeignProgram => "program belongs to another host",
            Self::InvalidState => "session is not awaiting work",
            Self::AlreadyDispatched => "request already dispatched",
            Self::Unsupported => "unsupported scalar host operation",
            Self::IdentityExhausted => "request identity exhausted",
            Self::Capture => "capture failed",
            Self::Execution(message) => message,
            Self::Evaluation(_) => "language execution failed (see native failure)",
            Self::Acceptance(error) => return error.fmt(f),
            Self::Host(error) => return error.fmt(f),
        })
    }
}
impl std::error::Error for Error {}

/// Refusal preserves the exact input. It never grants permission to retry work.
#[must_use]
pub struct Rejected<T> {
    pub error: Error,
    pub input: T,
}
impl<T> fmt::Debug for Rejected<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.error, f)
    }
}
impl<T> fmt::Display for Rejected<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.error.fmt(f)
    }
}
impl<T> std::error::Error for Rejected<T> {}

/// `request` is Some only when dispatch was refused before entering the callback.
#[must_use]
pub struct DispatchError {
    pub error: Error,
    pub request: Option<AuthorizedRequest>,
}
impl fmt::Debug for DispatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.error, f)
    }
}
impl fmt::Display for DispatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.error.fmt(f)
    }
}
impl std::error::Error for DispatchError {}
