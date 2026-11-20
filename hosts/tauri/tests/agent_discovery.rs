//! The interactive catalog remains available without a desktop or JS process.
use std::process::Command;

#[test]
fn agent_catalog_cli_is_complete_offline_and_matches_the_public_api() {
    let output = Command::new(env!("CARGO_BIN_EXE_leselang-tauri-host"))
        .arg("--describe-agent")
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert_eq!(
        output.stdout,
        leselang_tauri_host::describe_agent().unwrap().as_bytes()
    );
    assert_eq!(output.stdout.last(), Some(&b'\n'));
    assert_eq!(
        output.stdout.iter().filter(|byte| **byte == b'\n').count(),
        1
    );
}

#[test]
fn agent_catalog_cli_refuses_execution_options_without_partial_output() {
    for args in [
        vec!["--describe-agent", "--agent"],
        vec!["--describe-agent", "--text", "literal"],
        vec!["--describe-agent", "--run", "missing"],
        vec!["--describe-agent", "--report-json"],
        vec!["--hold", "--describe-agent"],
        vec!["--describe-agent-observed", "--agent"],
        vec!["--describe-agent-observed", "--run", "missing"],
        vec!["--describe-agent-observed", "--text", "literal"],
        vec!["--describe-agent-observed", "--describe-agent"],
        vec!["--hold", "--describe-agent-observed"],
        vec!["--describe-agent-board", "--agent-board"],
        vec!["--describe-agent-board", "--agent"],
        vec!["--describe-agent-board", "--text", "literal"],
        vec!["--describe-agent-board", "--run", "missing"],
        vec!["--describe-agent-board", "--report-json"],
        vec!["--hold", "--describe-agent-board"],
        vec!["--describe-agent-article", "--agent-article"],
        vec!["--describe-agent-article", "--agent"],
        vec!["--describe-agent-article", "--text", "literal"],
        vec!["--describe-agent-article", "--run", "missing"],
        vec!["--describe-agent-article", "--report-json"],
        vec!["--describe-agent-article", "--describe-agent-board"],
        vec!["--hold", "--describe-agent-article"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_leselang-tauri-host"))
            .args(args)
            .env("PATH", "")
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
}

#[test]
fn article_catalog_is_an_independent_offline_v3_contract() {
    let output = Command::new(env!("CARGO_BIN_EXE_leselang-tauri-host"))
        .arg("--describe-agent-article")
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert_eq!(
        output.stdout,
        leselang_tauri_host::describe_agent_article()
            .unwrap()
            .as_bytes()
    );
    assert_eq!(
        output.stdout,
        include_bytes!("../../../fixtures/host-semantics/tauri-article-semantics-v3.json")
    );
    assert_eq!(
        output.stdout.iter().filter(|byte| **byte == b'\n').count(),
        1
    );
    let article: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(article["profile"], "tauri.article");
    assert_eq!(article["semantics"]["version"], 3);
    let tasks = article["semantics"]["tasks"].as_array().unwrap();
    assert_eq!(
        tasks
            .iter()
            .map(|task| task["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "read_title",
            "read_content",
            "publish_article",
            "check_article"
        ]
    );
    assert_eq!(
        tasks[2]["inputs"],
        serde_json::json!([
            {"name":"title", "scalar_type":"string"},
            {"name":"content", "scalar_type":"string"},
        ])
    );
    assert_eq!(tasks[2]["execution"], "named_strings");
    let form: serde_json::Value =
        serde_json::from_str(&leselang_tauri_host::describe_agent_observed().unwrap()).unwrap();
    assert_eq!(article["operations"], form["operations"]);
    assert_eq!(article["native_types"], form["native_types"]);
    assert_eq!(
        leselang_tauri_host::describe_agent_observed()
            .unwrap()
            .as_bytes(),
        include_bytes!("../../../fixtures/host-semantics/tauri-agent-semantics-v2.json")
    );
    assert_eq!(
        leselang_tauri_host::describe_agent_board()
            .unwrap()
            .as_bytes(),
        include_bytes!("../../../fixtures/host-semantics/tauri-board-semantics-v2.json")
    );
}

#[test]
fn board_catalog_is_an_independent_offline_application_contract() {
    let output = Command::new(env!("CARGO_BIN_EXE_leselang-tauri-host"))
        .arg("--describe-agent-board")
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert_eq!(
        output.stdout,
        leselang_tauri_host::describe_agent_board()
            .unwrap()
            .as_bytes()
    );
    assert_eq!(
        output.stdout,
        include_bytes!("../../../fixtures/host-semantics/tauri-board-semantics-v2.json")
    );
    assert_eq!(
        output.stdout.iter().filter(|byte| **byte == b'\n').count(),
        1
    );
    let board: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(board["profile"], "tauri.taskboard");
    assert_eq!(board["semantics"]["version"], 2);
    assert_eq!(
        board["semantics"]["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|task| task["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "read_review_status",
            "read_archive_status",
            "complete_review",
            "complete_archive",
            "check_board"
        ]
    );
    let form: serde_json::Value =
        serde_json::from_str(&leselang_tauri_host::describe_agent_observed().unwrap()).unwrap();
    assert_eq!(form["profile"], "tauri.typescript");
    assert_eq!(board["operations"], form["operations"]);
    assert_eq!(board["native_types"], form["native_types"]);
    assert!(
        board["semantics"]["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|task| task["inputs"] == serde_json::json!([]))
    );
}

#[test]
fn observed_catalog_is_offline_and_preserves_the_v1_entry() {
    let output = Command::new(env!("CARGO_BIN_EXE_leselang-tauri-host"))
        .arg("--describe-agent-observed")
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert_eq!(
        output.stdout,
        leselang_tauri_host::describe_agent_observed()
            .unwrap()
            .as_bytes()
    );
    assert_eq!(
        output.stdout.iter().filter(|byte| **byte == b'\n').count(),
        1
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["semantics"]["version"], 2);
    assert_eq!(value["semantics"]["tasks"].as_array().unwrap().len(), 5);
    let previous: serde_json::Value =
        serde_json::from_str(&leselang_tauri_host::describe_agent().unwrap()).unwrap();
    assert_eq!(previous["semantics"]["version"], 1);
    assert_eq!(previous["semantics"]["tasks"].as_array().unwrap().len(), 4);
}
