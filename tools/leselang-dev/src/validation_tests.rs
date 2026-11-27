use super::*;
use crate::model::{Architecture, Catalog, Milestones, Module, Shard};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    tensor: Tensor,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "leselang-tensor-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        fs::create_dir_all(root.join("project/status/cells")).unwrap();
        fs::write(
            root.join("test.rs"),
            "fn check() {}\n// 2024-02-29 integration receipt\n",
        )
        .unwrap();
        let architectures = DOMAINS
            .iter()
            .map(|id| Architecture {
                id: (*id).into(),
                label: (*id).into(),
                modules: vec![Module {
                    id: "core".into(),
                    owner: "crate team".into(),
                    functions: vec!["check".into()],
                }],
            })
            .collect();
        let cells = DOMAINS
            .iter()
            .map(|&id| Cell {
                id: format!("{id}/core/check"),
                architecture: id.into(),
                module: "core".into(),
                function: "check".into(),
                lifecycle: if id == "future" { "target" } else { "current" }.into(),
                status: "early".into(),
                progress: 0,
                priority: if id == "future" { "deferred" } else { "active" }.into(),
                confidence: "medium".into(),
                contract: "Check this slice.".into(),
                dependencies: vec![],
                independent_reason: (id != "project")
                    .then(|| "Self-contained fixture contract.".into()),
                blockers: vec![],
                acceptance_criteria: vec!["Result matches the fixture.".into()],
                evidence: vec![Evidence {
                    kind: "unit-test".into(),
                    path: "test.rs".into(),
                    anchor: "fn check()".into(),
                    date: None,
                    version: None,
                    platform: None,
                }],
                next_action: "Run a cross-platform check.".into(),
                validation_command: "cargo test".into(),
                expected_artifact: "Check receipt.".into(),
            })
            .collect();
        let tensor = Tensor {
            catalog: Catalog {
                schema_version: 1,
                status_protocol: "dev-tensor-status-v1".into(),
                architectures,
            },
            milestones: Milestones {
                schema_version: 1,
                id: "test-mainline".into(),
                objective: "Test one gate.".into(),
                goals: vec!["project/core/check".into()],
            },
            cells,
        };
        Self { root, tensor }
    }

    fn write(&self) {
        let dir = self.root.join("project/status");
        fs::write(
            dir.join("schema.json"),
            include_str!("../../../project/status/schema.json"),
        )
        .unwrap();
        fs::write(
            dir.join("catalog.json"),
            serde_json::to_vec(&self.tensor.catalog).unwrap(),
        )
        .unwrap();
        fs::write(
            dir.join("milestones.json"),
            serde_json::to_vec(&self.tensor.milestones).unwrap(),
        )
        .unwrap();
        for cell in &self.tensor.cells {
            let shard = Shard {
                schema_version: 1,
                cells: vec![cell.clone()],
            };
            fs::write(
                dir.join(format!("cells/{}.json", cell.architecture)),
                serde_json::to_vec(&shard).unwrap(),
            )
            .unwrap();
        }
    }

    fn rejected(&self, edit: impl FnOnce(&mut Tensor), expected: &str) {
        let mut changed = self.tensor.clone();
        edit(&mut changed);
        let errors = validate(&self.root, &changed);
        assert!(
            errors.iter().any(|s| s.contains(expected)),
            "expected {expected}: {errors:?}"
        );
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn loader_round_trips_v1_and_refuses_missing_unregistered_and_misfiled_shards() {
    let fixture = Fixture::new();
    fixture.write();
    let dir = fixture.root.join("project/status/cells");
    let wrong = dir.join("wrong.json");
    let correct = dir.join("project.json");
    let load = || Tensor::load(&fixture.root);
    assert!(validate(&fixture.root, &load().unwrap()).is_empty());
    fs::copy(&correct, &wrong).unwrap();
    assert!(load().unwrap_err().contains("unregistered cell shard"));
    fs::remove_file(wrong).unwrap();
    fs::write(&correct, fs::read(dir.join("frontend.json")).unwrap()).unwrap();
    assert!(load().unwrap_err().contains("belongs to architecture"));
    fs::remove_file(correct).unwrap();
    assert!(load().unwrap_err().contains("cells/project.json"));
}

#[test]
fn rejects_schema_drift_and_unknown_or_missing_fields() {
    let fixture = Fixture::new();
    let examples = [
        (
            "schema.json",
            vec!["$defs", "version", "const"],
            json!(2),
            "schema differs",
        ),
        (
            "catalog.json",
            vec!["schema_version"],
            json!(2),
            "unsupported schema_version",
        ),
        (
            "milestones.json",
            vec!["unexpected"],
            json!(true),
            "unknown field",
        ),
        (
            "cells/project.json",
            vec!["schema_version"],
            json!(3),
            "unsupported schema_version",
        ),
    ];
    for (name, pointer, replacement, reason) in examples {
        fixture.write();
        let path = fixture.root.join("project/status").join(name);
        let mut payload: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let mut target = &mut payload;
        for key in pointer {
            target = &mut target[key];
        }
        *target = replacement;
        fs::write(path, serde_json::to_vec(&payload).unwrap()).unwrap();
        assert!(Tensor::load(&fixture.root).unwrap_err().contains(reason));
    }
    let mut json = serde_json::to_value(&fixture.tensor.cells[0]).unwrap();
    json["evidence"][0]["date"] = Value::Null;
    assert!(serde_json::from_value::<Cell>(json.clone()).is_err());
    json.as_object_mut().unwrap().remove("next_action");
    json["evidence"][0].as_object_mut().unwrap().remove("date");
    assert!(
        serde_json::from_value::<Cell>(json)
            .unwrap_err()
            .to_string()
            .contains("next_action")
    );
}

#[test]
fn enforces_bidirectional_coverage_unique_dimensions_and_ownership() {
    let f = Fixture::new();
    f.rejected(
        |t| {
            t.catalog.architectures.pop();
        },
        "missing required architecture future",
    );
    f.rejected(
        |t| t.catalog.architectures[0].modules[0].owner.clear(),
        "module requires an owner",
    );
    f.rejected(
        |t| {
            t.catalog.architectures[0].modules[0]
                .functions
                .push("check".into())
        },
        "duplicate function",
    );
    f.rejected(
        |t| {
            t.cells.pop();
        },
        "missing cell for declared coordinate",
    );
    f.rejected(
        |t| t.cells[0].id = "project/new/check".into(),
        "orphan cell",
    );
    f.rejected(|t| t.cells[0].module = "typo".into(), "expected coordinate");
    f.rejected(|t| t.cells.push(t.cells[0].clone()), "duplicate cell");
}

#[test]
fn graph_and_mainline_checks_keep_open_and_cyclic_dependencies_visible() {
    let f = Fixture::new();
    f.rejected(
        |t| t.milestones.goals.push("project/z/x".into()),
        "unknown mainline goal",
    );
    f.rejected(
        |t| t.milestones.goals.push("future/core/check".into()),
        "ineligible mainline goal",
    );
    f.rejected(
        |t| t.cells[0].dependencies.push("project/missing/dep".into()),
        "unknown dependency",
    );
    f.rejected(
        |t| {
            t.cells[0].dependencies.push("frontend/core/check".into());
            t.cells[1].dependencies.push("project/core/check".into());
        },
        "dependency cycle",
    );
    f.rejected(
        |t| {
            t.cells[0].status = "stable".into();
            t.cells[0].progress = 100;
            t.cells[0].dependencies.push("frontend/core/check".into());
        },
        "open prerequisite",
    );
    f.rejected(
        |t| t.cells.last_mut().unwrap().priority = "active".into(),
        "explicit mainline promotion",
    );
}

#[test]
fn isolated_cells_require_named_independence_except_for_mainline_goals() {
    let mut f = Fixture::new();
    f.rejected(|t| t.cells[1].independent_reason = None, "isolated cell");
    f.rejected(
        |t| t.cells[1].independent_reason = Some(" ".into()),
        "must be nonblank",
    );
    f.rejected(
        |t| t.cells[0].dependencies.push("frontend/core/check".into()),
        "only used for an isolated cell",
    );
    let leaf = f.tensor.cells[1].id.clone();
    f.tensor.cells[0].dependencies.push(leaf);
    f.tensor.cells[1].independent_reason = None;
    assert!(validate(&f.root, &f.tensor).is_empty());
}

#[test]
fn missing_proof_criteria_and_incoherent_status_cannot_validate() {
    let f = Fixture::new();
    f.rejected(
        |t| t.cells[0].acceptance_criteria = vec![" ".into()],
        "acceptance_criteria",
    );
    f.rejected(|t| t.cells[0].progress = 101, "inconsistent status");
    f.rejected(|t| t.cells[0].status = "pass".into(), "invalid status");
    f.rejected(|t| t.cells[0].priority = "high".into(), "invalid priority");
    f.rejected(
        |t| t.cells[0].confidence = "infinite".into(),
        "invalid confidence",
    );
    f.rejected(
        |t| {
            t.cells[0].status = "stable".into();
            t.cells[0].progress = 100;
            t.cells[0].blockers.push("awaiting proof".into());
        },
        "zero blockers",
    );
    f.rejected(
        |t| {
            t.cells[0].status = "stable".into();
            t.cells[0].progress = 100;
            t.cells[0].evidence[0].kind = "source".into();
        },
        "stable requires a test",
    );
}

#[test]
fn receipts_require_identity_date_version_platform_and_local_anchors() {
    let mut f = Fixture::new();
    fs::create_dir_all(f.root.join("docs/acceptance")).unwrap();
    fs::copy(
        f.root.join("test.rs"),
        f.root.join("docs/acceptance/result.md"),
    )
    .unwrap();
    let receipt = &mut f.tensor.cells[0].evidence[0];
    receipt.kind = "ci-receipt".into();
    receipt.path = "docs/acceptance/result.md".into();
    receipt.anchor = "integration receipt".into();
    receipt.date = Some("2024-02-29".into());
    receipt.version = Some("tested revision".into());
    receipt.platform = Some("linux-x86_64".into());
    assert!(validate(&f.root, &f.tensor).is_empty());
    f.rejected(
        |t| t.cells[0].evidence[0].kind = "consumer-test".into(),
        "path does not identify",
    );
    f.rejected(
        |t| t.cells[0].evidence[0].platform = None,
        "historical receipt requires",
    );
    f.rejected(
        |t| t.cells[0].evidence[0].date = Some("2026-02-29".into()),
        "invalid evidence date",
    );
    f.rejected(
        |t| t.cells[0].evidence[0].date = Some("2024-03-01".into()),
        "receipt date missing",
    );
    f.rejected(
        |t| t.cells[0].evidence[0].anchor = "a missing test".into(),
        "anchor missing",
    );
    f.rejected(
        |t| t.cells[0].evidence[0].anchor = "e".into(),
        "ambiguous anchor",
    );
    f.rejected(
        |t| t.cells[0].evidence[0].path = "other.rs".into(),
        "other.rs",
    );
    f.rejected(
        |t| t.cells[0].evidence[0].path = "../test.rs".into(),
        "invalid repository-relative",
    );
    f.rejected(
        |t| t.cells[0].evidence[0].path = r"C:\tmp\receipt".into(),
        "invalid repository-relative",
    );
    f.rejected(
        |t| {
            let duplicate = t.cells[0].evidence[0].clone();
            t.cells[0].evidence.push(duplicate);
        },
        "duplicate evidence identity",
    );
}

#[cfg(unix)]
#[test]
fn evidence_cannot_follow_a_symlink_outside_the_repository() {
    let a = Fixture::new();
    let b = Fixture::new();
    std::os::unix::fs::symlink(b.root.join("test.rs"), a.root.join("foreign.rs")).unwrap();
    a.rejected(
        |t| t.cells[0].evidence[0].path = "foreign.rs".into(),
        "inside the repository",
    );
}
