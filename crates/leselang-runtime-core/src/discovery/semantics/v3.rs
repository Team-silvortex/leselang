//! Opt-in named string inputs. Earlier versions keep their single-input contract.
use super::{OperationSemantics, TaskInput, TaskStep, Term, v2};
use serde::{Deserialize, Serialize};

pub const VERSION: u32 = 3;
pub const MAX_INPUTS: usize = 8;

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
    pub verification: v2::ReportContract,
}

/// Distinct from the original enum so existing exhaustive matches stay valid.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStyle {
    NoText,
    NamedStrings,
}
