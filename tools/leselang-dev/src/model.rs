use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path};

#[derive(Clone, Debug)]
pub struct Tensor {
    pub catalog: Catalog,
    pub milestones: Milestones,
    pub cells: Vec<Cell>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub schema_version: u32,
    pub status_protocol: String,
    pub architectures: Vec<Architecture>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Architecture {
    pub id: String,
    pub label: String,
    pub modules: Vec<Module>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Module {
    pub id: String,
    pub owner: String,
    pub functions: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Milestones {
    pub schema_version: u32,
    pub id: String,
    pub objective: String,
    pub goals: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Shard {
    pub schema_version: u32,
    pub cells: Vec<Cell>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cell {
    pub id: String,
    pub architecture: String,
    pub module: String,
    pub function: String,
    pub lifecycle: String,
    pub status: String,
    pub progress: u8,
    pub priority: String,
    pub confidence: String,
    pub contract: String,
    pub dependencies: Vec<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "optional_text"
    )]
    pub independent_reason: Option<String>,
    pub blockers: Vec<String>,
    pub evidence: Vec<Evidence>,
    pub next_action: String,
    pub validation_command: String,
    pub expected_artifact: String,
    pub acceptance_criteria: Vec<String>,
}

impl Cell {
    pub fn closed(&self) -> bool {
        self.status == "stable" && self.progress == 100
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub kind: String,
    pub path: String,
    pub anchor: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "optional_text"
    )]
    pub date: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "optional_text"
    )]
    pub version: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "optional_text"
    )]
    pub platform: Option<String>,
}

pub const EVIDENCE_KINDS: &[(&str, &str)] = &[
    ("documentation-contract", "specification"),
    ("source", "implementation"),
    ("unit-test", "automated-coverage"),
    ("integration-test", "automated-coverage"),
    ("compatibility-fixture", "automated-coverage"),
    ("benchmark", "automated-coverage"),
    ("consumer-test", "independent-consumer"),
    ("native-acceptance", "recorded-environment"),
    ("ci-receipt", "recorded-environment"),
];

impl Evidence {
    pub fn level(&self) -> Option<&'static str> {
        EVIDENCE_KINDS
            .iter()
            .find_map(|&(kind, level)| (kind == self.kind).then_some(level))
    }
}

// Missing metadata is allowed for source anchors. Explicit null is not a v1 string.
fn optional_text<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    String::deserialize(deserializer).map(Some)
}

pub fn identifier(value: &str) -> bool {
    value.split('-').all(|piece| {
        !piece.is_empty()
            && piece
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
    })
}

// root must already be canonical. Use one portable spelling on every host.
pub(crate) fn read_repo_file(root: &Path, path: &str) -> Result<String, String> {
    if path.contains(['\\', ':'])
        || path.split('/').any(|part| matches!(part, "" | "." | ".."))
        || !Path::new(path)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
    {
        return Err(format!("invalid repository-relative path {path:?}"));
    }
    let full = root
        .join(path)
        .canonicalize()
        .map_err(|e| format!("{path}: {e}"))?;
    if !full.starts_with(root) || !full.is_file() {
        return Err(format!(
            "{path}: expected a regular file inside the repository"
        ));
    }
    fs::read_to_string(&full).map_err(|e| format!("{path}: {e}"))
}

fn read_json<T: DeserializeOwned>(root: &Path, path: &str) -> Result<T, String> {
    serde_json::from_str(&read_repo_file(root, path)?).map_err(|e| format!("{path}: {e}"))
}

fn require_version(path: &str, version: u32) -> Result<(), String> {
    if version != 1 {
        return Err(format!(
            "{path}: unsupported schema_version {version}, expected 1"
        ));
    }
    Ok(())
}

impl Tensor {
    pub fn load(root: &Path) -> Result<Self, String> {
        let root = root
            .canonicalize()
            .map_err(|e| format!("{}: {e}", root.display()))?;
        let schema: serde_json::Value = read_json(&root, "project/status/schema.json")?;
        // Pin the published schema independently of the file being checked, so
        // rebuilding the CLI from a checkout with a widened schema cannot hide drift.
        // serde_json's sorted object keys make whitespace and key reordering irrelevant.
        let fingerprint = serde_json::to_vec(&schema)
            .map_err(|e| e.to_string())?
            .into_iter()
            .fold(0xcbf29ce484222325_u64, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
            });
        if fingerprint != 0x6de3040c1d234e2 {
            return Err(
                "project/status/schema.json: schema differs from the tool's v1 contract".into(),
            );
        }
        let catalog: Catalog = read_json(&root, "project/status/catalog.json")?;
        require_version("catalog", catalog.schema_version)?;
        let milestones: Milestones = read_json(&root, "project/status/milestones.json")?;
        require_version("milestones", milestones.schema_version)?;
        let mut expected_shards = BTreeSet::new();
        let mut cells = Vec::new();
        for arch in &catalog.architectures {
            if !identifier(&arch.id) {
                return Err(format!(
                    "catalog: invalid architecture identifier {:?}",
                    arch.id
                ));
            }
            let filename = format!("{}.json", arch.id);
            if !expected_shards.insert(filename.clone()) {
                return Err(format!("catalog: duplicate architecture {}", arch.id));
            }
            let path = format!("project/status/cells/{filename}");
            let shard: Shard = read_json(&root, &path)?;
            require_version(&path, shard.schema_version)?;
            for cell in shard.cells {
                if cell.architecture != arch.id {
                    return Err(format!(
                        "{path}: cell {} belongs to architecture {}",
                        cell.id, cell.architecture
                    ));
                }
                cells.push(cell);
            }
        }
        let directory = root.join("project/status/cells");
        for entry in
            fs::read_dir(&directory).map_err(|e| format!("{}: {e}", directory.display()))?
        {
            let name = entry.map_err(|e| e.to_string())?.file_name();
            if Path::new(&name).extension().is_some_and(|e| e == "json")
                && !name.to_str().is_some_and(|n| expected_shards.contains(n))
            {
                return Err(format!(
                    "unregistered cell shard: {}",
                    name.to_string_lossy()
                ));
            }
        }
        Ok(Self {
            catalog,
            milestones,
            cells,
        })
    }
}
