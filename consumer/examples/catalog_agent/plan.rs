//! Auditable GUI strategies selected only after complete catalog validation.
//! A type signature alone does not tell an agent what an arbitrary operation does.
#[path = "calculator.rs"]
mod calculator;
#[path = "plan/catalog.rs"]
mod catalog;
#[path = "plan/json.rs"]
mod json;
#[path = "plan/legacy.rs"]
mod legacy;
#[path = "semantic/mod.rs"]
mod semantic;
#[cfg(test)]
#[path = "plan/tests.rs"]
mod tests;

use catalog::Catalog;
use json::decode;
pub use legacy::{generate, profile, task, verify, verify_for};
use leselang_runtime_core::discovery::{
    MAX_DESCRIPTION_BYTES,
    semantics::{ExecutionStyle, SemanticDescription, TaskStep, v2},
};
use std::collections::BTreeSet;

pub const MAX_REPORT_BYTES: usize = 128 * 1024;

#[derive(Debug, Eq, PartialEq)]
pub struct Verified {
    pub effects: usize,
    pub fuel_remaining: u64,
}

/// A validated plan owns the exact report contract used after execution.
pub struct PreparedPlan {
    pub profile: String,
    pub task: String,
    pub entry: String,
    pub takes_text: bool,
    pub input_name: Option<String>,
    pub execution: ExecutionStyle,
    pub required_capabilities: Vec<String>,
    pub expected_effects: usize,
    pub checks: Vec<String>,
    pub source: String,
    pub semantic: bool,
    pub semantic_version: Option<u32>,
    verification: Option<v2::ReportContract>,
}

impl PreparedPlan {
    pub fn verify(&self, report_bytes: &[u8], text: &str) -> Result<Verified, String> {
        if let Some(contract) = &self.verification {
            semantic::verify(
                &self.profile,
                self.expected_effects,
                contract,
                self.input_name.as_deref(),
                report_bytes,
                text,
            )
        } else if self.profile == "appkit" {
            verify(report_bytes, text)
        } else {
            verify_for(&self.profile, report_bytes, text)
        }
    }
}

/// Present semantic metadata is authoritative for planning, including errors.
/// Only an absent extension may use the original known-profile strategies.
pub fn prepare(catalog_bytes: &[u8], requested_task: &str) -> Result<PreparedPlan, String> {
    let value: serde_json::Value = decode(catalog_bytes, MAX_DESCRIPTION_BYTES, "catalog")?;
    if let Some(extension) = value.get("semantics") {
        let catalog = decode::<Catalog>(catalog_bytes, MAX_DESCRIPTION_BYTES, "catalog")?
            .into_semantic_description()?;
        // Dispatch strictly on the admitted version. A present unsupported or
        // malformed extension never falls back to a legacy profile strategy.
        let version = extension
            .get("version")
            .and_then(serde_json::Value::as_u64)
            .ok_or("catalog semantic metadata has an invalid version")?;
        let semantics = match version {
            1 => {
                let semantics: SemanticDescription = serde_json::from_value(extension.clone())
                    .map_err(
                        |_| "catalog semantic metadata has an invalid structure or field type",
                    )?;
                catalog
                    .to_json_with_semantics(&semantics)
                    .map_err(|error| format!("invalid semantic catalog metadata: {error}"))?;
                v2::SemanticDescription::from(semantics)
            }
            2 => {
                let semantics: v2::SemanticDescription = serde_json::from_value(extension.clone())
                    .map_err(
                        |_| "catalog semantic metadata has an invalid structure or field type",
                    )?;
                catalog
                    .to_json_with_semantics_v2(&semantics)
                    .map_err(|error| format!("invalid semantic catalog metadata: {error}"))?;
                semantics
            }
            _ => return Err("catalog semantic metadata version is unsupported".into()),
        };
        let selected = semantics
            .tasks
            .iter()
            .find(|task| task.id == requested_task)
            .ok_or("requested task is not declared by the discovered host")?;
        let mut capabilities = BTreeSet::new();
        let mut effects = 0;
        for step in &selected.steps {
            if let TaskStep::Call { operation, .. } = step {
                effects += 1;
                capabilities.insert(
                    catalog
                        .operations
                        .iter()
                        .find(|candidate| candidate.name == *operation)
                        .expect("validated operation")
                        .required_capability
                        .clone(),
                );
            }
        }
        Ok(PreparedPlan {
            profile: catalog.profile.clone(),
            task: selected.id.clone(),
            entry: "main".into(),
            takes_text: !selected.inputs.is_empty(),
            input_name: selected.inputs.first().map(|input| input.name.clone()),
            execution: selected.execution.clone(),
            required_capabilities: capabilities.into_iter().collect(),
            expected_effects: effects,
            checks: selected
                .verification
                .predicates
                .iter()
                .map(|predicate| predicate.id.clone())
                .collect(),
            source: semantic::generate(&catalog, selected)?,
            semantic: true,
            semantic_version: Some(version as u32),
            verification: Some(selected.verification.clone()),
        })
    } else {
        let profile = profile(catalog_bytes)?;
        let contract = task(&profile)?;
        if requested_task != contract.name {
            return Err("requested task does not match the discovered host contract".into());
        }
        let execution = if !contract.takes_text {
            ExecutionStyle::NoText
        } else if profile == "tauri.typescript" {
            ExecutionStyle::Text
        } else {
            ExecutionStyle::NamedText
        };
        Ok(PreparedPlan {
            profile,
            task: contract.name.into(),
            entry: contract.entry.into(),
            takes_text: contract.takes_text,
            input_name: contract.takes_text.then(|| "text".into()),
            execution,
            required_capabilities: vec!["gui.read".into(), "gui.write".into()],
            expected_effects: contract.expected_effects,
            checks: contract
                .checks
                .iter()
                .map(|check| (*check).into())
                .collect(),
            source: generate(catalog_bytes)?,
            semantic: false,
            semantic_version: None,
            verification: None,
        })
    }
}

/// Planning metadata is selected by the validated profile, not a CLI override.
#[derive(Debug, Eq, PartialEq)]
pub struct TaskContract {
    pub name: &'static str,
    pub entry: &'static str,
    pub expected_effects: usize,
    pub takes_text: bool,
    pub checks: &'static [&'static str],
}
