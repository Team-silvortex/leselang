use crate::model::Cell;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

pub fn maturity(status: &str) -> u8 {
    match status {
        "stable" => 4,
        "usable" => 3,
        "active" => 2,
        "early" => 1,
        _ => 0,
    }
}

pub fn weight(priority: &str) -> u32 {
    match priority {
        "critical" => 4,
        "active" => 2,
        "maintenance" => 1,
        _ => 0,
    }
}

#[derive(Debug, Default, PartialEq, Eq, Serialize)]
pub struct Aggregate {
    pub cells: usize,
    pub closed_cells: usize,
    pub status: Option<String>,
    pub mean_progress: Option<u64>,
    pub delivery_cells: usize,
    pub delivery_progress: Option<u64>,
}

impl Aggregate {
    fn from_cells(cells: &[&Cell]) -> Self {
        let mut units = 0;
        let mut weighted = 0;
        let mut delivery_cells = 0;
        for cell in cells {
            let w = u64::from(weight(&cell.priority));
            if cell.lifecycle != "retired" && w > 0 {
                units += w;
                weighted += w * u64::from(cell.progress);
                delivery_cells += 1;
            }
        }
        let sum: u64 = cells.iter().map(|c| u64::from(c.progress)).sum();
        Self {
            cells: cells.len(),
            closed_cells: cells.iter().filter(|cell| cell.closed()).count(),
            status: cells
                .iter()
                .min_by_key(|c| maturity(&c.status))
                .map(|c| c.status.clone()),
            mean_progress: sum.checked_div(cells.len() as u64),
            delivery_cells,
            delivery_progress: weighted.checked_div(units),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Node {
    pub coordinate: String,
    pub aggregate: Aggregate,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Node>,
}

pub fn hierarchy(cells: &[&Cell]) -> Node {
    fn group(cells: &[&Cell], path: String, depth: usize) -> Node {
        let mut groups = BTreeMap::new();
        for &cell in cells {
            let key = match depth {
                0 => &cell.architecture,
                1 => &cell.module,
                2 => &cell.function,
                _ => break,
            };
            groups.entry(key).or_insert_with(Vec::new).push(cell);
        }
        let children = groups
            .into_iter()
            .map(|(key, members)| {
                let child = if depth == 0 {
                    key.clone()
                } else {
                    format!("{path}/{key}")
                };
                group(&members, child, depth + 1)
            })
            .collect();
        Node {
            coordinate: path,
            aggregate: Aggregate::from_cells(cells),
            children,
        }
    }
    group(cells, "leselang".into(), 0)
}

// Walk the rendered tree independently, rebuild each node from its actual leaves,
// and compare the reachable coordinate set with the source catalog.
pub fn check_tree(root: &Node, cells: &[Cell]) -> Result<(), String> {
    fn visit<'a>(
        node: &Node,
        cells: &BTreeMap<&str, &'a Cell>,
        seen: &mut BTreeSet<String>,
        depth: u8,
    ) -> Result<Vec<&'a Cell>, String> {
        let mut leaves = vec![];
        if !seen.insert(node.coordinate.clone()) {
            return Err(format!("duplicate tree node {}", node.coordinate));
        }
        if depth == 3 {
            let cell = cells
                .get(node.coordinate.as_str())
                .ok_or_else(|| format!("unregistered tree leaf {}", node.coordinate))?;
            if !node.children.is_empty() {
                return Err(format!("unexpected children of {}", node.coordinate));
            }
            leaves.push(*cell);
        } else {
            for child in &node.children {
                let parent = child.coordinate.rsplit_once('/').map(|(p, _)| p);
                if (depth == 0 && parent.is_some())
                    || (depth > 0 && parent != Some(node.coordinate.as_str()))
                {
                    return Err(format!("invalid tree parent for {}", child.coordinate));
                }
                leaves.extend(visit(child, cells, seen, depth + 1)?);
            }
        }
        if node.aggregate != Aggregate::from_cells(&leaves) {
            return Err(format!("inconsistent aggregate at {}", node.coordinate));
        }
        Ok(leaves)
    }
    let by_id = cells.iter().map(|c| (c.id.as_str(), c)).collect();
    let leaves = visit(root, &by_id, &mut BTreeSet::new(), 0)?;
    if leaves.len() != cells.len() {
        return Err("tree does not cover catalog".into());
    }
    Ok(())
}
