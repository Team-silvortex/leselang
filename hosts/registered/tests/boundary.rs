use serde_json::Value;
use std::{collections::BTreeSet, path::Path, process::Command};

const LOCAL_PACKAGES: &[&str] = &[
    "leselang-registered-host",
    "leselang-vm",
    "leselang-hir",
    "leselang-hir-core",
    "leselang-runtime-core",
    "leselang-syntax",
];

fn framework_package(name: &str) -> bool {
    [
        "tauri", "wry", "tao", "webkit", "webview", "gtk", "gdk", "gio", "atk",
    ]
    .iter()
    .any(|prefix| name.starts_with(prefix))
        || name == "leselang-tauri-host"
}

#[test]
fn complete_registered_worker_dependency_graph_has_no_desktop_framework() {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repository = directory.parent().unwrap().parent().unwrap();
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .args([
            "metadata",
            "--format-version",
            "1",
            "--all-features",
            "--locked",
            "--offline",
            "--manifest-path",
        ])
        .arg(directory.join("Cargo.toml"))
        .current_dir(repository)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: Value = serde_json::from_slice(&output.stdout).unwrap();
    let packages = metadata["packages"].as_array().unwrap();
    let mut local = BTreeSet::new();
    for package in packages {
        let name = package["name"].as_str().unwrap();
        assert!(!framework_package(name), "desktop package resolved: {name}");
        if package["source"].is_null() {
            assert!(
                LOCAL_PACKAGES.contains(&name),
                "unexpected local package: {name}"
            );
            local.insert(name);
            let manifest = Path::new(package["manifest_path"].as_str().unwrap())
                .canonicalize()
                .unwrap();
            let expected = if name == "leselang-registered-host" {
                directory.to_path_buf()
            } else {
                repository.join("crates").join(name)
            };
            assert_eq!(
                manifest,
                expected.join("Cargo.toml").canonicalize().unwrap()
            );
        }
    }
    assert_eq!(local, LOCAL_PACKAGES.iter().copied().collect());
    let resolved = metadata["resolve"]["nodes"].as_array().unwrap();
    assert_eq!(
        resolved.len(),
        packages.len(),
        "inspect the full resolved graph"
    );
    let members = metadata["workspace_members"].as_array().unwrap();
    assert_eq!(members.len(), 1, "the worker is a separate workspace");
    let package = packages
        .iter()
        .find(|package| package["name"] == "leselang-registered-host")
        .unwrap();
    assert_eq!(members[0], package["id"]);
    assert!(package["targets"].as_array().unwrap().iter().any(|target| {
        target["name"] == "leselang-agent-worker"
            && target["kind"]
                .as_array()
                .unwrap()
                .iter()
                .any(|kind| kind == "bin")
    }));

    // An old copied lockfile could otherwise retain the framework dependency
    // graph even though the selected executable no longer reaches it.
    let lockfile = std::fs::read_to_string(directory.join("Cargo.lock")).unwrap();
    for line in lockfile.lines() {
        if let Some(name) = line
            .strip_prefix("name = \"")
            .and_then(|line| line.strip_suffix('"'))
        {
            assert!(
                !framework_package(name),
                "desktop package remains locked: {name}"
            );
        }
    }
}
