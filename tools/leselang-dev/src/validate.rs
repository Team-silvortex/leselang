use crate::model::{Cell, Evidence, Tensor, identifier, read_repo_file};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

fn nonempty(value: &str) -> bool {
    !value.trim().is_empty()
}

fn date(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return false;
    }
    if !(0..10).all(|i| i == 4 || i == 7 || bytes[i].is_ascii_digit()) {
        return false;
    }
    let year: u32 = value[0..4].parse().unwrap();
    let month: u32 = value[5..7].parse().unwrap();
    let day: u32 = value[8..10].parse().unwrap();
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let limit = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        _ => 0,
    };
    year > 0 && (1..=limit).contains(&day)
}

pub const DOMAINS: [&str; 8] = [
    "project",
    "frontend",
    "semantics",
    "runtime",
    "embedding",
    "hosts",
    "agent",
    "future",
];

fn unique_strings(values: &[String], name: &str, empty_allowed: bool, issues: &mut Vec<String>) {
    if !empty_allowed && values.is_empty() {
        issues.push(format!("{name}: expected at least one entry"));
    }
    let mut seen = BTreeSet::new();
    for value in values {
        if !nonempty(value) || !seen.insert(value) {
            issues.push(format!("{name}: blank or duplicate entry {value:?}"));
        }
    }
}

fn catalog_coordinates(t: &Tensor, issues: &mut Vec<String>) -> BTreeSet<String> {
    let mut declared = BTreeSet::new();
    let mut architectures = BTreeSet::new();
    for arch in &t.catalog.architectures {
        if !identifier(&arch.id) || !architectures.insert(arch.id.as_str()) {
            issues.push(format!("invalid or duplicate architecture {}", arch.id));
        }
        if !nonempty(&arch.label) || arch.modules.is_empty() {
            issues.push(format!(
                "{}: architecture requires a label and at least one module",
                arch.id
            ));
        }
        let mut modules = BTreeSet::new();
        for module in &arch.modules {
            let path = format!("{}/{}", arch.id, module.id);
            if !identifier(&module.id) || !modules.insert(&module.id) {
                issues.push(format!("{path}: invalid or duplicate module"));
            }
            if !nonempty(&module.owner) || module.functions.is_empty() {
                issues.push(format!(
                    "{path}: module requires an owner and at least one function"
                ));
            }
            for function in &module.functions {
                let coordinate = format!("{path}/{function}");
                if !identifier(function) || !declared.insert(coordinate.clone()) {
                    issues.push(format!("{coordinate}: invalid or duplicate function"));
                }
            }
        }
    }
    for required in DOMAINS {
        if !architectures.contains(required) {
            issues.push(format!("missing required architecture {required}"));
        }
    }
    declared
}

fn check_cell(cell: &Cell, issues: &mut Vec<String>) {
    let id = &cell.id;
    let coordinate = format!("{}/{}/{}", cell.architecture, cell.module, cell.function);
    if *id != coordinate {
        issues.push(format!("{id}: expected coordinate {coordinate}"));
    }
    for (name, value, choices) in [
        (
            "status",
            &cell.status,
            &["early", "active", "usable", "stable"][..],
        ),
        (
            "priority",
            &cell.priority,
            &["critical", "active", "maintenance", "deferred"],
        ),
        ("confidence", &cell.confidence, &["low", "medium", "high"]),
        (
            "lifecycle",
            &cell.lifecycle,
            &["current", "bridge", "target", "retired"],
        ),
    ] {
        if !choices.contains(&value.as_str()) {
            issues.push(format!("{id}: invalid {name} {value:?}"));
        }
    }
    if cell.progress > 100
        || (cell.progress == 100) != (cell.status == "stable")
        || (cell.progress == 0 && cell.status != "early")
    {
        issues.push(format!(
            "{id}: inconsistent status {} and progress {}",
            cell.status, cell.progress
        ));
    }
    if cell.status == "stable"
        && (!matches!(cell.lifecycle.as_str(), "current" | "bridge") || !cell.blockers.is_empty())
    {
        issues.push(format!(
            "{id}: stable requires current/bridge lifecycle and zero blockers"
        ));
    }
    for (name, value) in [
        ("contract", &cell.contract),
        ("next_action", &cell.next_action),
        ("validation_command", &cell.validation_command),
        ("expected_artifact", &cell.expected_artifact),
    ] {
        if !nonempty(value) {
            issues.push(format!("{id}: empty {name}"));
        }
    }
    unique_strings(
        &cell.dependencies,
        &format!("{id}/dependencies"),
        true,
        issues,
    );
    unique_strings(&cell.blockers, &format!("{id}/blockers"), true, issues);
    unique_strings(
        &cell.acceptance_criteria,
        &format!("{id}/acceptance_criteria"),
        false,
        issues,
    );
    if cell.evidence.is_empty() {
        issues.push(format!("{id}: no evidence for this capability"));
    }
    if cell.status == "usable"
        && !cell
            .evidence
            .iter()
            .any(|ev| ev.kind != "documentation-contract")
    {
        issues.push(format!(
            "{id}: usable requires implementation or proof evidence"
        ));
    }
    if cell.status == "stable"
        && !cell.evidence.iter().any(|ev| {
            ev.level().is_some()
                && !matches!(
                    ev.kind.as_str(),
                    "documentation-contract" | "source" | "benchmark"
                )
        })
    {
        issues.push(format!(
            "{id}: stable requires a test, compatibility fixture or acceptance receipt"
        ));
    }
}

type FileCache = BTreeMap<String, Result<String, String>>;

// Distinguish inspectable proof shelves from narrative claims even when an
// exact anchor happens to occur in both. A kind never implies a test was run.
fn admissible_path(kind: &str, path: &str) -> bool {
    let extension = Path::new(path).extension().and_then(|e| e.to_str());
    let executable = matches!(
        extension,
        Some(
            "rs" | "js"
                | "mjs"
                | "cjs"
                | "ts"
                | "cs"
                | "sh"
                | "py"
                | "swift"
                | "m"
                | "mm"
                | "c"
                | "cpp"
                | "lese"
        )
    );
    let component = |name| path.split('/').any(|part| part == name);
    match kind {
        "source" => {
            executable
                || matches!(
                    extension,
                    Some(
                        "toml" | "json" | "yaml" | "yml" | "csproj" | "lock" | "h" | "html" | "css"
                    )
                )
        }
        "unit-test" | "integration-test" => executable,
        "consumer-test" => executable && component("consumer"),
        "compatibility-fixture" => component("fixtures") || path.contains("-fixture."),
        "benchmark" => executable || component("benchmarks"),
        "native-acceptance" | "ci-receipt" => {
            path.starts_with("docs/acceptance/") || path.starts_with("project/status/evidence/")
        }
        "documentation-contract" => matches!(extension, Some("md" | "txt" | "json" | "toml")),
        _ => false,
    }
}

fn check_evidence(root: &Path, cell: &Cell, cache: &mut FileCache, issues: &mut Vec<String>) {
    let mut seen = BTreeSet::new();
    for item in &cell.evidence {
        let Evidence {
            kind,
            path,
            anchor,
            date: recorded,
            version,
            platform,
        } = item;
        let key = format!("{}: evidence {kind} {path}", cell.id);
        if !seen.insert((kind, path, anchor)) {
            issues.push(format!("{key}: duplicate evidence identity"));
        }
        if item.level().is_none() {
            issues.push(format!("{key}: unknown evidence kind"));
        } else if !admissible_path(kind, path) {
            issues.push(format!(
                "{key}: path does not identify a {kind} proof source"
            ));
        }
        let historical = matches!(kind.as_str(), "native-acceptance" | "ci-receipt");
        if historical && (recorded.is_none() || version.is_none() || platform.is_none()) {
            issues.push(format!(
                "{key}: historical receipt requires date, version, and platform"
            ));
        }
        if recorded.as_ref().is_some_and(|d| !date(d)) {
            issues.push(format!("{key}: invalid evidence date"));
        }
        if [version, platform]
            .into_iter()
            .flatten()
            .any(|s| !nonempty(s))
        {
            issues.push(format!("{key}: blank evidence version or platform"));
        }
        if !nonempty(anchor) {
            issues.push(format!("{key}: empty anchor"));
        }
        match cache
            .entry(path.clone())
            .or_insert_with(|| read_repo_file(root, path))
        {
            Ok(contents) => {
                if nonempty(anchor) {
                    match contents.match_indices(anchor.as_str()).take(2).count() {
                        0 => issues.push(format!("{key}: anchor missing: {anchor:?}")),
                        1 => {}
                        _ => issues.push(format!("{key}: ambiguous anchor: {anchor:?}")),
                    }
                }
                if historical && recorded.as_ref().is_some_and(|day| !contents.contains(day)) {
                    issues.push(format!("{key}: receipt date missing from cited file"));
                }
            }
            Err(error) => issues.push(format!("{key}: {error}")),
        }
    }
}

fn check_dependencies(by_id: &BTreeMap<&str, &Cell>, goals: &[String], issues: &mut Vec<String>) {
    // Paths in the current DFS stack identify an actual cycle (including self-loops).
    // The explicit stack also works for long chains in programmatically edited catalogs.
    let mut done = BTreeSet::new();
    let referenced: BTreeSet<_> = by_id
        .values()
        .flat_map(|cell| cell.dependencies.iter().map(String::as_str))
        .collect();
    for (&id, &cell) in by_id {
        let isolated = cell.dependencies.is_empty() && !referenced.contains(id);
        match &cell.independent_reason {
            Some(reason) if !nonempty(reason) || !isolated => {
                issues.push(format!(
                    "{id}: independent_reason must be nonblank and only used for an isolated cell"
                ));
            }
            None if isolated && !goals.contains(&cell.id) => {
                issues.push(format!(
                    "{id}: isolated cell has no independent_reason or mainline goal"
                ));
            }
            _ => {}
        }
        for dep in &cell.dependencies {
            match by_id.get(dep.as_str()) {
                None => issues.push(format!("{id}: unknown dependency {dep}")),
                Some(other) if cell.closed() && !other.closed() => {
                    issues.push(format!("{id}: stable cell has an open prerequisite {dep}"));
                }
                _ => {}
            }
        }
        if done.contains(id) {
            continue;
        }
        let mut active = BTreeSet::from([id]);
        let mut stack = vec![(id, cell.dependencies.iter())];
        while let Some((node, deps)) = stack.last_mut() {
            match deps.next().map(String::as_str) {
                None => {
                    done.insert(*node);
                    active.remove(*node);
                    stack.pop();
                }
                Some(dep) if active.contains(dep) => {
                    issues.push(format!("dependency cycle: {node} -> {dep}"));
                }
                Some(dep) if !done.contains(dep) => {
                    if let Some(target) = by_id.get(dep) {
                        active.insert(dep);
                        stack.push((dep, target.dependencies.iter()));
                    }
                }
                _ => {}
            }
        }
    }
}

pub fn validate(root: &Path, t: &Tensor) -> Vec<String> {
    let root = match root.canonicalize() {
        Ok(root) => root,
        Err(error) => return vec![format!("{}: {error}", root.display())],
    };
    let mut issues = Vec::new();
    if t.catalog.schema_version != 1
        || t.milestones.schema_version != 1
        || t.catalog.status_protocol != "dev-tensor-status-v1"
    {
        issues.push("unsupported schema version or status protocol".into());
    }
    let declared = catalog_coordinates(t, &mut issues);
    let mut cache = FileCache::new();
    let mut by_id = BTreeMap::new();
    for cell in &t.cells {
        if cell.architecture == "future"
            && cell.priority == "deferred"
            && cell.lifecycle != "target"
        {
            issues.push(format!(
                "{}: deferred future cell must have target lifecycle",
                cell.id
            ));
        }
        if by_id.insert(cell.id.as_str(), cell).is_some() {
            issues.push(format!("duplicate cell {}", cell.id));
        }
        if !declared.contains(&cell.id) {
            issues.push(format!(
                "{}: orphan cell absent from catalog dimensions",
                cell.id
            ));
        }
        check_cell(cell, &mut issues);
        check_evidence(&root, cell, &mut cache, &mut issues);
    }
    for coordinate in &declared {
        if !by_id.contains_key(coordinate.as_str()) {
            issues.push(format!("missing cell for declared coordinate {coordinate}"));
        }
    }
    check_dependencies(&by_id, &t.milestones.goals, &mut issues);
    if !identifier(&t.milestones.id) || !nonempty(&t.milestones.objective) {
        issues.push("milestones: invalid id or missing mainline objective".into());
    }
    unique_strings(&t.milestones.goals, "milestones/goals", false, &mut issues);
    for goal in &t.milestones.goals {
        match by_id.get(goal.as_str()) {
            None => issues.push(format!("unknown mainline goal {goal}")),
            Some(c) if c.priority == "deferred" || c.lifecycle == "retired" => {
                issues.push(format!(
                    "ineligible mainline goal {goal}: deferred or retired"
                ));
            }
            _ => {}
        }
    }
    for cell in &t.cells {
        if cell.architecture == "future"
            && cell.priority != "deferred"
            && !t.milestones.goals.contains(&cell.id)
        {
            issues.push(format!(
                "{}: future delivery requires explicit mainline promotion",
                cell.id
            ));
        }
    }
    issues
}

#[cfg(test)]
#[path = "validation_tests.rs"]
mod tests;
