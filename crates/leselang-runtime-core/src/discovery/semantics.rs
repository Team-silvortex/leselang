//! Optional host-authored descriptions of meaning, finite tasks and evidence.
//!
//! These owned serde values are unchecked metadata, never executable authority.
//! Decoders must bound input before deserialization and call [`SemanticDescription::validate`]
//! against the actual catalog before using them. Validation cannot establish that
//! an operation's claimed meaning is true, or that a live object or grant exists.
//! No source snippets, shell commands, scheduler, retries or native identities
//! are represented here. Hosts and consumers retain all runtime checks.

use super::{CatalogDescription, DescriptionError, Encoder, TypeDescription, identifier};
use crate::{ScalarType, ScalarValue};
use serde::{Deserialize, Serialize};

pub const VERSION: u32 = 1;
pub const MAX_TASKS: usize = 8;
pub const MAX_STEPS: usize = 21;
pub const MAX_CALLS: usize = 16;
pub const MAX_CONDITIONS: usize = 8;
pub const MAX_PREDICATES: usize = 64;
pub const MAX_PATH_SEGMENTS: usize = 8;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticDescription {
    pub version: u32,
    pub operations: Vec<OperationSemantics>,
    pub tasks: Vec<TaskDescription>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationSemantics {
    pub operation: String,
    pub summary: String,
    pub effect: EffectClass,
    pub completion: CompletionKind,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectClass {
    Read,
    Write,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletionKind {
    Value,
    Submission,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskDescription {
    pub id: String,
    pub summary: String,
    pub inputs: Vec<TaskInput>,
    pub steps: Vec<TaskStep>,
    pub result: Term,
    pub execution: ExecutionStyle,
    pub verification: ReportContract,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskInput {
    pub name: String,
    pub scalar_type: ScalarType,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStyle {
    NamedText,
    Text,
    NoText,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TaskStep {
    Call {
        binding: String,
        operation: String,
        arguments: Vec<TaskArgument>,
    },
    Guard {
        conditions: Vec<Condition>,
        failure: ScalarValue,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskArgument {
    pub name: String,
    pub value: Term,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Term {
    Literal { value: ScalarValue },
    Input { name: String },
    Binding { name: String },
    Field { binding: String, name: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Condition {
    Equal {
        left: Term,
        right: Term,
    },
    Contains {
        list: Term,
        value: Term,
    },
    IncreasedBy {
        before: Term,
        after: Term,
        amount: u64,
    },
    OptionalEquals {
        optional: Term,
        value: Term,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReportContract {
    pub kind: String,
    pub max_fuel: u64,
    pub result: ScalarValue,
    pub require_verified: bool,
    pub predicates: Vec<ReportPredicate>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReportPredicate {
    pub id: String,
    pub left: ReportTerm,
    pub relation: ReportRelation,
    pub right: ReportTerm,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReportRelation {
    Equal,
    AtMost,
    IncreasedBy { amount: u64 },
}

impl<'de> Deserialize<'de> for ReportRelation {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // An internally tagged unit variant ignores extra fields in serde.
        // Decode through an empty struct variant so unknown semantics cannot
        // disappear while preserving the ergonomic public `Equal` variant.
        #[derive(Deserialize)]
        #[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
        enum Wire {
            Equal {},
            AtMost {},
            IncreasedBy { amount: u64 },
        }
        match Wire::deserialize(deserializer)? {
            Wire::Equal {} => Ok(Self::Equal),
            Wire::AtMost {} => Ok(Self::AtMost),
            Wire::IncreasedBy { amount } => Ok(Self::IncreasedBy { amount }),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReportTerm {
    Literal {
        value: ScalarValue,
    },
    Input {
        name: String,
    },
    Path {
        path: Vec<ReportSegment>,
    },
    Length {
        path: Vec<ReportSegment>,
    },
    Scalar {
        path: Vec<ReportSegment>,
        scalar_type: ScalarType,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReportSegment {
    Field { name: String },
    Match { field: String, value: String },
}

fn invalid() -> DescriptionError {
    DescriptionError::InvalidSemantics
}
fn limit(ok: bool) -> Result<(), DescriptionError> {
    if ok {
        Ok(())
    } else {
        Err(DescriptionError::SemanticsLimit)
    }
}
fn require(ok: bool) -> Result<(), DescriptionError> {
    if ok { Ok(()) } else { Err(invalid()) }
}
fn name(value: &str) -> bool {
    identifier(value, 64) && value != "body" && !value.starts_with("_lese_")
}
fn summary(value: &str) -> Result<(), DescriptionError> {
    require(!value.is_empty())?;
    limit(value.len() <= 1024)
}
fn literal(value: &ScalarValue) -> Result<ScalarType, DescriptionError> {
    require(matches!(
        value,
        ScalarValue::Integer(_)
            | ScalarValue::Boolean(_)
            | ScalarValue::String(_)
            | ScalarValue::None
    ))?;
    limit(value.is_bounded())?;
    Ok(value.scalar_type())
}

type Bindings = Vec<(String, TypeDescription)>;

impl SemanticDescription {
    /// Validate bounds, references and scalar types without invoking host code.
    /// Result success describes consistency only; it never creates live grants.
    pub fn validate(&self, catalog: &CatalogDescription) -> Result<(), DescriptionError> {
        catalog.validate()?;
        require(self.version == VERSION)?;
        limit(self.operations.len() <= super::MAX_OPERATIONS && self.tasks.len() <= MAX_TASKS)?;
        require(!self.tasks.is_empty())?;
        for (index, operation) in self.operations.iter().enumerate() {
            require(
                catalog
                    .operations
                    .iter()
                    .any(|item| item.name == operation.operation),
            )?;
            require(
                !self.operations[..index]
                    .iter()
                    .any(|prior| prior.operation == operation.operation),
            )?;
            summary(&operation.summary)?;
        }
        for (index, task) in self.tasks.iter().enumerate() {
            require(
                name(&task.id) && !self.tasks[..index].iter().any(|prior| prior.id == task.id),
            )?;
            task.validate(catalog, &self.operations)?;
        }
        Ok(())
    }
}

impl TaskDescription {
    fn validate(
        &self,
        catalog: &CatalogDescription,
        operations: &[OperationSemantics],
    ) -> Result<(), DescriptionError> {
        summary(&self.summary)?;
        limit(self.inputs.len() <= 1 && self.steps.len() <= MAX_STEPS)?;
        require(!self.steps.is_empty())?;
        match self.execution {
            ExecutionStyle::NoText => require(self.inputs.is_empty())?,
            ExecutionStyle::NamedText | ExecutionStyle::Text => require(self.inputs.len() == 1)?,
        }
        for input in &self.inputs {
            require(name(&input.name) && input.scalar_type == ScalarType::String)?;
        }
        let result_type = literal(&self.verification.result)?;
        let mut bindings = Bindings::new();
        let mut calls = 0;
        for step in &self.steps {
            match step {
                TaskStep::Call {
                    binding,
                    operation,
                    arguments,
                } => {
                    calls += 1;
                    limit(
                        calls <= MAX_CALLS
                            && arguments.len() <= super::MAX_PARAMETERS_PER_OPERATION,
                    )?;
                    require(
                        name(binding)
                            && !self.inputs.iter().any(|input| input.name == *binding)
                            && !bindings.iter().any(|(name, _)| name == binding),
                    )?;
                    let schema = catalog
                        .operations
                        .iter()
                        .find(|schema| schema.name == *operation)
                        .ok_or_else(invalid)?;
                    require(
                        operations
                            .iter()
                            .any(|metadata| metadata.operation == *operation),
                    )?;
                    for (index, argument) in arguments.iter().enumerate() {
                        require(
                            !arguments[..index]
                                .iter()
                                .any(|prior| prior.name == argument.name),
                        )?;
                        let parameter = schema
                            .parameters
                            .iter()
                            .find(|parameter| parameter.name == argument.name)
                            .ok_or_else(invalid)?;
                        let ty = self.term_type(catalog, &bindings, &argument.value)?;
                        require(parameter.types.contains(&ty))?;
                    }
                    require(schema.parameters.iter().all(|parameter| {
                        !parameter.required
                            || arguments
                                .iter()
                                .any(|argument| argument.name == parameter.name)
                    }))?;
                    bindings.push((binding.clone(), schema.result.clone()));
                }
                TaskStep::Guard {
                    conditions,
                    failure,
                } => {
                    limit(conditions.len() <= MAX_CONDITIONS)?;
                    require(!conditions.is_empty() && literal(failure)? == result_type)?;
                    for condition in conditions {
                        self.condition(catalog, &bindings, condition)?;
                    }
                }
            }
        }
        require(
            self.term_type(catalog, &bindings, &self.result)?
                == TypeDescription::Scalar(result_type),
        )?;
        self.verification.validate(&self.inputs)
    }

    fn term_type(
        &self,
        catalog: &CatalogDescription,
        bindings: &Bindings,
        term: &Term,
    ) -> Result<TypeDescription, DescriptionError> {
        match term {
            Term::Literal { value } => literal(value).map(TypeDescription::Scalar),
            Term::Input { name } => self
                .inputs
                .iter()
                .find(|input| input.name == *name)
                .map(|input| TypeDescription::Scalar(input.scalar_type))
                .ok_or_else(invalid),
            Term::Binding { name } => bindings
                .iter()
                .find(|(candidate, _)| candidate == name)
                .map(|(_, ty)| ty.clone())
                .ok_or_else(invalid),
            Term::Field { binding, name } => {
                let Some((_, TypeDescription::Native(alias))) =
                    bindings.iter().find(|(candidate, _)| candidate == binding)
                else {
                    return Err(invalid());
                };
                catalog
                    .native_types
                    .iter()
                    .find(|ty| ty.name == *alias)
                    .and_then(|ty| ty.fields.iter().find(|field| field.name == *name))
                    .map(|field| TypeDescription::Scalar(field.scalar_type))
                    .ok_or_else(invalid)
            }
        }
    }

    fn condition(
        &self,
        catalog: &CatalogDescription,
        bindings: &Bindings,
        condition: &Condition,
    ) -> Result<(), DescriptionError> {
        let scalar = |term| match self.term_type(catalog, bindings, term)? {
            TypeDescription::Scalar(ty) => Ok(ty),
            TypeDescription::Native(_) => Err(invalid()),
        };
        match condition {
            Condition::Equal { left, right } => require(scalar(left)? == scalar(right)?),
            Condition::Contains { list, value } => require(
                scalar(list)? == ScalarType::StringList && scalar(value)? == ScalarType::String,
            ),
            Condition::IncreasedBy { before, after, .. } => require(
                scalar(before)? == ScalarType::Integer && scalar(after)? == ScalarType::Integer,
            ),
            Condition::OptionalEquals { optional, value } => require(
                scalar(optional)? == ScalarType::OptionalString
                    && scalar(value)? == ScalarType::String,
            ),
        }
    }
}

impl ReportContract {
    fn validate(&self, inputs: &[TaskInput]) -> Result<(), DescriptionError> {
        require(!self.kind.is_empty() && self.max_fuel != 0 && !self.predicates.is_empty())?;
        limit(
            self.kind.len() <= 128
                && self.max_fuel <= 1_000_000
                && self.predicates.len() <= MAX_PREDICATES,
        )?;
        literal(&self.result)?;
        for (index, predicate) in self.predicates.iter().enumerate() {
            require(
                name(&predicate.id)
                    && !self.predicates[..index]
                        .iter()
                        .any(|prior| prior.id == predicate.id),
            )?;
            let left = predicate.left.validate(inputs)?;
            let right = predicate.right.validate(inputs)?;
            match predicate.relation {
                ReportRelation::Equal => {
                    if let (Some(left), Some(right)) = (left, right) {
                        require(left == right)?;
                    }
                }
                ReportRelation::AtMost | ReportRelation::IncreasedBy { .. } => {
                    require(
                        left.is_none_or(|ty| ty == ScalarType::Integer)
                            && right.is_none_or(|ty| ty == ScalarType::Integer),
                    )?;
                }
            }
        }
        Ok(())
    }
}

impl ReportTerm {
    fn validate(&self, inputs: &[TaskInput]) -> Result<Option<ScalarType>, DescriptionError> {
        match self {
            Self::Literal { value } => literal(value).map(Some),
            Self::Input { name } => inputs
                .iter()
                .find(|input| input.name == *name)
                .map(|input| Some(input.scalar_type))
                .ok_or_else(invalid),
            Self::Path { path } | Self::Length { path } | Self::Scalar { path, .. } => {
                limit(path.len() <= MAX_PATH_SEGMENTS)?;
                require(!path.is_empty())?;
                for segment in path {
                    let field = match segment {
                        ReportSegment::Field { name } => name,
                        ReportSegment::Match { field, value } => {
                            limit(value.len() <= 4096)?;
                            field
                        }
                    };
                    require(!field.is_empty())?;
                    limit(field.len() <= 128)?;
                }
                Ok(match self {
                    Self::Length { .. } => Some(ScalarType::Integer),
                    Self::Scalar { scalar_type, .. } => Some(*scalar_type),
                    _ => None,
                })
            }
        }
    }
}

// A closed metadata encoder uses the catalog's bounded JSON writer, retaining
// its aggregate limit and escaping without a runtime JSON dependency. Serde is
// available to consumers; tests keep both representations exactly equivalent.
trait Encode {
    fn encode(&self, output: &mut Encoder) -> Result<(), DescriptionError>;
}

fn object(output: &mut Encoder, fields: &[(&str, &dyn Encode)]) -> Result<(), DescriptionError> {
    output.push("{")?;
    for (index, (name, value)) in fields.iter().enumerate() {
        if index != 0 {
            output.push(",")?;
        }
        output.string(name)?;
        output.push(":")?;
        value.encode(output)?;
    }
    output.push("}")
}

impl Encode for &str {
    fn encode(&self, output: &mut Encoder) -> Result<(), DescriptionError> {
        output.string(self)
    }
}
impl Encode for String {
    fn encode(&self, output: &mut Encoder) -> Result<(), DescriptionError> {
        output.string(self)
    }
}
impl Encode for bool {
    fn encode(&self, output: &mut Encoder) -> Result<(), DescriptionError> {
        output.push(if *self { "true" } else { "false" })
    }
}
impl Encode for u32 {
    fn encode(&self, output: &mut Encoder) -> Result<(), DescriptionError> {
        output.push(&self.to_string())
    }
}
impl Encode for u64 {
    fn encode(&self, output: &mut Encoder) -> Result<(), DescriptionError> {
        output.push(&self.to_string())
    }
}
impl<T: Encode> Encode for Vec<T> {
    fn encode(&self, output: &mut Encoder) -> Result<(), DescriptionError> {
        output.push("[")?;
        for (index, value) in self.iter().enumerate() {
            if index != 0 {
                output.push(",")?;
            }
            value.encode(output)?;
        }
        output.push("]")
    }
}
impl Encode for ScalarType {
    fn encode(&self, output: &mut Encoder) -> Result<(), DescriptionError> {
        output.string(super::scalar_name(*self))
    }
}
impl Encode for ScalarValue {
    fn encode(&self, output: &mut Encoder) -> Result<(), DescriptionError> {
        match self {
            Self::Integer(value) => object(output, &[("kind", &"integer"), ("value", value)]),
            Self::Boolean(value) => object(output, &[("kind", &"boolean"), ("value", value)]),
            Self::String(value) => object(output, &[("kind", &"string"), ("value", value)]),
            Self::None => object(output, &[("kind", &"none")]),
            Self::OptionalString(_) | Self::StringList(_) => Err(invalid()),
        }
    }
}

impl SemanticDescription {
    pub(super) fn encode(&self, output: &mut Encoder) -> Result<(), DescriptionError> {
        object(
            output,
            &[
                ("version", &self.version),
                ("operations", &self.operations),
                ("tasks", &self.tasks),
            ],
        )
    }
}
impl Encode for OperationSemantics {
    fn encode(&self, output: &mut Encoder) -> Result<(), DescriptionError> {
        object(
            output,
            &[
                ("operation", &self.operation),
                ("summary", &self.summary),
                ("effect", &self.effect),
                ("completion", &self.completion),
            ],
        )
    }
}
impl Encode for EffectClass {
    fn encode(&self, output: &mut Encoder) -> Result<(), DescriptionError> {
        output.string(match self {
            Self::Read => "read",
            Self::Write => "write",
        })
    }
}
impl Encode for CompletionKind {
    fn encode(&self, output: &mut Encoder) -> Result<(), DescriptionError> {
        output.string(match self {
            Self::Value => "value",
            Self::Submission => "submission",
        })
    }
}
impl Encode for ExecutionStyle {
    fn encode(&self, output: &mut Encoder) -> Result<(), DescriptionError> {
        output.string(match self {
            Self::NamedText => "named_text",
            Self::Text => "text",
            Self::NoText => "no_text",
        })
    }
}
impl Encode for TaskDescription {
    fn encode(&self, output: &mut Encoder) -> Result<(), DescriptionError> {
        object(
            output,
            &[
                ("id", &self.id),
                ("summary", &self.summary),
                ("inputs", &self.inputs),
                ("steps", &self.steps),
                ("result", &self.result),
                ("execution", &self.execution),
                ("verification", &self.verification),
            ],
        )
    }
}
impl Encode for TaskInput {
    fn encode(&self, output: &mut Encoder) -> Result<(), DescriptionError> {
        object(
            output,
            &[("name", &self.name), ("scalar_type", &self.scalar_type)],
        )
    }
}
impl Encode for TaskStep {
    fn encode(&self, output: &mut Encoder) -> Result<(), DescriptionError> {
        match self {
            Self::Call {
                binding,
                operation,
                arguments,
            } => object(
                output,
                &[
                    ("kind", &"call"),
                    ("binding", binding),
                    ("operation", operation),
                    ("arguments", arguments),
                ],
            ),
            Self::Guard {
                conditions,
                failure,
            } => object(
                output,
                &[
                    ("kind", &"guard"),
                    ("conditions", conditions),
                    ("failure", failure),
                ],
            ),
        }
    }
}
impl Encode for TaskArgument {
    fn encode(&self, output: &mut Encoder) -> Result<(), DescriptionError> {
        object(output, &[("name", &self.name), ("value", &self.value)])
    }
}
impl Encode for Term {
    fn encode(&self, output: &mut Encoder) -> Result<(), DescriptionError> {
        match self {
            Self::Literal { value } => object(output, &[("kind", &"literal"), ("value", value)]),
            Self::Input { name } => object(output, &[("kind", &"input"), ("name", name)]),
            Self::Binding { name } => object(output, &[("kind", &"binding"), ("name", name)]),
            Self::Field { binding, name } => object(
                output,
                &[("kind", &"field"), ("binding", binding), ("name", name)],
            ),
        }
    }
}
impl Encode for Condition {
    fn encode(&self, output: &mut Encoder) -> Result<(), DescriptionError> {
        match self {
            Self::Equal { left, right } => object(
                output,
                &[("kind", &"equal"), ("left", left), ("right", right)],
            ),
            Self::Contains { list, value } => object(
                output,
                &[("kind", &"contains"), ("list", list), ("value", value)],
            ),
            Self::IncreasedBy {
                before,
                after,
                amount,
            } => object(
                output,
                &[
                    ("kind", &"increased_by"),
                    ("before", before),
                    ("after", after),
                    ("amount", amount),
                ],
            ),
            Self::OptionalEquals { optional, value } => object(
                output,
                &[
                    ("kind", &"optional_equals"),
                    ("optional", optional),
                    ("value", value),
                ],
            ),
        }
    }
}
impl Encode for ReportContract {
    fn encode(&self, output: &mut Encoder) -> Result<(), DescriptionError> {
        object(
            output,
            &[
                ("kind", &self.kind),
                ("max_fuel", &self.max_fuel),
                ("result", &self.result),
                ("require_verified", &self.require_verified),
                ("predicates", &self.predicates),
            ],
        )
    }
}
impl Encode for ReportPredicate {
    fn encode(&self, output: &mut Encoder) -> Result<(), DescriptionError> {
        object(
            output,
            &[
                ("id", &self.id),
                ("left", &self.left),
                ("relation", &self.relation),
                ("right", &self.right),
            ],
        )
    }
}
impl Encode for ReportRelation {
    fn encode(&self, output: &mut Encoder) -> Result<(), DescriptionError> {
        match self {
            Self::Equal => object(output, &[("kind", &"equal")]),
            Self::AtMost => object(output, &[("kind", &"at_most")]),
            Self::IncreasedBy { amount } => {
                object(output, &[("kind", &"increased_by"), ("amount", amount)])
            }
        }
    }
}
impl Encode for ReportTerm {
    fn encode(&self, output: &mut Encoder) -> Result<(), DescriptionError> {
        match self {
            Self::Literal { value } => object(output, &[("kind", &"literal"), ("value", value)]),
            Self::Input { name } => object(output, &[("kind", &"input"), ("name", name)]),
            Self::Path { path } => object(output, &[("kind", &"path"), ("path", path)]),
            Self::Length { path } => object(output, &[("kind", &"length"), ("path", path)]),
            Self::Scalar { path, scalar_type } => object(
                output,
                &[
                    ("kind", &"scalar"),
                    ("path", path),
                    ("scalar_type", scalar_type),
                ],
            ),
        }
    }
}
impl Encode for ReportSegment {
    fn encode(&self, output: &mut Encoder) -> Result<(), DescriptionError> {
        match self {
            Self::Field { name } => object(output, &[("kind", &"field"), ("name", name)]),
            Self::Match { field, value } => object(
                output,
                &[("kind", &"match"), ("field", field), ("value", value)],
            ),
        }
    }
}
