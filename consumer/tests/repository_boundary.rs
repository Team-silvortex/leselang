use std::{collections::BTreeSet, path::Path, process::Command};

use serde_json::{Value, json};

const CRATES: &[&str] = &[
    "leselang-syntax",
    "leselang-runtime-core",
    "leselang-hir-core",
    "leselang-hir",
    "leselang-vm",
];

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
}

fn inside(root: &Path, value: &Value) -> Result<(), String> {
    let path = value.as_str().ok_or("missing source path")?;
    let path = Path::new(path)
        .canonicalize()
        .map_err(|error| format!("cannot resolve {path}: {error}"))?;
    if !path.starts_with(root) {
        return Err(format!("source escapes repository: {}", path.display()));
    }
    Ok(())
}

fn validate(metadata: &Value, root: &Path, expected: &[&str]) -> Result<(), String> {
    let root = root.canonicalize().map_err(|error| error.to_string())?;
    let packages = metadata["packages"].as_array().ok_or("missing packages")?;
    let mut names = BTreeSet::new();
    for package in packages {
        let name = package["name"].as_str().ok_or("missing package name")?;
        if !names.insert(name) {
            return Err("duplicate package identity".into());
        }
        if package["license"] != "MIT" {
            return Err(format!("non-MIT package: {name}"));
        }
        if package["version"] != env!("CARGO_PKG_VERSION") {
            return Err(format!("package version differs from consumer: {name}"));
        }
        inside(&root, &package["manifest_path"])?;
        let features = package["features"].as_object().ok_or("missing features")?;
        if features.contains_key("leserpent-profile") || features.contains_key("sqlite-journal") {
            return Err("product feature is available".into());
        }
        for dependency in package["dependencies"]
            .as_array()
            .ok_or("missing dependencies")?
        {
            let name = dependency["name"].as_str().ok_or("unnamed dependency")?;
            if let Some(path) = dependency.get("path") {
                if !CRATES.contains(&name) {
                    return Err(format!("unexpected local dependency: {name}"));
                }
                inside(&root, path)?;
            } else if !["serde", "serde_json"].contains(&name) {
                return Err(format!("unexpected external dependency: {name}"));
            }
        }
        for target in package["targets"].as_array().ok_or("missing targets")? {
            inside(&root, &target["src_path"])?;
        }
    }
    if names != expected.iter().copied().collect() {
        return Err("workspace package inventory changed".into());
    }
    Ok(())
}

fn metadata(manifest: &Path) -> Value {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .args(["--all-features", "--locked", "--offline", "--manifest-path"])
        .arg(manifest)
        .current_dir(root())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn language_consumer_and_gui_hosts_remain_separate_mit_and_repository_local() {
    validate(&metadata(&root().join("Cargo.toml")), root(), CRATES).unwrap();
    validate(
        &metadata(&root().join("consumer/Cargo.toml")),
        root(),
        &["leselang-native-source-consumer"],
    )
    .unwrap();
    validate(
        &metadata(&root().join("hosts/appkit/Cargo.toml")),
        root(),
        &["leselang-appkit-host"],
    )
    .unwrap();
    validate(
        &metadata(&root().join("hosts/macos-ax/Cargo.toml")),
        root(),
        &["leselang-macos-ax-host"],
    )
    .unwrap();
}

#[test]
fn typescript_host_remains_a_separate_workspace_with_scoped_framework_dependencies() {
    let metadata = metadata(&root().join("hosts/tauri/Cargo.toml"));
    let packages = metadata["packages"].as_array().unwrap();
    assert_eq!(packages.len(), 1);
    let package = &packages[0];
    assert_eq!(package["name"], "leselang-tauri-host");
    assert_eq!(package["license"], "MIT");
    assert_eq!(package["version"], env!("CARGO_PKG_VERSION"));
    inside(root(), &package["manifest_path"]).unwrap();
    for dependency in package["dependencies"].as_array().unwrap() {
        let name = dependency["name"].as_str().unwrap();
        if let Some(path) = dependency.get("path") {
            assert!(CRATES.contains(&name));
            inside(root(), path).unwrap();
        } else {
            assert!(["serde", "serde_json", "tauri", "tauri-build"].contains(&name));
            if name.starts_with("tauri") {
                assert_eq!(dependency["optional"], true);
                assert_eq!(dependency["target"], "cfg(target_os = \"macos\")");
            }
        }
    }
    for target in package["targets"].as_array().unwrap() {
        inside(root(), &target["src_path"]).unwrap();
    }
}

fn fixture() -> Value {
    json!({"packages": [{
        "name": "leselang-vm",
        "version": env!("CARGO_PKG_VERSION"),
        "license": "MIT",
        "manifest_path": root().join("crates/leselang-vm/Cargo.toml"),
        "features": {},
        "dependencies": [{
            "name": "leselang-hir",
            "path": root().join("crates/leselang-hir"),
        }],
        "targets": [{"src_path": root().join("crates/leselang-vm/src/lib.rs")}],
    }]})
}

#[test]
fn accepts_native_metadata() {
    validate(&fixture(), root(), &["leselang-vm"]).unwrap();
}

#[test]
fn rejects_mismatched_and_missing_package_versions() {
    let mut value = fixture();
    value["packages"][0]["version"] = json!("0.0.0-version-drift");
    assert!(validate(&value, root(), &["leselang-vm"]).is_err());
    value["packages"][0]
        .as_object_mut()
        .unwrap()
        .remove("version");
    assert!(validate(&value, root(), &["leselang-vm"]).is_err());
}

#[test]
fn rejects_sibling_source_dependencies_and_targets() {
    let mut value = fixture();
    value["packages"][0]["dependencies"][0]["path"] = json!(root().parent().unwrap());
    assert!(validate(&value, root(), &["leselang-vm"]).is_err());
    let mut value = fixture();
    value["packages"][0]["targets"][0]["src_path"] = json!(root().parent().unwrap());
    assert!(validate(&value, root(), &["leselang-vm"]).is_err());
}

#[test]
fn rejects_optional_product_dependencies_and_features() {
    let mut value = fixture();
    value["packages"][0]["dependencies"][0] = json!({
        "name": "leserpent-vm", "optional": true,
        "path": root().join("crates/leselang-vm"),
    });
    assert!(validate(&value, root(), &["leselang-vm"]).is_err());
    for feature in ["leserpent-profile", "sqlite-journal"] {
        let mut value = fixture();
        value["packages"][0]["features"] = json!({feature: []});
        assert!(validate(&value, root(), &["leselang-vm"]).is_err());
    }
}

#[test]
fn rejects_unreviewed_registry_dependencies_and_licenses() {
    let mut value = fixture();
    value["packages"][0]["dependencies"][0] = json!({"name": "rusqlite"});
    assert!(validate(&value, root(), &["leselang-vm"]).is_err());
    let mut value = fixture();
    value["packages"][0]["license"] = json!("Proprietary");
    assert!(validate(&value, root(), &["leselang-vm"]).is_err());
}

#[test]
fn rejects_duplicate_missing_and_extra_packages() {
    let mut value = fixture();
    let package = value["packages"][0].clone();
    value["packages"].as_array_mut().unwrap().push(package);
    assert!(validate(&value, root(), &["leselang-vm"]).is_err());
    assert!(validate(&fixture(), root(), CRATES).is_err());
    assert!(validate(&fixture(), root(), &[]).is_err());
}
