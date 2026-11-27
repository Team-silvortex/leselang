mod model;
mod planning;
mod tree;
mod validate;

use model::{Cell, Tensor};
use planning::Plan;
use serde_json::{Value, json};
use std::{collections::BTreeMap, env, path::PathBuf, process::ExitCode};

const USAGE: &str =
    "leselang-dev tensor <summary|validate|tree|list|weakest|blocked|inspect COORDINATE|next>
  [--architecture ID] [--module ID] [--function ID] [--status STATUS] [--priority PRIORITY]
  [--json] [--root REPOSITORY]
Queries always validate the whole catalog. Filters narrow the presented cells.";

#[derive(Default)]
struct Options {
    command: String,
    coordinate: Option<String>,
    filters: BTreeMap<String, String>,
    root: Option<PathBuf>,
}

impl Options {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut it = args.iter().map(String::as_str);
        if it.next() != Some("tensor") {
            return Err(USAGE.into());
        }
        let command = it.next().ok_or(USAGE)?;
        if !matches!(
            command,
            "validate" | "list" | "summary" | "tree" | "weakest" | "blocked" | "inspect" | "next"
        ) {
            return Err(format!("unknown tensor command {command}"));
        }
        let mut out = Self {
            command: command.into(),
            ..Self::default()
        };
        let mut json = false;
        while let Some(arg) = it.next() {
            match arg {
                "--json" if !json => json = true,
                "--json" => return Err("repeated option --json".into()),
                "--architecture" | "--module" | "--function" | "--status" | "--priority"
                | "--root" => {
                    let value = it
                        .next()
                        .filter(|v| !v.starts_with("--") && !v.is_empty())
                        .ok_or_else(|| format!("{arg} needs a value"))?;
                    if (arg == "--status" && tree::maturity(value) == 0)
                        || (arg == "--priority" && tree::weight(value) == 0 && value != "deferred")
                    {
                        return Err(format!("unknown value {value:?} for {arg}"));
                    }
                    let duplicate = if arg == "--root" {
                        out.root.replace(PathBuf::from(value)).is_some()
                    } else {
                        out.filters
                            .insert(arg.trim_start_matches('-').into(), value.into())
                            .is_some()
                    };
                    if duplicate {
                        return Err(format!("repeated option {arg}"));
                    }
                }
                id if !id.starts_with('-') && command == "inspect" && out.coordinate.is_none() => {
                    out.coordinate = Some(id.into())
                }
                other => return Err(format!("unexpected argument {other}")),
            }
        }
        if command == "inspect" && out.coordinate.is_none() {
            return Err("inspect needs a coordinate".into());
        }
        Ok(out)
    }

    fn matches(&self, c: &Cell) -> bool {
        self.filters.iter().all(|(key, value)| match key.as_str() {
            "architecture" => value == &c.architecture,
            "module" => value == &c.module,
            "function" => value == &c.function,
            "status" => value == &c.status,
            "priority" => value == &c.priority,
            _ => false,
        })
    }

    fn repository(&self) -> Result<PathBuf, String> {
        if let Some(root) = &self.root {
            return Ok(root.clone());
        }
        let cwd = env::current_dir().map_err(|e| e.to_string())?;
        cwd.ancestors()
            .find(|path| path.join("project/status/catalog.json").is_file())
            .map(|p| p.to_owned())
            .ok_or_else(|| "cannot find project/status/catalog.json; pass --root REPOSITORY".into())
    }
}

fn query(options: &Options) -> Result<Value, Vec<String>> {
    let root = options.repository().map_err(|e| vec![e])?;
    let tensor = Tensor::load(&root).map_err(|e| vec![e])?;
    let errors = validate::validate(&root, &tensor);
    if !errors.is_empty() {
        return Err(errors);
    }
    let plan = Plan::new(&tensor);
    let all: Vec<_> = plan.cells.values().copied().collect();
    let full_tree = tree::hierarchy(&all);
    tree::check_tree(&full_tree, &tensor.cells).map_err(|e| vec![e])?;
    plan.check_selection().map_err(|e| vec![e])?;
    let mut selected: Vec<_> = all.into_iter().filter(|c| options.matches(c)).collect();
    let view = match options.command.as_str() {
        "validate" => {
            json!({"valid":true,"validated_cells":tensor.cells.len(),"matching_cells":selected.len()})
        }
        "tree" => json!(tree::hierarchy(&selected)),
        "summary" => {
            let tree = tree::hierarchy(&selected);
            let modules = tree.children.iter().flat_map(|domain| &domain.children);
            let represented = modules.clone().count();
            let all_matching_contracts_closed = modules
                .filter(|module| module.aggregate.cells == module.aggregate.closed_cells)
                .count();
            let domains: Vec<_> = tree
                .children
                .iter()
                .map(|n| json!({"architecture":n.coordinate,"aggregate":n.aggregate}))
                .collect();
            let mut statuses = BTreeMap::new();
            let mut evidence_counts: BTreeMap<_, usize> = model::EVIDENCE_KINDS
                .iter()
                .map(|&(kind, _)| (kind, 0))
                .collect();
            for cell in &selected {
                *statuses.entry(&cell.status).or_insert(0_usize) += 1;
                for evidence in &cell.evidence {
                    *evidence_counts
                        .get_mut(evidence.kind.as_str())
                        .expect("validated evidence kind") += 1;
                }
            }
            json!({
                "aggregate": tree.aggregate, "architectures": domains, "statuses": statuses,
                "modules": {
                    "represented": represented,
                    "all_matching_contracts_closed": all_matching_contracts_closed
                },
                "evidence_counts": evidence_counts, "mainline": tensor.milestones
            })
        }
        "list" => json!(selected),
        "weakest" => {
            selected.sort_by_key(|c| (tree::maturity(&c.status), c.progress, &c.id));
            json!(selected)
        }
        "blocked" => {
            let rows: Vec<_> = selected
                .iter()
                .filter(|c| !c.blockers.is_empty() || !plan.unmet(c).is_empty())
                .map(|c| plan.blocking_details(c))
                .collect();
            json!(rows)
        }
        "inspect" => {
            let id = options.coordinate.as_deref().unwrap();
            let cell = plan
                .cells
                .get(id)
                .filter(|c| options.matches(c))
                .ok_or_else(|| vec![format!("no matching coordinate {id}")])?;
            plan.inspect(cell)
        }
        "next" => plan.next(|c| options.matches(c)),
        _ => unreachable!(),
    };
    Ok(view)
}

fn details(value: &Value, indent: usize) {
    let pad = " ".repeat(indent);
    match value {
        Value::Object(map) => {
            for (name, item) in map {
                if item.is_object() || item.is_array() {
                    println!("{pad}{name}");
                    details(item, indent + 2);
                } else {
                    println!(
                        "{pad}{name}  {}",
                        item.as_str()
                            .map(str::to_owned)
                            .unwrap_or_else(|| item.to_string())
                    );
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                details(item, indent);
            }
        }
        Value::String(s) => println!("{pad}{s}"),
        other => println!("{pad}{other}"),
    }
}

fn print_tree(node: &Value, depth: usize) {
    let a = &node["aggregate"];
    println!(
        "{}{}  {}  matching-contracts-closed={}/{}  mean={}  delivery={}  delivery-population={}/{}",
        "  ".repeat(depth),
        node["coordinate"].as_str().unwrap(),
        a["status"].as_str().unwrap_or("empty"),
        a["closed_cells"],
        a["cells"],
        a["mean_progress"],
        a["delivery_progress"],
        a["delivery_cells"],
        a["cells"]
    );
    if let Some(children) = node["children"].as_array() {
        for child in children {
            print_tree(child, depth + 1);
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<_> = env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let json_output = args.iter().any(|a| a == "--json");
    let options = match Options::parse(&args) {
        Ok(o) => o,
        Err(error) => {
            return failure(json_output, vec![error], 2);
        }
    };
    let result = match query(&options) {
        Ok(v) => v,
        Err(errors) => return failure(json_output, errors, 1),
    };
    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(
                &json!({"schema_version":1,"command":options.command,"result":result})
            )
            .unwrap()
        );
    } else {
        match options.command.as_str() {
            "list" | "weakest" => {
                for c in result.as_array().unwrap() {
                    println!(
                        "{}\t{}\t{}\t{}",
                        c["id"].as_str().unwrap(),
                        c["status"].as_str().unwrap(),
                        c["progress"],
                        c["priority"].as_str().unwrap()
                    );
                }
            }
            "tree" => print_tree(&result, 0),
            _ => details(&result, 0),
        }
    }
    ExitCode::SUCCESS
}

fn failure(as_json: bool, errors: Vec<String>, code: u8) -> ExitCode {
    if as_json {
        println!(
            "{}",
            json!({"schema_version":1,"valid":false,"errors":errors})
        );
    } else {
        for e in errors {
            eprintln!("{e}");
        }
    }
    ExitCode::from(code)
}
