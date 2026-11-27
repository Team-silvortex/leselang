# Development tensor protocol

LeseLang records a capability at one coordinate, `architecture/module/function`.
The [coverage inventory](../project/status/catalog.json) declares architectures,
module owners and supported functions. Each architecture has one v1 cell shard
under `project/status/cells/`. A declaration and a cell must correspond in both
directions. Adding a module never implicitly creates all possible functions.
The [schema](../project/status/schema.json) and validator define serialized
records. The [mainline](development-mainline.md) defines the order in which
goals can consume development effort.

## Status and scope

Version 1 adopts NuisLang's `dev-tensor-status-v1` vocabulary and coordinate
order. `early` covers an intention or first exploration, `active` a slice
under construction, `usable` working but incompletely accepted scope, and
`stable` a closed, independently bounded contract. Progress is a reviewed
triage estimate from 0 to 100 for that exact contract. Stable requires 100,
evidence and satisfied prerequisites. Zero belongs to early. Changing a
contract's extent can reopen it. The value does not predict a release date.

The lifecycle, priority and confidence axes follow Gewyvern's Project Status
Tensor. Lifecycle describes the architectural role. `current` is authoritative
today, `bridge` serves compatibility, `target` represents intended scope and
`retired` preserves history. Priority expresses roadmap attention with
`critical`, `active`, `maintenance` and `deferred`. Confidence
(`low`, `medium`, `high`) describes confidence in the assessment, including
confidence that a capability is still a proposal. LeseLang's catalog version
is independent of both the language release and the two reference tools.
Changes to required fields, enums or interpretation require explicit schema
and reader migration. The reader rejects unknown versions and fields.

Module owners are repository responsibility boundaries. Each cell declares
its contract, hard prerequisites, external blockers, next action, validation
command, expected artifact and observable acceptance criteria. Dependencies
point to other coordinates. A blocker describes a concrete obstacle. A missing
deliverable that a developer can implement directly belongs in the next action.
A root capability can have zero dependencies. A cell with no edges in either
direction must be an explicit goal or declare why it is independently
maintained in `independent_reason`. Remove that reason when a dependency edge
is added. Every cell must have an owner, an inventory declaration and an
evidence identity.

## Evidence

The evidence key comprises the kind, a repository-relative file path and a
unique literal text anchor in that file. Prefer a declaration, test name, section
heading or fixture identity that would move with the capability. An acceptance
receipt also records the original date, version or revision identity, and platform
conditions. If a historical report does not state an application version, retain
that limitation in its description. Keep the original receipt when adding a
new platform or revision.

| Evidence kind | What the catalog can establish |
| --- | --- |
| `documentation-contract` | A specification, target boundary or acceptance procedure exists. |
| `source` | A specific implementation or current boundary exists. |
| `unit-test`, `integration-test`, `compatibility-fixture`, `benchmark` | A repeatable check, golden input or measurement procedure exists. A benchmark result needs its own observed run. |
| `consumer-test` | A separate consumer exercises the public interface. Its validation command reproduces the check. |
| `native-acceptance` | A dated record reports execution in a named real environment, with the recorded application's state and limitations. |
| `ci-receipt` | A dated CI run reports results for its particular revision, platform and checks. |

The summary counts evidence references by kind, including kinds with zero
references. Inspecting a coordinate shows the evidence levels represented by
its anchors. Several cells can cite the same dated run.
The validator reads local files and verifies their identities. Run the owning
test group to obtain a new test result. For a planned capability the validation
command is a baseline check. Closure also requires its expected artifact and
acceptance criteria. Historical desktop execution, portable fake backends, offline
planning, model transport fixtures and real model inference each have separate
scope. A real host receiving an offline plan proves host execution only.

## Reading the hierarchy

Use `tensor tree` for root → architecture → module → function. Each node
counts its descendant cells. Portfolio status is the weakest descendant
maturity. Mean progress is the integer floor of all descendant progress values
divided by their count. Delivery progress uses weights 4, 2, 1 and 0 for
critical, active, maintenance and deferred cells respectively. Retired cells
carry no delivery weight. Aggregation starts from leaves so that splitting a
module cannot increase its influence. Empty selections or zero delivery weight
have a null score. Filters select the population before aggregation.
`weakest` sorts by maturity, progress and coordinate and includes future
targets in portfolio queries.

Read `closed_cells` alongside `cells` to see how many registered contracts
have met their acceptance criteria. The summary counts the represented modules
and those whose matching contracts have all closed. A module can reach 100
with a single narrowly scoped contract. For example, a Windows console worker
run establishes worker behavior on that platform. A Windows GUI action would
need its own application observation. Full language parity, broader GUI coverage
and product trials have their own acceptance boundaries. A product-wide
maturity percentage would require a defined set of release gates and evidence
for each. [Architecture and remaining scope](architecture.md)

Local checkpoint primitives and continuation recovery have separate contracts
from a durable VM and bytecode execution. The same distinction applies to
an FFI proposal and the existing Rust embedding APIs.

## Future system integration

The following design intentions give future cells a scope and an evidence
anchor. Each begins with deferred priority. Admitting one into delivery
requires a reviewed goal, an eligible priority and a reproducible contract.

- **Sirius Native Shell** would discover a shell-owned capability, dispatch an authorized command and verify its observed result.
- **ns-nova integration** would translate one application-owned operation and a typed result across the boundary.
- **XR semantic control** would identify one scoped spatial object and verify a semantic action against an observed state transition.
- **System resource control** would quantify one bounded resource action and preserve ownership through denial and cancellation.
- **System-level authority model** would define the issuer, resource scope, lifetime and revocation of a grant that crosses a process boundary.

## Maintaining the protocol

Change the cell and its evidence in the same review as its contract. Changing
a coordinate also requires updating its declaration, incoming dependencies and
mainline references. Keep progress and task readiness out of Markdown tables.
Run `sh scripts/check.sh tensor` and the affected capability checks. The
tensor check validates all coordinates, evidence anchors, dependencies,
hierarchy coverage and selected card, including when a query uses filters.

The design references are the Nuis development tensor in
`docs/reference/nuis-development-tensor.md` (reviewed checkout
`335e49bbf9a2add3323b55ebd0dfdfe8fab30180`) and the Gewyvern status protocol
in `docs/project-status-system.md` (reviewed checkout
`aba115a2a346808c792b8ee61c84d817651b3408`). The standalone tool uses the
LeseLang checkout and its locked Rust dependencies.
