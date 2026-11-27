# Selecting development work

The ordered goals and objective are maintained in
[the mainline manifest](../project/status/milestones.json). View the current
task from the repository root after building the [status tool](../STATUS.md).

```sh
target/lean/leselang-dev tensor next
target/lean/leselang-dev tensor next --json
target/lean/leselang-dev tensor blocked
target/lean/leselang-dev tensor inspect hosts/conformance/shared-programs
```

For each open goal in order, the planner considers that goal and its transitive
prerequisites. Candidates need an eligible priority and lifecycle, no explicit
blockers, all prerequisites stable at 100, valid evidence, a validation command,
a concrete artifact and acceptance criteria. Within one goal the planner ranks
ready candidates by priority, then weakest maturity, lowest progress and
coordinate. If an earlier goal has no eligible work, it can select work on a
later goal. Filtering narrows candidate selection and still checks dependencies
in the whole catalog.

A ready card includes its goal, selection reason, implementation owner and
scope, current evidence, blocking dependencies, next action, expected artifact
and acceptance criteria. A blocked valid mainline returns `no-ready-task`
with its unfinished goals. A fully closed mainline returns `complete`.
The `scope` field shows the closed and total goal counts alongside the closed
and total contract counts for the entire catalog. These counts keep their
original denominators when a filter narrows the set of candidate tasks.
Metadata drift invalidates the query and produces a nonzero exit code before
a card is published.

To admit a new goal, define the exact gap in its cell, choose a delivery
priority, connect hard prerequisites and add the coordinate in the intended
goal order. Record the actual acceptance result before closing the cell.
Closing a goal advances selection from the same manifest.
