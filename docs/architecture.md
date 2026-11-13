# Architecture

## Host Direction

LeseLang is an agent control language whose first host target is GUI control;
CLI adapters share the same observation, action and verification contracts.
The language foundation owns typed execution and generic local protocol
primitives. A host owns the objects, live permissions, event processing and the
meaning of submission or completion. See the
[local host protocol draft](host-protocol.md) for the current contract and its
acceptance gates.

## Dependency Direction

Arrows mean consumer to dependency:

```text
leselang-vm -> leselang-hir -> leselang-hir-core
                                     |-> leselang-syntax
                                     |-> leselang-runtime-core
```

The facades also re-export shared syntax and runtime-core types. They do not
implement alternative evaluators or serialize values merely to cross crate
boundaries. The consumer workspace sits outside the package workspace and tests
the same public types with distinct host representations.

The `leselang` executable lives in `leselang-vm` and consumes its public
`embedding` API as a reference host. It supplies `std.print`, file/stdin ingress,
CLI permissions and diagnostics. `std.print` is a host registration, not a new
language primitive; the library remains usable without this executable.

`leselang-runtime-core::discovery` contains owned catalog-description DTOs and
a bounded schema-version 1 JSON encoder. The scalar embedding adapter projects
its immutable registrations; the CLI's `describe`, `check` and `run` share the
same host registration function. Native GUI profiles project shared operation
and field/type declarations offline, without constructing backends or sessions.
This presentation layer is separate from the original `OperationCatalog` and
local object/request/receipt types, which keep their existing ownership and
serialization boundaries. Description schema, operation catalog and local
protocol versions are distinct. The [discovery contract](host-discovery.md)
does not introduce transport, authentication or live resource availability.

The scalar embedding layer forwards an explicit entry name through
`Host::compile_entry` to the existing catalog program compiler; `compile`
continues to select `main`. The complete declaration forest is checked for
either path. The resulting `Program` retains its selected name and the scalar
result type already inferred by compilation. These read-only facts neither
grant authority nor create an additional evaluator or entry wrapper. CLI
`check/run --entry NAME` uses this same API; session startup and Effect driving
stay unchanged.

The separate `Host::compile_task` path admits required scalar entry inputs and
retains their owned declaration-ordered signature. `start_with_arguments` uses
the shared `bind_entry_arguments` ingress gate to validate the complete names,
types, scalar bounds and execution binding capacity before creating a scope.
Values move into initial VM locals; the session borrows their names from its
immutable `Program`. Startup adds no Fuel charge or wrapper computation, and
later reads use the original evaluator's accounting. CLI `--text NAME VALUE`
supplies bounded string data through this path; `check` performs the same input
binding without starting a session. Legacy compilation and startup APIs retain
their parameterless contract.

`embedding::diagnostics` extracts host-neutral categories, bounded messages
and available source spans from the original typed errors. It leaves source
storage, rendering and serialization with the caller and never drives a
session. The CLI consumes this shared view for its default text output and
optional version 1 JSON Lines diagnostic records, adding only its own path,
phase, help and known remaining Fuel. Runtime locations remain absent because
the runtime IR does not retain source spans. The JSON schema is a diagnostic
presentation contract, separate from an Effect or checkpoint wire format.

The syntax crate's `format_source` operates on parsed original tokens, retaining
comments and non-whitespace token spelling and order. It is separate from the
legacy AST formatter, whose comment-erasing behavior remains compatible. The
CLI's `fmt` path uses this source formatter before host registration or semantic
compilation, applies its own smaller input/output budget, and either writes
source to stdout or checks exact formatting with `--check`. Neither mode writes
the input file or invokes an Effect. Syntax validity, formatting conformance and
host-specific semantic admission remain separate checks.

`serde` is the only registry dependency of production library code.
`serde_json` supports tests, executable host reports and host-owned checkpoint fixtures. Neither implies
a built-in journal, authentic storage or replay protection.

## Execution

1. Parse source with bounded source input and diagnostics.
2. Resolve the host catalog and compile typed, bounded HIR.
3. Enter a native session with explicit limits and original fuel.
4. Yield a typed effect request; the host checks current authority and dispatches.
5. Accept a corresponding reply, then explicitly poll to resume the session.

The language does not install a global scheduler, timer, worker or global VM lock.
`Vm` is the shared stacked session; `LocalVm` adds local scope ownership around
that same implementation. GUI-local handles remain on their owning thread.
Sharing immutable IR does not authorize concurrent mutation of one session.

Native hosts can opt into typed native operands through
`CallFlowEnvironment::call_native_argument_type`. The default refuses them.
The compiler keeps these operands in ordinary `Call` argument trees and retains
their `PureType::Result` metadata, so the existing structural checks, lexical
scope and helper hygiene remain in force. Runtime `PreparedValueCall` preparation
reuses the pure evaluator and original Fuel while requiring native adapter
validation. Scalar `PreparedCall` and the high-level scalar embedding API remain
available with their existing contracts.

`compile_catalog_typed_program` also permits helper parameter annotations
resolved by `CatalogSourceEnvironment::source_type_name`. The default is refusal;
the six scalar names retain their fixed meanings. Host-declared native metadata
is resolved after physical/header admission and retained as exact
`PureType::Result` parameter types. Helper operands still pass pure-IR checks,
named binding, hygiene and existing compilation budgets. The AppKit and AX
controls adapters explicitly map `object` and `snapshot`; scalar task entry
inputs remain separate from those execution-local values. Neither this type
mapping nor helper passing authorizes native work or serializes an object.

## Ownership

| Language foundation | Host or product |
| --- | --- |
| Syntax, typing and bounded computation | Operation names and native implementations |
| Fuel accounting and continuation mechanics | Aggregate ingress, callback work limits and scheduling |
| Reply shape/correlation protocols | Live grants, authentic identity and transport |
| Cooperative local lifetime fencing | Cancellation delivery and effect reconciliation |
| Opaque local references, generations and revision comparison | Native object storage, stable snapshots and recording external state changes |
| Generic checkpoint and dispatch primitives | Persistence format, atomic settlement and crash recovery |

Leserpent remains a reference host, not a runtime dependency. Its product evaluator,
compatibility facades, SQLite recovery, UI, authentication and orchestration remain
in the product repository. Do not import them to make a native acceptance test pass.

## Host Adapter Boundaries

The language, independent consumer, AppKit host, macOS Accessibility host and
Tauri host are five separate Cargo workspaces. They share the repository build cache and the
`lean` profile, while keeping their manifests, lockfiles and acceptance suites
independent. A shared output directory does not add host dependencies to the
language. The [contributor guide](../CONTRIBUTING.md#build-output-and-cleanup)
describes the cache layout and cleanup procedure.

The [TypeScript/Tauri host](typescript-host.md) keeps its Rust gateway, Tauri
transport and frontend assets in `hosts/tauri`. The framework-independent
[`@leselang/bridge`](javascript-sdk.md) package lives in `packages/javascript`;
both the Tauri frontend and the separate Node consumer import its public package
entry. The portable Rust `run_registered` entry accepts a bounded application
inventory and returns execution evidence without a form-specific postcondition.
The original form wrapper retains its names and verification policy. A dedicated Rust worker owns the
local VM, while the Tauri event loop routes bounded requests and replies to the
registered TypeScript adapters. The bridge has its own transport version and
page/session/object lifetime checks; it does not serialize local `ObjectRef`,
leases or receipts. The TypeScript application owns DOM handlers and Promise
completion, and gates every deferred mutation with the current bridge context.
Its Node/TypeScript build tools and Tauri dependencies remain outside the five
language crates and the separate Consumer. The existing catalog agent selects
this known form strategy by exact profile and contract, then independently
checks the host's public DOM/model evidence.

The [in-app agent entry](agent-entry.md) adds an application-owned planning
boundary above that host. `AgentSession` reads the host catalog, admits a
provider's bounded source and arguments, invokes the host and requires an
independent application verifier. It serializes user submissions without
polling the VM or installing a language scheduler. The interactive Tauri mode
keeps its model and page alive while creating new bridge/VM identities for each
workflow; cancellation must settle the old worker before another starts.
The offline reference planner has a fixed, documented command grammar. Model
selection, credentials and conversational memory remain provider concerns.

Within `hosts/macos-ax`, new profiles fit these existing responsibilities:

| Layer | Responsibility |
| --- | --- |
| Portable profile models and backend traits | Describe snapshots, fields, supported actions and profile-specific validation without invoking AX. |
| `host.rs` and its private modules | Keep the public protocol types, stable object owners and profile contracts together; `host/catalog.rs` defines operation schemas and bounded source compilation, while `host/session.rs` implements authorization, dispatch, explicit pumping and reply acceptance. `host/tests.rs` exercises those shared contracts. |
| Native access modules | Own thread-affine connections and convert native results into validated profile values. `scope_access.rs` prepares exact window selectors once through `PreparedWindowSelector`; controls and widgets consume the same preparation rules. |
| Private `transport.rs` | Share the actual wire layouts (`RawControl`, `RawScope`) and bounded UTF-8 decoding (`PackedStrings`). Callers retain their own status meanings, row validation and all-or-nothing snapshot publication. |
| Objective-C bridge and profile includes | Perform AX traversal, identity and dependency checks, immediate native preflight and scoped mutations. Process grants and window ownership stay here and in the native access boundary. |

The module split preserves public `host` paths and the original profile entry
points. Sharing code does not merge profile catalogs, remove native validation
or introduce a scheduler. The driver still follows explicit
`poll → authorize → dispatch → accept`, with a host-controlled pump for queued
delivery and a separate observation to verify a submitted action's result.
An optional [observation wait](macos-ax-observation-wait.md) occupies the host-side
interval before accepting a chosen `gui.observe` reply. Its explicit steps use
the original VM Fuel and fixed target identity, while the embedding host owns
the clock and scheduling. Existing catalogs and source syntax remain unchanged.
The separate [source wait catalog](macos-ax-source-waits.md) adds
`gui.wait_value` only through explicit widget opt-in. Source declares its value
condition and budgets using existing argument types; the same host-side wait
engine performs the steps. This keeps application-specific expectations out of
the generic driver while preserving the original 13-operation widget catalog.
An independent [state-wait catalog](macos-ax-state-waits.md) adds typed checked,
selected and position conditions through the same engine. State availability
is part of those conditions; missing state cannot satisfy a boolean or numeric
fallback. The 13- and 14-operation entry points keep their original schemas.

## Acceptance Limits

The existing matrices cover native compilation, bounded control, typed host
effects, suspension/re-entry, cancellation, reply ownership and host-written
checkpoint fixtures. Local protocol conformance uses CLI and headless GUI
models with explicit queues and event pumping, distinguishing an action's
submission from its observed postcondition. The source discovers one of multiple
native objects, binds its typed result and passes it to observation/action calls.
This tests actual dynamic identity rather than a fixed object hidden behind an
operation key. Typed task inputs reuse ordinary initial locals, and native
helper parameters retain exact host type metadata through the same lowering
and hygiene pipeline. Native-valued field projections remain outside the
profile; scalar embedding remains a separate convenience API.

The separate [`hosts/appkit`](appkit-host.md) workspace adds a concrete macOS
adapter for application-owned `NSButton` and `NSTextField` controls, with bounded
identifier discovery, native text replacement, target/action dispatch and an
explicit main-thread desktop self-test. Its form tracks Save's dependency on the
input state so stale queued submissions refuse before native mutation. Its
Objective-C bridge and framework link flags stay outside the language dependency
graph.

The independent [`hosts/macos-ax`](macos-ax-host.md) workspace scopes cross-process
Accessibility access to a TextEdit document or a Calculator window.
The profiles share a typed host/session driver while supplying distinct native
snapshots, fields and mutation policies. Existing TextEdit backend and snapshot
types remain available. TextEdit checks the process instance, document URL and
retained element identity before queuing text replacement. Calculator checks
its window, display and allowed button identities before queuing a named press;
the display, optional last expression and all allowed button states are part of
its observed dependency.
An additive [controls profile](macos-ax-controls.md) discovers individual
buttons and text controls under the same explicit window scope. Connection-local
keys are metadata; fresh resolution issues native references with separate
owners. A bounded registry retains retired identities instead of rebinding
references to replacement controls. Each observation also retains the entire
observed scope epoch, so changes to another control or dependency invalidate a
queued operation. The same driver handles exact selectors, per-target reply
domains and explicit press/text delivery through a portable `ControlsBackend`.
The [application/window selector](macos-ax-applications.md) adds host-side
admission for an exact application bundle and optional PID, followed by bounded
window metadata discovery and exact unique selection. It pins the process's
start identity/executable and retains the chosen native window, so title
changes do not redirect it and same-title replacements do not inherit its
authority. Connection-local window keys and retired entries stay separate
from the execution-bound control references. This scope setup supplies another
`ControlsAccess` to the existing catalog; it adds no application selection
operation to language execution.
The optional [widgets profile](macos-ax-widgets.md) adds scoped popup menus,
desired checkbox state, native item selection and normalized scrollbar
positioning through that shared session engine. It retains the old controls
types and catalog. Widget observations include the relevant checked, selected,
membership and range dependencies; menu and list items are native references,
not serialized positions. Transient menu choices remain correlated with their
stable owner while item identity is checked before delivery.
Widget-only exact role/value discovery returns the same scoped native reference
as other selectors, without treating a displayed file name as action authority.
The Finder reference follows observed parent relationships from a name field
to its actual row, then separately verifies row selection and scroll position.
Explicit [auxiliary-process grants](macos-ax-auxiliary-processes.md) keep helper
PID/start-time/executable checks in the native widget transport. A selected
window remains the sole traversal root, and revocable process approval does
not add a helper's other windows or new source-language operations.
The profiles verify results through new observations. Their external state checks
are snapshot based: they cannot detect an unseen A→B→A change or close the race
between a final read and a native write. This limitation belongs to the adapter
and does not weaken the own-process AppKit host's mutation tracking or the
language's local lifetime, reply and Fuel contracts. A real desktop run requires
the supported application state and permission for the executing host; model tests alone
do not establish it.

Broader application or cross-platform GUI automation, a remote/durable host protocol,
full language/host parity and stable FFI ABI each require their own evidence and
release decisions. The current tests do not establish those milestones or a
security sandbox for native callbacks.
