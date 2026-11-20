//! Optional host-authored descriptions of meaning, finite tasks and evidence.
//!
//! These owned serde values are unchecked metadata, never executable authority.
//! Decoders must bound input before deserialization and call [`SemanticDescription::validate`]
//! against the actual catalog before using them. Validation cannot establish that
//! an operation's claimed meaning is true, or that a live object or grant exists.
//! No source snippets, shell commands, scheduler, retries or native identities
//! are represented here. Hosts and consumers retain all runtime checks.

use crate::{ScalarType, ScalarValue};
use serde::{Deserialize, Serialize};

mod encode;
pub mod v2;
pub mod v3;
mod validation;

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
