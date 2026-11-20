//! Closed, bounded JSON encoding for v1 and v2 declarations.
use super::super::{DescriptionError, Encoder};
use super::*;

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
        output.string(super::super::scalar_name(*self))
    }
}
impl Encode for ScalarValue {
    fn encode(&self, output: &mut Encoder) -> Result<(), DescriptionError> {
        match self {
            Self::Integer(value) => object(output, &[("kind", &"integer"), ("value", value)]),
            Self::Boolean(value) => object(output, &[("kind", &"boolean"), ("value", value)]),
            Self::String(value) => object(output, &[("kind", &"string"), ("value", value)]),
            Self::None => object(output, &[("kind", &"none")]),
            Self::OptionalString(_) | Self::StringList(_) => {
                Err(DescriptionError::InvalidSemantics)
            }
        }
    }
}

impl SemanticDescription {
    pub(in crate::discovery) fn encode(
        &self,
        output: &mut Encoder,
    ) -> Result<(), DescriptionError> {
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

impl v2::SemanticDescription {
    pub(in crate::discovery) fn encode(
        &self,
        output: &mut Encoder,
    ) -> Result<(), DescriptionError> {
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

impl Encode for v2::TaskDescription {
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
impl Encode for v2::ReportContract {
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
impl Encode for v2::ResultContract {
    fn encode(&self, output: &mut Encoder) -> Result<(), DescriptionError> {
        match self {
            Self::Exact(value) => value.encode(output),
            Self::Observed { scalar_type, path } => object(
                output,
                &[
                    ("kind", &"observed"),
                    ("scalar_type", scalar_type),
                    ("path", path),
                ],
            ),
        }
    }
}
