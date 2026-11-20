//! Borrowed validation shared by both public semantic declaration versions.
use super::super::{CatalogDescription, DescriptionError, TypeDescription, identifier};
use super::*;

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
        limit(
            self.operations.len() <= super::super::MAX_OPERATIONS && self.tasks.len() <= MAX_TASKS,
        )?;
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
        TaskBody {
            summary: &self.summary,
            inputs: &self.inputs,
            steps: &self.steps,
            result: &self.result,
            execution: &self.execution,
        }
        .validate(catalog, operations, literal(&self.verification.result)?)?;
        self.verification.validate(&self.inputs)
    }
}

// Shared borrowed validation keeps both public versions source-compatible and
// rejects oversized owned declarations without first cloning their contents.
struct TaskBody<'a> {
    summary: &'a str,
    inputs: &'a [TaskInput],
    steps: &'a [TaskStep],
    result: &'a Term,
    execution: &'a ExecutionStyle,
}

impl TaskBody<'_> {
    fn validate(
        &self,
        catalog: &CatalogDescription,
        operations: &[OperationSemantics],
        result_type: ScalarType,
    ) -> Result<(), DescriptionError> {
        summary(self.summary)?;
        limit(self.inputs.len() <= 1 && self.steps.len() <= MAX_STEPS)?;
        require(!self.steps.is_empty())?;
        match self.execution {
            ExecutionStyle::NoText => require(self.inputs.is_empty())?,
            ExecutionStyle::NamedText | ExecutionStyle::Text => require(self.inputs.len() == 1)?,
        }
        for input in self.inputs {
            require(name(&input.name) && input.scalar_type == ScalarType::String)?;
        }
        let mut bindings = Bindings::new();
        let mut calls = 0;
        for step in self.steps {
            match step {
                TaskStep::Call {
                    binding,
                    operation,
                    arguments,
                } => {
                    calls += 1;
                    limit(
                        calls <= MAX_CALLS
                            && arguments.len() <= super::super::MAX_PARAMETERS_PER_OPERATION,
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
            self.term_type(catalog, &bindings, self.result)?
                == TypeDescription::Scalar(result_type),
        )?;
        Ok(())
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
        literal(&self.result)?;
        ReportBody {
            kind: &self.kind,
            max_fuel: self.max_fuel,
            predicates: &self.predicates,
        }
        .validate(inputs)
    }
}

struct ReportBody<'a> {
    kind: &'a str,
    max_fuel: u64,
    predicates: &'a [ReportPredicate],
}

impl ReportBody<'_> {
    fn validate(&self, inputs: &[TaskInput]) -> Result<(), DescriptionError> {
        require(!self.kind.is_empty() && self.max_fuel != 0 && !self.predicates.is_empty())?;
        limit(
            self.kind.len() <= 128
                && self.max_fuel <= 1_000_000
                && self.predicates.len() <= MAX_PREDICATES,
        )?;
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
                validate_path(path)?;
                Ok(match self {
                    Self::Length { .. } => Some(ScalarType::Integer),
                    Self::Scalar { scalar_type, .. } => Some(*scalar_type),
                    _ => None,
                })
            }
        }
    }
}

fn validate_path(path: &[ReportSegment]) -> Result<(), DescriptionError> {
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
    Ok(())
}

impl v2::ResultContract {
    fn validate(&self) -> Result<(), DescriptionError> {
        match self {
            Self::Exact(value) => {
                literal(value)?;
            }
            Self::Observed { scalar_type, path } => {
                require(matches!(
                    scalar_type,
                    ScalarType::String | ScalarType::Integer | ScalarType::Boolean
                ))?;
                require(
                    matches!(path.first(), Some(ReportSegment::Field { name }) if name != "result"),
                )?;
                validate_path(path)?;
            }
        }
        Ok(())
    }
}

impl v2::SemanticDescription {
    /// Validate the complete bounded task graph and observed evidence paths.
    pub fn validate(&self, catalog: &CatalogDescription) -> Result<(), DescriptionError> {
        catalog.validate()?;
        require(self.version == v2::VERSION)?;
        limit(
            self.tasks.len() <= super::MAX_TASKS
                && self.operations.len() <= super::super::MAX_OPERATIONS,
        )?;
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
            task.verification.result.validate()?;
            TaskBody {
                summary: &task.summary,
                inputs: &task.inputs,
                steps: &task.steps,
                result: &task.result,
                execution: &task.execution,
            }
            .validate(
                catalog,
                &self.operations,
                task.verification.result.scalar_type(),
            )?;
            ReportBody {
                kind: &task.verification.kind,
                max_fuel: task.verification.max_fuel,
                predicates: &task.verification.predicates,
            }
            .validate(&task.inputs)?;
        }
        Ok(())
    }
}
