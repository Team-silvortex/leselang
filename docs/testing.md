# Testing And Compatibility Requirements

Use [the contributor guide](../CONTRIBUTING.md#verification) to select checks by
changed layer. [`scripts/check.sh`](../scripts/README.md) is the common local and
CI entry point: `--list` prints commands without executing them, and a group runs
only its explicit checks. It uses pinned dependencies, offline Cargo and the
shared `lean` profile. It does not install dependencies or launch a GUI.

The requirements below explain the compatibility and failure cases each area
must retain. Dependency setup commands are separate from running checks. A
passing portable suite does not establish native desktop behavior; desktop
procedures are explicit and their dated results belong in
[the acceptance archive](acceptance/README.md).

## Commands And Scope

Fetch the pinned dependencies once, then validate the language and consumer
workspaces offline:

```sh
cargo fetch --locked
cargo fetch --manifest-path consumer/Cargo.toml --locked
cargo fetch --manifest-path hosts/appkit/Cargo.toml --locked
cargo fetch --manifest-path hosts/macos-ax/Cargo.toml --locked
cargo fetch --manifest-path hosts/registered/Cargo.toml --locked
```

The Tauri/npm prerequisites are listed below. With all required dependencies
installed, inspect or run the common checks:

```sh
sh scripts/check.sh --list all
sh scripts/check.sh all
```

The consumer is deliberately a separate Cargo workspace. Its repository-boundary
test checks actual Cargo metadata, including all-features builds, so a sibling
checkout or optional product dependency cannot silently become a prerequisite.

Use the `lean` profile for local checks. The independent workspaces share the repository's
`target/` through `.cargo/config.toml`; their independence concerns dependencies
and acceptance, not separate build caches. See [build output and cleanup](../CONTRIBUTING.md#build-output-and-cleanup)
in the contributor guide before changing output paths. No containers or external server are needed
for these native tests.

CLI integration tests run with the workspace suite. The `smoke` group
checks a pure program, evaluates its result and executes two explicitly granted
stdout Effects. Keep the examples in `examples/` executable when changing the
scalar language or CLI host; document profile changes in
[the language guide](language.md).

## Diagnostics

Keep `embedding::diagnostics` host-neutral. Structured categories and source
locations must come from the public error view, while terminal escaping, CLI
capability hints and JSON Lines presentation belong to the executable. Cover
both default text and opt-in JSON for syntax, compilation, input, usage and
runtime failures; preserve stdout and previously delivered Effects. Test
Unicode scalar columns, UTF-8 byte spans and absent runtime locations without
turning diagnostic messages into a parsing contract. Verify the API from the
independent Consumer as well as the CLI. Dotted host operation names are exact
catalog keys, so parser admission must remain consistent with registration,
including names with more than two segments.

## Catalog Discovery

Catalog discovery projects actual operation, capability and field declarations;
do not maintain a separate list that can diverge from compilation. Preserve the
borrowed `Host::describe` API and native catalog/handle serialization boundaries.
Test optional and deny-all domains, all scalar alternatives, exact native type
references, Unicode escaping, declaration order and every metadata/output bound.
Encoding failure returns no partial record; external write failures must not
retry. Cover the versioned JSON contract in the independent
[`catalog_discovery` Consumer tests](../consumer/tests/catalog_discovery.rs), including
zero callbacks, original Fuel cutoffs, revocation and partial side effects.
For GUI `--describe`, test all catalog profiles without backend creation,
permission queries or native dispatch; keep live `--discover` inventory distinct.
Document schema changes in the [discovery guide](host-discovery.md), and
preserve unknown-field tolerance without treating unknown types as wildcards.

## Independent Rust Catalog Agent

Keep the [catalog agent](agent-workflow.md) independent of native host
dependencies. Its optional [semantic metadata](host-semantics.md) path
validates finite task declarations against actual catalog signatures, then
generates source and checks declared report predicates without profile branches.
Keep absent-metadata legacy strategies compatible, and reject malformed present
metadata without fallback. Equal types do not establish equal operation meaning.
Task text stays in typed inputs rather than generated
source. Cover malformed or incompatible catalogs and reports, false or
contradictory postconditions, subprocess output and time limits, missing
executables, and preservation of failure evidence without automatic retry.
The default planning path must finish before GUI execution; `--execute` runs
the selected host once. Keep output directories new and refuse overwriting
previous evidence. Run the example tests explicitly, since ordinary package
tests do not execute example unit tests by default. CI also builds a plan
against the real offline AppKit, TypeScript and Calculator catalogs on Linux
and macOS. Forward the same literal host selectors during discovery and
execution, and reject a requested task that differs from the discovered
contract. Calculator requires no text input: test its ten-Effect success,
nonzero initial display with zero presses, mismatched last expression, and
failed reports without replay. Execute the agent-generated Calculator fixture
through the actual host driver with a fake backend as well as checking types.
For semantics, cover an unknown profile with renamed operations, controls,
input and task; forward/duplicate bindings; mismatched field/argument types;
unknown versions; ambiguous report matches; missing nullable fields; checked
integer arithmetic; and failed predicates without retry. Preserve the base
catalog structs and byte output, global bounds and existing host Fuel budgets.
Keep application task declarations in hosts rather than the language crates.

## Observed Result Contracts

For semantics v2 observed results, preserve every v1 public type, serializer and
golden byte. Publish v2 through its separate owned declarations and serializer;
fixed result JSON retains its existing shape. Reject observed contracts under
v1, unknown fields, unsupported scalar types, empty/oversized paths and paths
rooted in the report's `result`. An observed result must equal its declared
actual evidence value exactly, in addition to the original report header,
Fuel, Effect and predicate checks. Cover wrong types and values, missing or
ambiguous matches, duplicate JSON keys, exact `u64` values and forged/stale
observations rejected by the application host. Run the same catalog with at
least two different dynamic values; do not replace observed verification with
type-only admission or use the catalog's expected value as runtime evidence.

## Entry Selection

Explicit entry selection must use the existing catalog compiler without
synthetic source or an extra runtime wrapper. Keep default `main` behavior,
complete cold/unused declaration checks and the ban on calling the selected
entry covered. Test named-entry execution against equivalent `main` source for
results, original Fuel and partial Effect delivery, including revocation and
cancellation. Verify owned entry metadata and inferred scalar result types from
the independent Consumer, and `--entry` parsing and text/JSON failures from the
CLI. Program metadata is descriptive and must not authorize execution.

## Task Inputs

Typed task ingress is an explicit `compile_task` / `start_with_arguments` path.
Keep legacy parameterless admission unchanged, require every named input and
check types, scalar bounds and binding capacity before session creation. Test
reordered inputs, all six scalar kinds, foreign programs and malformed inputs
with no host work. The shared binder must move payloads rather than clone them;
compare task-local reads against native `LocalVm` initial bindings at every
Fuel cutoff. Preserve original Fuel through two Effects, captures, revocation,
cancellation, late replies and partial side effects. The independent
[`task_inputs` tests](../consumer/tests/task_inputs.rs) exercise these contracts.
For CLI `--text`, cover literal Unicode/quote/backslash/newline values, all
option and size limits, source stdin, quiet `check`, JSON input diagnostics and
zero output on rejected inputs.

## Native Helper Types

Native helper type annotations require the typed catalog compiler and explicit
host type-name resolution. Preserve the old compiler's scalar-only profile and
default refusal of native type names. Test exact native type matching, cold
errors, helper hygiene and pure previously bound operands; no helper parameter
may manufacture an object identity or admit an Effectful argument. GUI adapters
must retain runtime object/lease/revision/authority checks when a helper receives
an `object` or `snapshot`. Keep source parameter inputs separate from native
object values and keep native field projections scalar.

## Source Formatting

Source formatting uses `leselang_syntax::format_source`, while the older
AST-based `format` retains its comment-erasing compatibility contract. Keep
non-whitespace token spellings and order, comment text and neighboring-token
relationships, string contents and formatting idempotence covered by syntax
tests. Verify formatted programs through an independent Consumer, including
their results, Effect order, original Fuel and cold-code admission failures.
The CLI's `fmt` and `fmt --check` must not write input files or register or run
host operations. Cover input/output bounds before emitting stdout, formatting
differences, partial stdout write failures and the existing text/JSON
diagnostic contracts. Document formatting profile changes in the
[language guide](language.md#format-source-without-losing-comments).

## Local Host Protocol

The `protocol_hosts` example runs both reference adapters against the local host
protocol. Conformance changes must preserve stale-object/revision rejection,
VM-scope revocation and the distinction between submitted actions and observed
postconditions. These headless fixtures do not establish compatibility with a
concrete GUI framework; update [the protocol draft](host-protocol.md) when
its evidence or remaining gates change.

## AppKit Adapter

The real AppKit adapter is an independent workspace in `hosts/appkit`. Keep its
framework link flags, Objective-C bridge and widget policy out of the five
language crates. On macOS, also run:

```sh
sh scripts/check.sh appkit
```

Then explicitly run the native desktop acceptance:

```sh
cargo run --manifest-path hosts/appkit/Cargo.toml --locked --offline --profile lean -- --self-test
hosts/appkit/test-native.sh
```

The self-test and native bridge test run on the process's main thread and use
actual AppKit controls. They require a logged-in macOS 14+ desktop session and
Xcode Command Line Tools. The ordinary package tests cover command-line ingress
and compile-time thread affinity without starting GUI work on test workers.
There is no successful headless fallback or skipped GUI test reported as proof.
CI lints the host and runs its portable catalog, report and ingress tests on
Linux and macOS; native AppKit builds run on macOS. Run
both desktop commands explicitly when changing its bridge, event pump,
discovery, identity/dependency tracking or operation policy. Preserve both the
four-Effect button compatibility example
and the source-file form workflow. See
[the AppKit guide](appkit-host.md) for the adapter's tested scope.
For structured execution reports, keep stdout to one complete bounded JSON
record and send traces to stderr. Project public scalar fields explicitly;
never serialize `WidgetSnapshot` identities or native authority. Native
before/after verification reads must remain identified separately from VM
Effects and their Fuel. Exercise the generated
[agent workflow](agent-workflow.md#execute-and-verify-once) on a real
desktop when changing that integration, and record the source result and
independent native postconditions rather than treating successful launch as
acceptance.

## Tauri And JavaScript Checks

The independent `hosts/tauri` workspace supplies the registered TypeScript
adapter and macOS WebView transport. Run its frontend and portable checks:

```sh
npm ci --prefix packages/javascript --ignore-scripts
npm ci --prefix hosts/tauri --ignore-scripts
cargo fetch --manifest-path hosts/tauri/Cargo.toml --locked
sh scripts/check.sh tauri-portable
```

`npm test --prefix hosts/tauri` checks the frontend only. The shared `javascript`
check group runs the SDK first and then the frontend; `npm run test:all --prefix hosts/tauri` retains that combined npm entry.
Install the SDK's development dependencies before frontend compilation too:
the build emits both SDK entries, while WebView assets include only the browser
entry's module tree. Node typings are build dependencies, not dependencies of
the installed package's public declarations.

The shared bridge is an independent package in `packages/javascript`. Validate
its public ESM/declaration exports and packed artifact, then run the independent
JavaScript application against the real Rust VM:

```sh
npm ci --prefix packages/javascript --ignore-scripts
npm ci --prefix consumer/javascript --ignore-scripts
sh scripts/check.sh javascript
sh scripts/check.sh node-consumer
```

Keep Tauri-specific behavior outside the SDK, and consume the package entry
rather than importing its source files. Preserve stable object slots within a
session, bounded inventories and explicit re-find after replacement. The generic
execution report carries evidence; application verification remains with its
caller. See the [JavaScript SDK guide](javascript-sdk.md).

## Public Registered Application Host

Run the `javascript` group after changing `createRegisteredAgentHost`. The SDK
suite covers independent observations and lifecycle boundaries, including
synchronous application callback reentry, cancellation, partial attachment,
cleanup failures and late replies. The packed-package Consumer imports only
`@leselang/bridge`, checks its public TypeScript declarations and executes two
Promise actions through a scripted transport and the real Bridge. It supplies
its own endpoint, profile, report kind and observation field names; it does not
execute a language VM in that package test.

The original Tauri host tests remain as transport-adapter integration checks.
Rebuild frontend assets and the native desktop host, then exercise both form
and board windows for real WebView/VM acceptance. The independent `consumer`
and `node-consumer` groups continue to check the existing Rust and Node public
boundaries. The additional Node-owned application now exercises this public
host through its own real Rust worker; its separate transport fixtures remain
distinct from those real VM checks.
See the [registered host guide](registered-agent-host.md).

## Node-Owned Agent And Rust Worker

Semantic authoring changes also require the `javascript` group. Its packed
Consumer checks public v1/v2 declaration types, while runtime tests cover
duplicate keys, invalid references and types, raw integer tokens, owned inputs,
combined byte/node/depth budgets and exact u64 authoring. The ordinary catalog
and report readers must retain their existing object-ingress contracts.
The Node Consumer's `check:catalogs` script checks the reference declarations as
strict JavaScript, and its installed tarball Consumer repeats those checks using
only installed public declarations. Its real-worker suite compiles and verifies
an authored `u64::MAX` result without passing that integer through Number.

Registered-task constructor changes also run `consumer`: the expanded
publication catalog and four generated programs are shared fixtures checked by
both Rust and JavaScript planners, and compiled by the independent Rust Consumer.
Keep internal binding allocation from capturing unknown or forward references,
check budgets after expansion, and retain explicit report predicates. The real
worker tests cover another application's self-disabling completion and a text
field that becomes read-only, along with missing/disabled/wrong-role controls,
stale revisions, false acknowledgements and unsuccessful business postconditions.

The `registered` group formats, tests and lints the standalone library and
worker, inspects its complete resolved dependency graph and lockfile for GUI
framework dependencies, and runs a separate Rust Consumer through two actual
Effects. The Consumer uses the public crate and the shared build cache; it does
not include implementation source or spawn a nested build inside `cargo test`.
Tauri compatibility tests check shared Rust type identity and equivalent reports.

The `node-consumer` group builds the standalone `leselang-agent-worker`, the old
compatibility `agent_worker` entry, and the Rust-launched `node_host`,
then runs the original Rust-launched JS application and Node-owned agent
integration. It also packs and installs the SDK into an external temporary
Consumer that uses `@leselang/bridge/node` with the real worker, tests a complete
workflow, cancellation and partial effects, and checks public TypeScript types
without installing Node typings. The `javascript` group owns the process-carrier
fixtures and browser isolation tests. The process fixtures are deliberately
synthetic workers; they test malformed framing, early terminal data, nonzero
exit, process deadlines, cancellation/reaping and old-scope isolation.

The real-worker suite must preserve one parent application across tasks, verify
byte-identical base descriptions from the new and compatibility executables,
observed-result bindings and conditions, retain a mutation after a post-commit
failure, and reject a deferred commit after SDK cancellation. It also checks
native startup/request deadlines, compilation before application requests and
exact `u64` report bytes. Run the [offline CLI demo](node-agent-host.md#build-and-run)
in both success and `--fault-after-commit` modes; the latter is expected to exit
nonzero while showing retained publication. The existing Tauri prepared-runner
tests compare report content, Effects and Fuel across the two fixed identities.

## Second TypeScript Application

The [second TypeScript GUI](taskboard-host.md) additionally requires catalog and
source golden checks, real Rust compilation of every declared program, and
completion/observation verification for both task records. Check that a completed
task cannot be counted twice and the unrelated task is preserved. The shared
provider must select from either application's catalog without an application
branch; an invalid later form task in a board plan must prevent all execution.
Native page, catalog and success/failure report identities must remain bound to
the startup application, including cancellation and retirement paths.

For actions that disable their own control or make it read-only, verify both
successful committed settlement and refusal of a further mutation. External
state/revision changes, object replacement, revocation, cancellation, deadline
and rejected Promises must still fence success and preserve earlier effects.
Real desktop acceptance should include a pending-to-done workflow, the same
request after completion, an invalid cross-application plan, cancellation,
offline commands, and the original form after common chat extraction.

## Application Agent Entry

The in-app `AgentSession` tests must cover cancellation in every awaited phase,
late provider/host completion, overlapping submissions, plan/report ownership
and failed application verification. The Tauri `--agent` page adds reusable
application state with a fresh transport/VM session per admitted step. On a desktop,
exercise two sequential commands and an unsupported command through the actual
conversation UI; the latter must perform no application operation. Preserve
the original one-shot desktop acceptance as well. The reference planner must
remain explicitly labelled offline; separately configured model mode must not
claim live inference acceptance from protocol fixtures. See
the [agent-entry guide](agent-entry.md).

## Semantic Planning And Task Execution

Keep the shared `createSemanticProvider` independent of form operation names,
controls and task aliases. Match the Rust semantic planner's golden source for
all reference catalogs, and test renamed applications and zero-input tasks.
Preserve exact `u64` JSON tokens, reject duplicate keys and unsafe object numbers,
and bound raw catalogs and reports before planning or verification. The host
must independently rebuild the declared plan, bind evidence to its execution
and original cancellation signal, and compare both initial and fresh observed
state; provider summaries never establish success. Run the independent Node
Consumer's declared workflow through the real Rust VM, including unchanged
unrelated controls, failure after a committed mutation and failed declared
postconditions without automatic replay. For the Tauri task carrier, admit no
input or one exact named string before transport bootstrap; test all six scalar
result types and refusal of native results. Keep the original form entry and
catalog compatible. Compare old/new execution Fuel and Effect counts for the
same source, preserve exact raw integer reports, and run no-input, renamed-input
and different-result workflows through the real conversation UI. Reference
carrier input restrictions must remain explicit even when the language can
describe more task shapes.

## Tauri Transport And Desktop Verification

Keep the Tauri dependency, IPC command permissions and frontend build toolchain
outside the five language crates and independent Consumer. The gateway must
retain Rust object ownership and original VM correlation while validating the
separate transport version, page epoch, session, object generation and revision.
Every deferred JavaScript mutation must re-enter `context.commit`; neither a
Promise nor a timeout can roll back earlier effects. Test replacement, ABA,
wrong scope, duplicate/late replies, cancellation and partial mutations.
The actual DOM handlers share their application logic with the portable model
tests, but those tests do not establish WebView behavior. On macOS, run `sh scripts/check.sh tauri-desktop` after building frontend
assets, then explicitly run the [TypeScript agent workflow](typescript-host.md#drive-it-from-the-independent-agent)
against a real window. Record the source result, original Fuel, DOM/model
observations and unrelated-control state. The Rust worker owns `LocalVm`; Tauri
callbacks only transport bounded data and must not poll or resume it reentrantly.

## macOS Accessibility Adapter

The independent `hosts/macos-ax` workspace supplies scoped TextEdit and Calculator
Accessibility profiles over one shared typed host driver. Keep its process
selection, permissions, traversal and native bridge out of the language crates.
Preserve the existing TextEdit `Backend`, `Snapshot` and default driver entry
points when extending profiles. Run:

```sh
sh scripts/check.sh macos-ax
```

On macOS, also run `hosts/macos-ax/test-native.sh`, build the host and follow the
permission check and the relevant TextEdit or Calculator workflow in
[the Accessibility guide](macos-ax-host.md). The native boundary checks
cover URL/text admission and AX error handling without remote GUI calls. Ordinary
backend tests do not establish cross-process desktop execution. Use a dedicated
new file for TextEdit. Calculator requires the supported Basic-mode UI and an
initial display of `0`; obtain authorization before clearing an existing
calculation and restore its original display after the test. Preserve unrelated
documents and application state. Do not
automatically grant Accessibility access or retry an uncertain native mutation.
Document snapshot-based freshness separately from the application-owned
AppKit host's mutation tracking: a remote read followed by a write is not an
atomic operation, and unseen A→B→A changes cannot be ruled out. Calculator
snapshots must include the display, optional last-expression dependency and the
allowed buttons' identity and enabled state. Test expression changes with an
unchanged displayed result; testing only the chosen button or the display would
miss stale calculations.

## Accessibility Control Discovery

The additive `--controls` mode uses the same typed driver with per-control
native references. Keep its discovery keys connection-local and non-reusable,
preserve owner tombstones when controls disappear, and reject empty/ambiguous
selectors and cross-target replies. Test inventory reordering and replacement
separately. Scope freshness must include other controls and non-target
dependencies, including Calculator expression changes with an unchanged
display or selected button. A queued action must recheck the complete expected
scope before delivery. Exercise both controls actions, stale keys, identity
exhaustion, permission revocation, cancellation, late replies and partial
effects without weakening the original profiles' regression tests. Follow the
[control guide](macos-ax-controls.md) for explicit discovery/source runs;
`--controls` has no implicit demonstration action. Record real controls-mode
desktop evidence separately from earlier profile runs and fake-backend tests.

## Application And Window Selection

For [generic application/window selection](macos-ax-applications.md), keep
the bundle path and optional PID admission exact, preserve same-user process
executable/start checks, and require unique exact window selectors. Cover
ambiguous or absent matches, wrong-process PIDs, stale/cross-connection window
keys, title changes, reordered windows and same-title replacements. Selecting
a window pins its native identity; later reads must not rerun a selector and
silently switch targets. `--windows` remains metadata-only, and generic
controls runs require explicit window selection and source. Use dedicated test
content in another real application and inspect its final state separately
before recording desktop acceptance; native test doubles alone are insufficient.

## Accessibility Widgets

The optional [widgets catalog](macos-ax-widgets.md) preserves the original
controls API while adding typed menu, checkbox, selection and scroll operations.
Test item membership with native identities, including menu items that vanish
after a successful choice. Replies must remain correlated with the stable
owner and execution without weakening cross-target or late-reply checks. Cover
mixed checkbox values, unsupported capabilities, selection changes, non-finite
scroll ranges, invalid normalized positions and stale queued operations.
Value selectors must require an exact nonempty role, distinguish an absent
value from an empty string, and reject duplicate matches including disabled
items. Keep the Finder example sources covered directly by reference tests,
including an acknowledged menu choice whose immediate observation has not yet
updated, and stopping before a second mutation after a failed postcondition.
Desktop acceptance should use disposable document/dialog content and separately
verify each supported operation family. Opening a menu does not authorize the
global menu bar, and selecting an item must not silently activate or open it.

## Auxiliary Process Grants

For [auxiliary-process grants](macos-ax-auxiliary-processes.md), keep the
default same-process refusal and cover exact process identity, process restart
or PID reuse, mismatched executable/user, duplicate and excessive grants,
foreign-process boundaries, and unreachable objects in an otherwise approved
process. Revocation must fence dispatch, queued delivery and reply acceptance
without undoing delivered effects. Discovery must not grant approval or query
an unapproved helper's other windows. Native callback storage must outlive the
connection and remain confined to the owning thread.

## Model Provider

The optional [model provider](model-provider.md) uses an application-owned
transport and returns declared task data; canonical source and verification
remain catalog-owned. Keep credentials in the native backend, preserve exact
catalog integers, and reject malformed or undeclared model output before VM
execution. Test HTTP errors, bounded responses, cancellation before admission,
page retirement and late completion without retry or planner fallback. Native
loopback tests need permission to bind a local socket. A protocol fixture and
real WebView check do not establish live-model inference; report that distinction
explicitly. Model integration must preserve the no-desktop build and independent
Consumers.

## Sequential Workflows And Result Bindings

For [sequential agent workflows](agent-workflows.md), admit every step before
executing any, pin the complete lossless catalog including verification metadata,
and verify each ordinary host session before continuing. Cover invalid later
steps with zero executions, changed catalogs, failed later verification, and
cancellation before dispatch, during work and between completed steps. Preserve
prior reports and possible side effects. Each step retains its own Fuel and
authority; a workflow is not an atomic transaction and must not silently retry.

For verified-result bindings, reject the complete invalid reference graph before
the first execution. Only earlier steps with both semantic and host verification
may supply a value; decode their actual reports without rounding integers or
substituting expected catalog values. Cover exact `u64` conversion, format and
parameter mismatches, source verification failure, cancellation and retained
earlier effects. Keep symbolic bindings in the aggregate plan and resolved
literal inputs in attempted step plans. Recheck the pinned catalog before
resolving a step's inputs, and preserve the default literal-only model provider.

For [verified-result requirements](agent-workflows.md#require-a-verified-result-before-continuing),
admit the complete plan and all descriptors before executing any step. Cover
duplicate targets, unknown fields, forward/self references, unsupported source
types, format mismatches and noncanonical or out-of-range expected values.
Keep exact `u64` comparisons lossless, including its maximum value. Test
requirements with and without bindings; every source must pass semantic and
application verification even if no binding consumes it. A mismatch must
stop before the target's execute/verify calls while preserving earlier outcomes
and effects. Only completed comparisons appear in immutable outcome evidence;
cancellation, late reports, changed catalogs and unvisited requirements cannot
fabricate comparison results. Preserve unchanged plan/outcome shapes when no
requirements are used, and the separate default-off model options. Validate
the packed external JS/TS Consumer and the actual Tauri host's success and
no-save mismatch paths. A desktop run should use the same selected plan with
matching and nonmatching current text; record its observed Effects separately
from any claim of natural-language model inference. Do not describe the check
as atomic compare-and-update or a guarantee that GUI state cannot change later.
