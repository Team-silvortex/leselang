use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

const GATE: &str = "project/gate/verify";
const SCAN: &str = "frontend/parser/scan";
const FENCE: &str = "runtime/engine/fence";
const TYPES: &str = "semantics/hir/types";
const SHIP: &str = "agent/entry/ship";
const FUTURE: &str = "future/shell/control";

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_leselang-dev"))
        .arg("tensor")
        .args(args)
        .args(["--json", "--root"])
        .arg(root)
        .output()
        .expect("run leselang-dev")
}

fn query(root: &Path, args: &[&str]) -> Value {
    let output = run(root, args);
    assert!(
        output.status.success(),
        "{args:?}: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(envelope["schema_version"], 1);
    assert_eq!(envelope["command"], args[0]);
    envelope["result"].clone()
}

fn failure(root: &Path, args: &[&str], code: i32) -> Vec<String> {
    let output = run(root, args);
    assert_eq!(output.status.code(), Some(code), "{args:?}");
    let body: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(body["valid"], false);
    assert!(body.get("result").is_none());
    serde_json::from_value(body["errors"].clone()).unwrap()
}

fn write_json(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
}

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn tool_and_language_resolve_independent_cargo_graphs() {
    for (directory, is_tool) in [
        (repo(), false),
        (PathBuf::from(env!("CARGO_MANIFEST_DIR")), true),
    ] {
        let output = Command::new(env!("CARGO"))
            .current_dir(&directory)
            .args([
                "metadata",
                "--format-version",
                "1",
                "--locked",
                "--offline",
                "--all-features",
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let graph: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            Path::new(graph["workspace_root"].as_str().unwrap()),
            directory.canonicalize().unwrap()
        );
        let project_packages: BTreeSet<_> = graph["packages"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|p| p["name"].as_str())
            .filter(|name| name.starts_with("leselang-"))
            .collect();
        if is_tool {
            assert_eq!(project_packages, BTreeSet::from(["leselang-dev"]));
        } else {
            assert!(project_packages.contains("leselang-vm"));
            assert!(!project_packages.contains("leselang-dev"));
        }
    }
}

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = loop {
            let dir = std::env::temp_dir().join(format!(
                "lese-tensor-cli-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&dir) {
                Ok(()) => break dir,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => panic!("create test directory: {e}"),
            }
        };
        let root = Self(dir);
        let status = root.0.join("project/status");
        fs::create_dir_all(status.join("cells")).unwrap();
        fs::create_dir(root.0.join("src")).unwrap();
        fs::create_dir(root.0.join("tests")).unwrap();
        fs::copy(
            repo().join("project/status/schema.json"),
            status.join("schema.json"),
        )
        .unwrap();

        // The first goal has two independent open prerequisites with different priorities.
        // The second goal is immediately actionable and must wait for the first goal's work.
        let rows = [
            (GATE, "active", 60, "critical", vec![SCAN, FENCE]),
            (SCAN, "active", 40, "active", vec![TYPES]),
            (TYPES, "stable", 100, "maintenance", vec![]),
            (FENCE, "usable", 75, "critical", vec![TYPES]),
            (
                "embedding/api/catalog",
                "stable",
                100,
                "maintenance",
                vec![TYPES],
            ),
            (
                "hosts/portable/wire",
                "stable",
                100,
                "maintenance",
                vec!["embedding/api/catalog"],
            ),
            (SHIP, "early", 10, "critical", vec!["hosts/portable/wire"]),
            (FUTURE, "early", 0, "deferred", vec![SHIP]),
        ];
        let mut architectures = vec![];
        for (id, maturity, progress, priority, dependencies) in rows {
            let segments: Vec<_> = id.split('/').collect();
            let (architecture, module, function) = (segments[0], segments[1], segments[2]);
            let owner = format!("src/{function}.rs");
            let test = format!("tests/{function}.rs");
            let definition = format!("pub fn {function}()");
            let proof = format!("fn verify_{function}()");
            fs::write(root.0.join(&owner), format!("{definition} {{}}\n")).unwrap();
            fs::write(root.0.join(&test), format!("#[test]\n{proof} {{}}\n")).unwrap();
            architectures.push(json!({
                "id":architecture, "label":architecture,
                "modules":[{"id":module,"owner":owner,"functions":[function]}]
            }));
            let lifecycle = if architecture == "future" {
                "target"
            } else {
                "current"
            };
            let cell = json!({
                "id":id,"architecture":architecture,"module":module,"function":function,
                "lifecycle":lifecycle,"status":maturity,"progress":progress,"priority":priority,"confidence":"medium",
                "contract":format!("Check the {function} contract."),"dependencies":dependencies,"blockers":[],
                "evidence":[{"kind":"source","path":owner,"anchor":definition},
                            {"kind":"unit-test","path":test,"anchor":proof}],
                "next_action":format!("Verify {function}."),"validation_command":"cargo test",
                "expected_artifact":test,"acceptance_criteria":[format!("Reject invalid {function} input.")]
            });
            write_json(
                &status.join("cells").join(format!("{architecture}.json")),
                &json!({"schema_version":1,"cells":[cell]}),
            );
        }
        write_json(
            &status.join("catalog.json"),
            &json!({
                "schema_version":1,"status_protocol":"dev-tensor-status-v1","architectures":architectures
            }),
        );
        write_json(
            &status.join("milestones.json"),
            &json!({
                "schema_version":1,"id":"contracts","objective":"Close the compiler and host contracts.","goals":[GATE,SHIP]
            }),
        );
        root
    }

    fn update(&self, id: &str, change: impl FnOnce(&mut Value)) {
        let arch = id.split('/').next().unwrap();
        let path = self.0.join(format!("project/status/cells/{arch}.json"));
        let mut shard: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let cell = shard["cells"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|cell| cell["id"] == id)
            .unwrap();
        change(cell);
        write_json(&path, &shard);
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// Use the flat catalog as an independent oracle for every level, including
// architectures with different numbers of functions per module.
fn check_rollups(node: &Value, entries: &[Value]) {
    let coordinate = node["coordinate"].as_str().unwrap();
    let prefix = format!("{coordinate}/");
    let rows: Vec<_> = entries
        .iter()
        .filter(|entry| {
            let id = entry["id"].as_str().unwrap();
            coordinate == "leselang" || coordinate == id || id.starts_with(&prefix)
        })
        .collect();
    let mut units = 0;
    let mut weighted = 0;
    let mut count = 0;
    let mut total = 0;
    for row in &rows {
        let progress = row["progress"].as_u64().unwrap();
        total += progress;
        let weight = match (row["lifecycle"].as_str(), row["priority"].as_str()) {
            (Some("retired"), _) | (_, Some("deferred")) => 0,
            (_, Some("maintenance")) => 1,
            (_, Some("active")) => 2,
            (_, Some("critical")) => 4,
            invalid => panic!("unknown priority {invalid:?}"),
        };
        units += weight;
        weighted += weight * progress;
        if weight > 0 {
            count += 1;
        }
    }
    let weakest = ["early", "active", "usable", "stable"]
        .into_iter()
        .find(|s| rows.iter().any(|c| c["status"] == *s));
    let closed = rows
        .iter()
        .filter(|c| c["status"] == "stable" && c["progress"] == 100)
        .count();
    assert_eq!(
        node["aggregate"],
        json!({
            "cells":rows.len(),"closed_cells":closed,"status":weakest,"mean_progress":total.checked_div(rows.len() as u64),
            "delivery_cells":count,"delivery_progress":weighted.checked_div(units)
        }),
        "{coordinate}"
    );
    if let Some(children) = node["children"].as_array() {
        for child in children {
            check_rollups(child, entries);
        }
    }
}

#[test]
fn repository_tree_and_summary_account_for_every_flat_cell() {
    let root = repo();
    let flat = query(&root, &["list"]);
    let entries = flat.as_array().unwrap();
    let tree = query(&root, &["tree"]);
    let summary = query(&root, &["summary"]);
    let validate = query(&root, &["validate"]);
    assert_eq!(
        validate["validated_cells"].as_u64(),
        Some(entries.len() as u64)
    );
    assert_eq!(tree["aggregate"], summary["aggregate"]);
    let evidence_count: usize = entries
        .iter()
        .map(|c| c["evidence"].as_array().unwrap().len())
        .sum();
    let by_kind = summary["evidence_counts"].as_object().unwrap();
    assert!(by_kind.contains_key("benchmark"));
    assert_eq!(
        by_kind.values().map(|n| n.as_u64().unwrap()).sum::<u64>(),
        evidence_count as u64
    );
    check_rollups(&tree, entries);

    let represented: BTreeSet<_> = entries
        .iter()
        .map(|cell| cell["id"].as_str().unwrap().rsplit_once('/').unwrap().0)
        .collect();
    let modules_for_status = |stable: bool| -> BTreeSet<_> {
        entries
            .iter()
            .filter(|cell| (cell["status"] == "stable") == stable)
            .map(|cell| cell["id"].as_str().unwrap().rsplit_once('/').unwrap().0)
            .collect()
    };
    assert_eq!(
        summary["modules"],
        json!({
            "represented": represented.len(),
            "all_matching_contracts_closed": represented.len() - modules_for_status(false).len()
        })
    );
    // A module with both open and closed contracts contributes to this
    // filtered count because the query explicitly selected its closed slice.
    let stable_modules = modules_for_status(true).len();
    let stable_summary = query(&root, &["summary", "--status", "stable"]);
    assert_eq!(
        stable_summary["modules"],
        json!({
            "represented": stable_modules, "all_matching_contracts_closed": stable_modules
        })
    );

    let mut reached = BTreeSet::new();
    for domain in tree["children"].as_array().unwrap() {
        let mut domain_count = 0;
        for module in domain["children"].as_array().unwrap() {
            let leaves = module["children"].as_array().unwrap();
            assert_eq!(
                module["aggregate"]["cells"].as_u64(),
                Some(leaves.len() as u64)
            );
            domain_count += leaves.len();
            for leaf in leaves {
                let id = leaf["coordinate"].as_str().unwrap();
                let (parent, _) = id.rsplit_once('/').unwrap();
                assert_eq!(Some(parent), module["coordinate"].as_str());
                assert_eq!(leaf["aggregate"]["cells"], 1);
                assert!(reached.insert(id));
            }
        }
        assert_eq!(
            domain["aggregate"]["cells"].as_u64(),
            Some(domain_count as u64)
        );
    }
    assert_eq!(
        reached,
        entries
            .iter()
            .map(|cell| cell["id"].as_str().unwrap())
            .collect()
    );
}

#[test]
fn filters_and_aggregate_use_only_matching_leaves_and_exclude_deferred_delivery() {
    let root = Fixture::new();
    let summary = query(&root.0, &["summary"]);
    let all = &summary["aggregate"];
    assert_eq!(
        all,
        &json!({"cells":8,"closed_cells":3,"status":"early","mean_progress":60,"delivery_cells":7,"delivery_progress":56})
    );
    let future = query(&root.0, &["summary", "--architecture", "future"]);
    assert_eq!(
        future["aggregate"],
        json!({"cells":1,"closed_cells":0,"status":"early","mean_progress":0,"delivery_cells":0,"delivery_progress":null})
    );
    assert_eq!(
        future["modules"],
        json!({"represented": 1, "all_matching_contracts_closed": 0})
    );
    let filtered = query(
        &root.0,
        &[
            "list",
            "--architecture",
            "runtime",
            "--module",
            "engine",
            "--function",
            "fence",
            "--status",
            "usable",
            "--priority",
            "critical",
        ],
    );
    assert_eq!(filtered.as_array().unwrap().len(), 1);
    assert_eq!(filtered[0]["id"], FENCE);
    let weakest = query(&root.0, &["weakest"]);
    assert_eq!(weakest[0]["id"], FUTURE);
    assert_eq!(weakest[1]["id"], SHIP);
    let empty = query(
        &root.0,
        &["tree", "--architecture", "future", "--priority", "critical"],
    );
    assert_eq!(empty["aggregate"]["cells"], 0);
    assert!(empty["aggregate"]["delivery_progress"].is_null());
    let empty_summary = query(
        &root.0,
        &[
            "summary",
            "--architecture",
            "future",
            "--priority",
            "critical",
        ],
    );
    assert_eq!(
        empty_summary["modules"],
        json!({"represented": 0, "all_matching_contracts_closed": 0})
    );
    assert_eq!(empty["aggregate"], empty_summary["aggregate"]);
    let printed = Command::new(env!("CARGO_BIN_EXE_leselang-dev"))
        .args(["tensor", "tree", "--architecture", "future", "--root"])
        .arg(&root.0)
        .output()
        .unwrap();
    assert!(printed.status.success());
    let rendered = String::from_utf8(printed.stdout).unwrap();
    assert!(rendered.contains("matching-contracts-closed=0/1"));
    assert!(rendered.contains("delivery-population=0/1"));
    assert!(failure(&root.0, &["inspect", "runtime/engine/missing"], 1)[0].contains("missing"));
    assert!(failure(&root.0, &["inspect", SCAN, "--architecture", "agent"], 1)[0].contains(SCAN));
    assert!(!failure(&root.0, &["list", "--priority"], 2).is_empty());
    for (flag, value) in [("--status", "stabel"), ("--priority", "urgent")] {
        let errors = failure(&root.0, &["list", flag, value], 2);
        assert!(!errors.is_empty(), "{flag} {value}");
    }
    // The test runner already adds --json to request machine-readable diagnostics.
    assert!(!failure(&root.0, &["summary", "--json"], 2).is_empty());
}

#[test]
fn ordered_goals_pick_the_highest_priority_ready_prerequisite_and_report_blocks() {
    let root = Fixture::new();
    let result = query(&root.0, &["next"]);
    assert_eq!(result["state"], "ready");
    assert_eq!(
        result["scope"],
        json!({
            "state_applies_to": "mainline",
            "mainline_goals": {"closed": 0, "total": 2},
            "registered_contracts": {"closed": 3, "total": 8}
        })
    );
    assert_eq!(result["goal"], GATE);
    let task = &result["task"];
    assert_eq!(task["coordinate"], FENCE);
    let inspected = query(&root.0, &["inspect", FENCE]);
    let cell = &inspected["cell"];
    for field in [
        "next_action",
        "validation_command",
        "expected_artifact",
        "acceptance_criteria",
    ] {
        assert_eq!(task[field], cell[field], "{field}");
    }
    assert_eq!(task["objective"], cell["contract"]);
    assert_eq!(task["current_evidence"], cell["evidence"]);
    assert_eq!(task["implementation_scope"]["owner"], inspected["owner"]);
    assert!(task["selection_reason"].as_str().unwrap().contains(GATE));
    assert_eq!(task["blocking_dependencies"], json!([]));

    let blocked = query(&root.0, &["blocked", "--architecture", "project"]);
    assert_eq!(blocked.as_array().unwrap().len(), 1);
    assert_eq!(blocked[0]["coordinate"], GATE);
    assert_eq!(blocked[0]["blocking_dependencies"], json!([SCAN, FENCE]));
    assert_eq!(inspected["dependents"], json!([GATE]));
    assert_eq!(
        query(&root.0, &["next", "--function", "scan"])["task"]["coordinate"],
        SCAN
    );
    let filtered = query(&root.0, &["next", "--architecture", "future"]);
    assert_eq!(filtered["state"], "no-ready-task");
    assert_eq!(filtered["scope"], result["scope"]);
    assert!(filtered["task"].is_null());

    root.update(FENCE, |c| {
        c["blockers"] = json!(["Await compatibility evidence."])
    });
    assert_eq!(query(&root.0, &["next"])["task"]["coordinate"], SCAN);
    let blocked = query(&root.0, &["blocked", "--architecture", "runtime"]);
    assert_eq!(
        blocked[0]["blockers"],
        json!(["Await compatibility evidence."])
    );
    root.update(SCAN, |c| c["blockers"] = json!(["Await parser contract."]));
    let later = query(&root.0, &["next"]);
    assert_eq!(later["goal"], SHIP);
    assert_eq!(later["task"]["coordinate"], SHIP);
    root.update(SHIP, |c| c["blockers"] = json!(["Await entry contract."]));
    let waiting = query(&root.0, &["next"]);
    assert_eq!(waiting["state"], "no-ready-task");
    assert!(waiting["task"].is_null());
}

#[test]
fn closing_mainline_goals_leaves_deferred_inventory_outside_the_work_queue() {
    let root = Fixture::new();
    for id in [FENCE, SCAN, GATE, SHIP] {
        root.update(id, |cell| {
            cell["status"] = json!("stable");
            cell["progress"] = json!(100);
        });
    }
    let result = query(&root.0, &["next"]);
    assert_eq!(result["state"], "complete");
    assert_eq!(
        result["scope"],
        json!({
            "state_applies_to": "mainline",
            "mainline_goals": {"closed": 2, "total": 2},
            "registered_contracts": {"closed": 7, "total": 8}
        })
    );
    assert!(result["task"].is_null());
    assert_eq!(query(&root.0, &["weakest"])[0]["id"], FUTURE);
    let summary = query(&root.0, &["summary"]);
    assert_eq!(summary["aggregate"]["delivery_progress"], 100);
    assert!(summary["aggregate"]["mean_progress"].as_u64().unwrap() < 100);
}

#[test]
fn an_intentionally_independent_inventory_entry_is_queryable() {
    let root = Fixture::new();
    // The future cell has no dependents. Detaching its one prerequisite isolates it.
    root.update(FUTURE, |cell| cell["dependencies"] = json!([]));
    assert!(
        failure(&root.0, &["validate"], 1)
            .iter()
            .any(|e| e.contains(FUTURE))
    );
    root.update(FUTURE, |cell| {
        cell["independent_reason"] = json!("This proposal awaits its first host contract.");
    });
    assert_eq!(
        query(&root.0, &["inspect", FUTURE])["dependency_closure"],
        json!([])
    );
    root.update(FUTURE, |cell| cell["dependencies"] = json!([SHIP]));
    assert!(
        failure(&root.0, &["validate"], 1)
            .iter()
            .any(|e| e.contains(FUTURE))
    );
}

#[test]
fn drift_or_missing_acceptance_blocks_ready_even_outside_query_filters() {
    let root = Fixture::new();
    assert_eq!(query(&root.0, &["next"])["task"]["coordinate"], FENCE);
    let source = root.0.join("src/fence.rs");
    let original = fs::read(&source).unwrap();
    fs::write(&source, "// renamed capability\n").unwrap();
    for command in ["next", "validate", "summary"] {
        let errors = failure(&root.0, &[command, "--architecture", "agent"], 1);
        assert!(
            errors
                .iter()
                .any(|e| e.contains(FENCE) && e.contains("anchor")),
            "{errors:?}"
        );
    }
    fs::write(source, original).unwrap();
    root.update(FENCE, |c| c["acceptance_criteria"] = json!([]));
    let errors = failure(&root.0, &["next"], 1);
    assert!(
        errors
            .iter()
            .any(|e| e.contains(FENCE) && e.contains("acceptance_criteria"))
    );
}
