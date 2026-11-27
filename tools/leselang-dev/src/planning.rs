use crate::model::{Cell, Tensor};
use crate::tree::{maturity, weight};
use serde_json::{Value, json};
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

pub struct Plan<'a> {
    pub tensor: &'a Tensor,
    pub cells: BTreeMap<&'a str, &'a Cell>,
}

impl<'a> Plan<'a> {
    pub fn new(tensor: &'a Tensor) -> Self {
        Self {
            cells: tensor.cells.iter().map(|c| (c.id.as_str(), c)).collect(),
            tensor,
        }
    }

    // Called only on a validated catalog. The visited set also deduplicates diamonds.
    fn chain(&self, cell: &'a Cell) -> BTreeSet<&'a str> {
        let mut queue: Vec<_> = cell.dependencies.iter().map(String::as_str).collect();
        let mut seen = BTreeSet::new();
        while let Some(id) = queue.pop() {
            if seen.insert(id) {
                queue.extend(self.cells[id].dependencies.iter().map(String::as_str));
            }
        }
        seen
    }

    pub fn unmet(&self, cell: &'a Cell) -> Vec<&'a str> {
        self.chain(cell)
            .into_iter()
            .filter(|id| !self.cells[id].closed())
            .collect()
    }

    pub fn blocking_details(&self, cell: &'a Cell) -> Value {
        json!({
            "coordinate": cell.id,
            "blockers": cell.blockers,
            "blocking_dependencies": self.unmet(cell)
        })
    }

    pub fn owner(&self, cell: &Cell) -> &str {
        self.tensor
            .catalog
            .architectures
            .iter()
            .find(|a| a.id == cell.architecture)
            .and_then(|a| a.modules.iter().find(|m| m.id == cell.module))
            .map(|m| m.owner.as_str())
            .expect("validated module")
    }

    pub fn inspect(&self, cell: &'a Cell) -> Value {
        let levels: BTreeSet<_> = cell.evidence.iter().filter_map(|e| e.level()).collect();
        let dependents: Vec<_> = self
            .cells
            .values()
            .filter(|c| c.dependencies.contains(&cell.id))
            .map(|c| &c.id)
            .collect();
        json!({
            "cell": cell,
            "owner": self.owner(cell),
            "dependency_closure": self.chain(cell),
            "blocking_dependencies": self.unmet(cell),
            "dependents": dependents,
            "evidence_levels": levels
        })
    }

    fn open_for_work(&self, cell: &Cell) -> bool {
        !cell.closed() && cell.lifecycle != "retired" && weight(&cell.priority) > 0
    }

    fn candidates(&self, goal: &'a Cell) -> Vec<&'a Cell> {
        let mut candidates = self.chain(goal);
        candidates.insert(&goal.id);
        let mut candidates: Vec<_> = candidates
            .into_iter()
            .map(|id| self.cells[id])
            .filter(|c| self.open_for_work(c) && c.blockers.is_empty() && self.unmet(c).is_empty())
            .collect();
        candidates.sort_by_key(|c| {
            (
                Reverse(weight(&c.priority)),
                maturity(&c.status),
                c.progress,
                &c.id,
            )
        });
        candidates
    }

    pub fn selected(&self, matches: impl Fn(&Cell) -> bool) -> Option<(&'a Cell, &'a Cell)> {
        for id in &self.tensor.milestones.goals {
            let goal = self.cells[id.as_str()];
            if goal.closed() {
                continue;
            }
            if let Some(candidate) = self.candidates(goal).into_iter().find(|c| matches(c)) {
                return Some((goal, candidate));
            }
        }
        None
    }

    pub fn next(&self, matches: impl Fn(&Cell) -> bool) -> Value {
        // Filters narrow the candidate task. Both populations below refer to the full catalog.
        let goals = &self.tensor.milestones.goals;
        let closed_goals = goals
            .iter()
            .filter(|id| self.cells[id.as_str()].closed())
            .count();
        let closed_contracts = self.cells.values().filter(|cell| cell.closed()).count();
        let scope = json!({
            "state_applies_to": "mainline",
            "mainline_goals": {"closed": closed_goals, "total": goals.len()},
            "registered_contracts": {"closed": closed_contracts, "total": self.cells.len()}
        });
        let Some((goal, cell)) = self.selected(matches) else {
            let unfinished: Vec<_> = self
                .tensor
                .milestones
                .goals
                .iter()
                .map(|id| self.cells[id.as_str()])
                .filter(|c| !c.closed())
                .map(|c| self.blocking_details(c))
                .collect();
            let state = if unfinished.is_empty() {
                "complete"
            } else {
                "no-ready-task"
            };
            return json!({
                "state": state,
                "scope": scope,
                "mainline": self.tensor.milestones,
                "unfinished_goals": unfinished,
                "task": null
            });
        };
        let reason = if goal.id == cell.id {
            format!(
                "Ordered goal {} has satisfied dependencies and priority {}.",
                goal.id, cell.priority
            )
        } else {
            format!(
                "Open prerequisite of ordered goal {} with priority {} and satisfied dependencies.",
                goal.id, cell.priority
            )
        };
        json!({
            "state": "ready",
            "scope": scope,
            "mainline": self.tensor.milestones,
            "goal": goal.id,
            "task": {
                "coordinate": cell.id,
                "selection_reason": reason,
                "objective": cell.contract,
                "current_evidence": cell.evidence,
                "blocking_dependencies": self.unmet(cell),
                "implementation_scope": {"owner": self.owner(cell), "contract": cell.contract},
                "next_action": cell.next_action,
                "validation_command": cell.validation_command,
                "expected_artifact": cell.expected_artifact,
                "acceptance_criteria": cell.acceptance_criteria
            }
        })
    }

    pub fn check_selection(&self) -> Result<(), String> {
        if let Some((goal, cell)) = self.selected(|_| true)
            && ((!self.chain(goal).contains(cell.id.as_str()) && goal.id != cell.id)
                || cell.closed()
                || !cell.blockers.is_empty()
                || !self.unmet(cell).is_empty()
                || cell.acceptance_criteria.is_empty())
        {
            return Err(format!("inconsistent task card for {}", cell.id));
        }
        Ok(())
    }
}
