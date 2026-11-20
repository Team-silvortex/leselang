//! Semantic version 2 adds results tied to typed, independently selected report
//! evidence. Version 1 declarations and their exact-result codec stay unchanged.
//! Metadata remains an unchecked claim until validated and verified by a host.

use super::{
    ExecutionStyle, OperationSemantics, ReportPredicate, ReportSegment, TaskInput, TaskStep, Term,
};
use crate::{ScalarType, ScalarValue};
use serde::{Deserialize, Serialize};

pub const VERSION: u32 = 2;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticDescription {
    pub version: u32,
    pub operations: Vec<OperationSemantics>,
    pub tasks: Vec<TaskDescription>,
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
pub struct ReportContract {
    pub kind: String,
    pub max_fuel: u64,
    pub result: ResultContract,
    pub require_verified: bool,
    pub predicates: Vec<ReportPredicate>,
}

/// Exact values retain their v1 JSON shape. An observed result must equal the
/// selected evidence scalar; its path cannot select the report's own result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResultContract {
    Exact(ScalarValue),
    Observed {
        scalar_type: ScalarType,
        path: Vec<ReportSegment>,
    },
}

// A tagged wire enum enforces duplicate/unknown fields, including `none`.
// A unit variant would permit unknown fields with serde's internal tagging.
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum ResultWire {
    Integer {
        value: u64,
    },
    Boolean {
        value: bool,
    },
    String {
        value: String,
    },
    None {},
    Observed {
        scalar_type: ScalarType,
        path: Vec<ReportSegment>,
    },
}

impl Serialize for ResultContract {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let wire = match self {
            Self::Exact(ScalarValue::Integer(value)) => ResultWire::Integer { value: *value },
            Self::Exact(ScalarValue::Boolean(value)) => ResultWire::Boolean { value: *value },
            Self::Exact(ScalarValue::String(value)) => ResultWire::String {
                value: value.clone(),
            },
            Self::Exact(ScalarValue::None) => ResultWire::None {},
            Self::Exact(_) => return Err(serde::ser::Error::custom("unsupported semantic result")),
            Self::Observed { scalar_type, path } => ResultWire::Observed {
                scalar_type: *scalar_type,
                path: path.clone(),
            },
        };
        wire.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ResultContract {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(match ResultWire::deserialize(deserializer)? {
            ResultWire::Integer { value } => Self::Exact(ScalarValue::Integer(value)),
            ResultWire::Boolean { value } => Self::Exact(ScalarValue::Boolean(value)),
            ResultWire::String { value } => Self::Exact(ScalarValue::String(value)),
            ResultWire::None {} => Self::Exact(ScalarValue::None),
            ResultWire::Observed { scalar_type, path } => Self::Observed { scalar_type, path },
        })
    }
}

impl ResultContract {
    /// The declared type is metadata, not proof that a report has that type.
    pub fn scalar_type(&self) -> ScalarType {
        match self {
            Self::Exact(value) => value.scalar_type(),
            Self::Observed { scalar_type, .. } => *scalar_type,
        }
    }
}

impl From<super::SemanticDescription> for SemanticDescription {
    fn from(value: super::SemanticDescription) -> Self {
        Self {
            version: VERSION,
            operations: value.operations,
            tasks: value.tasks.into_iter().map(Into::into).collect(),
        }
    }
}
impl From<super::TaskDescription> for TaskDescription {
    fn from(value: super::TaskDescription) -> Self {
        Self {
            id: value.id,
            summary: value.summary,
            inputs: value.inputs,
            steps: value.steps,
            result: value.result,
            execution: value.execution,
            verification: value.verification.into(),
        }
    }
}
impl From<super::ReportContract> for ReportContract {
    fn from(value: super::ReportContract) -> Self {
        Self {
            kind: value.kind,
            max_fuel: value.max_fuel,
            result: ResultContract::Exact(value.result),
            require_verified: value.require_verified,
            predicates: value.predicates,
        }
    }
}
impl From<ScalarValue> for ResultContract {
    fn from(value: ScalarValue) -> Self {
        Self::Exact(value)
    }
}
