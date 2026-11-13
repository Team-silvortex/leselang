# TypeScript / JavaScript Host Through Tauri

The independent [`hosts/tauri`](../hosts/tauri/Cargo.toml) workspace controls
objects implemented in TypeScript and rendered in a real Tauri WebView. The
Rust compiler and VM retain typed source, local object ownership, live checks
and Fuel. A small TypeScript bridge invokes registered application adapters;
the form's input and Save handlers perform the actual application work.

The shared bridge is the framework-independent
[`@leselang/bridge`](javascript-sdk.md) package. This frontend consumes its
compiled public entry; application registration is not tied to this form's
three control names. The portable Rust `run_registered` API and independent
Node consumer exercise custom inventories without a Tauri runtime. The desktop
runner described below retains its existing form-specific verification.

The separate `--agent` mode exposes an interactive conversation entry in this
same application. It uses a replaceable provider and an explicitly labelled
offline planner; the app remains open between workflows. See the
[in-app agent guide](agent-entry.md) for the session contract and desktop steps.

This is an application-owned reference adapter. The application explicitly
registers the objects it permits an agent to control. It does not attach to an
arbitrary third-party WebView or expose an operation for evaluating arbitrary
JavaScript. The five language crates and the independent Consumer do not depend
on Tauri, Node.js, TypeScript or a frontend framework.

## Build And Run

The desktop implementation currently targets macOS with a logged-in graphical
session. It uses the platform WebView through Tauri 2. The portable Rust gateway
and TypeScript bridge can be checked without launching a GUI.

Run from the repository root with the pinned Rust toolchain and Node.js 24:

```sh
npm ci --prefix hosts/tauri --ignore-scripts
npm run build --prefix hosts/tauri
cargo fetch --manifest-path hosts/tauri/Cargo.toml --locked
cargo build --manifest-path hosts/tauri/Cargo.toml --locked --offline --profile lean
target/lean/leselang-tauri-host --describe
target/lean/leselang-tauri-host --text 'TypeScript ✓' --report-json --hold
```

The first command installs the host's pinned TypeScript build tool. The build
emits browser JavaScript and copies the HTML and CSS into `frontend/dist`.
These generated assets and `node_modules` are host build outputs, not language
dependencies. Rust output continues to use the shared repository `target/`.

`--describe` emits the registered `tauri.typescript` catalog without creating a
window, WebView, session or frontend bridge. It uses the shared
[catalog JSON format](host-discovery.md). A portable catalog binary can also be
built with `--no-default-features`.

`--describe-agent` emits the separate in-app task contracts, also without GUI
or transport work. The [conversation entry](agent-entry.md) uses the additive
`agent_start_task` command: zero or one named string input and typed scalar
results, with raw JSON preserving exact integers. It supports no-input checks
and the `save_message(message)` task alongside the existing save command.
The one-shot `--describe` / `--run` / `--text` path below keeps its original
form-specific contract and report.

The default run loads the [form program](../hosts/tauri/examples/form.lese).
`--run FILE` replaces it with a bounded UTF-8 source file; `--text TEXT` supplies
the required `main(text: string)` task input as data. This reference runner
requires a Boolean final result and independently checks the form postcondition.
Use `--hold` to keep the window visible after completion. It does not keep the
finished agent bridge authorized for further requests.

## Drive It From The Independent Agent

The existing [catalog agent](agent-workflow.md) supports both the AppKit and
TypeScript form profiles. Select the Tauri executable; no new agent flag is
needed:

```sh
cargo run --manifest-path consumer/Cargo.toml --example catalog_agent --locked --offline --profile lean -- \
  --host target/lean/leselang-tauri-host \
  --output /tmp/leselang-typescript-agent \
  --text 'TypeScript ✓' \
  --execute
```

Choose a new output directory. Without `--execute`, the agent only reads the
actual catalog and generates a plan. The host now includes its
[semantic task declaration](host-semantics.md), which the agent validates before
generating a typed LeseLang program. With `--execute`, it launches the selected
host once. User text is never interpolated into generated source. The declared
task supplies the bounded CLI input convention and report predicates; the
report must still identify the exact discovered profile. Older catalogs
without the extension retain the original known-form strategy.

The successful program uses nine VM Effects: list controls, find and observe
the input, set text, observe it again, find and observe Save, click it, and
observe the saved result. Save crosses a JavaScript Promise boundary. Its
application mutation must re-enter the bridge's commit check after the await.
Fresh observations establish the result after submission.

The host report has `kind: "tauri_run"`, `profile: "tauri.typescript"`, a typed
Boolean `result`, `effects`, `fuel_remaining`, `verified`, and the public
`native_before` / `native_after` snapshot arrays. In this host those historical
field names carry TypeScript/DOM state, not native pointers. Each adapter checks
that its DOM node remains the registered instance and that displayed values
agree with the application model before publishing a snapshot.

The host and agent require the intended value in both input and Save, exactly
one new Save callback, unchanged Other state and a true program result. The
agent also checks the profile, report kind and expected Effect count. A complete
report with `verified: false` remains failure evidence; the CLI exits nonzero.

## Adapter Responsibilities

The [`@leselang/bridge` source](../packages/javascript/src/index.ts) supplies
`Bridge` and the `Adapter` / `ActionContext` interfaces. The frontend's
[`bridge.ts`](../hosts/tauri/frontend/bridge.ts) is a compatibility re-export.
The
[real form](../hosts/tauri/frontend/app.ts) registers `input`, `save` and `other`.
An adapter provides:

- `read()`: public scalar state such as role, title, value, enabled state,
  editability and callback count.
- `setText(text, context)` or `click(context)`: the operations supported by that
  object. These may return a Promise.
- `context.commit(() => ...)`: a synchronous gate immediately around each
  application or DOM mutation, including mutations after an await.

An asynchronous callback must use `context.commit` for its deferred mutation;
returning a Promise does not make arbitrary JavaScript cancellable. The bridge
is a contract for trusted application adapters, not a JavaScript sandbox.
`context.signal` also exposes cancellation to application work that supports it.

The form's text adapter dispatches a real DOM `input` event. Its Save adapter
dispatches the real button `click`; that listener invokes the shared
[`FormModel`](../hosts/tauri/frontend/form.ts), awaits a microtask and commits
the saved value and callback count. Those actions are implemented in TypeScript.
The Rust gateway does not duplicate the application's save logic.

Applications call `changed(name)` for model changes and dependencies that may
not appear as a different final snapshot. Input changes also advance Save's
revision, so input ABA cannot make an earlier Save observation current again.
Replacing a registered object uses `replace`; removal uses `unregister`.
A same-name replacement advances its generation. The form additionally rejects
an unregistered DOM-node replacement rather than silently resolving a new node
through the old reference.

## Transport And Lifetime Boundaries

The local Rust protocol and this JSON bridge have separate version fields.
Transport version 1 binds each request and reply to one `main` WebView, one
document epoch, one session and a monotonically increasing request identifier.
Object names and generations identify entries in the bridge registry. Actions
also carry the revision observed by the caller.

The Rust gateway retains `ObjectRef`, execution leases and original VM request
correlation locally. It converts validated bridge observations into those
local values. No JSON value restores a Rust object reference, receipt or grant.
The gateway checks the reply source and correlation before accepting data; the
frontend checks scope, object identity, revisions and current operation state
before committing a mutation.

Tauri's `bridge_ready`, `bridge_reply` and `bridge_closed` commands are confined
to the bundled `main` page. The ready handshake installs a window-specific
channel before the worker begins execution. The worker owns `LocalVm`; the
Tauri event loop transports bounded messages and does not hold or poll the VM.
The source driver remains explicit:

```text
poll → authorize → dispatch → pump → accept
```

Closing or navigating the WebView retires its connection. The frontend closes
its bridge on page teardown and on the final host outcome, including failure.
Once the frontend has closed its bridge, old Promise completions fail the commit
gate. Rust transport retirement and frontend cancellation are separate events:
an action may still commit before the frontend receives closure or reaches its
own deadline. Rust refuses replies after its retirement and treats that outcome
as potentially partial. Duplicate, wrong-session and stale-generation messages
are rejected. An action already committed before cancellation remains an external
side effect; neither cancellation nor a missing reply rolls it back.

## Recorded Desktop Verification

On 2026-11-13, the host was exercised on a real macOS Tauri WebView:

- The direct form run with `TypeScript ✓` returned Boolean `true`,
  `verified: true`, nine Effects and 9,425 remaining Fuel.
- The independent catalog agent read the Tauri executable's actual catalog,
  generated its LeseLang task, executed it and independently accepted the
  program result and before/after public-state evidence. This used the same
  external agent executable as the AppKit workflow.
- A separate inspection of the native window's Accessibility tree confirmed
  the `tauri://localhost` page, `TypeScript 控件 ✓` in both the input and
  Saved message, one save, zero Other clicks and the page's Verified status.
  This was an independent Accessibility observation, not screenshot evidence.
- The native process regression passed both exit paths: the successful form
  exited zero; a form that completed Save before returning false exited one,
  with `verified: false` and the saved value still present. The Tauri event
  loop returns control to Rust main so the CLI preserves that outcome.

The visible form and saved state were produced by the TypeScript input and
asynchronous Save handlers. Initialization and final public-state checks remain
extra host queries outside the reported nine VM Effects. These results establish
this registered form's macOS desktop workflow; they do not claim a Linux GUI
run or control of arbitrary third-party Tauri applications.

## Bounds And Verification

The reference runner has a 16 KiB source limit, 4,096-byte UTF-8 task text limit
and an initial 10,000 Fuel. Fuel remains with the original VM through transport
and Promise completion. This is the Tauri host's budget; it does not change the
existing AppKit or AX budgets.

Initialization and final public-state verification use additional host queries
outside the program's nine Effects. The report's Effect and Fuel values describe
the VM run, not those extra verification reads. Bridge requests have a bounded
time budget, and the host also bounds reply size and waiting time. These bounds
do not imply that arbitrary JavaScript computation is preemptible or metered by
LeseLang Fuel. The Rust reply-size check applies after Tauri decodes IPC into a
JSON value; it is not a pre-allocation limit on Tauri's raw IPC ingress.

Run the portable tests and frontend checks with:

```sh
npm test --prefix hosts/tauri
cargo test --manifest-path hosts/tauri/Cargo.toml --no-default-features --locked --offline --profile lean
cargo clippy --manifest-path hosts/tauri/Cargo.toml --no-default-features --all-targets --locked --offline --profile lean -- -D warnings
cargo fmt --manifest-path hosts/tauri/Cargo.toml --all -- --check
```

On an unlocked macOS desktop, explicitly run the native exit-status regression:

```sh
cargo test --manifest-path hosts/tauri/Cargo.toml --locked --offline --profile lean --test native_exit -- --ignored --nocapture
```

It opens two real WebViews. The successful form must exit zero; the other
completes Save and then returns a false final predicate, retaining its saved
value while exiting one. This test is ignored during ordinary headless checks.

The TypeScript tests cover registration, exact request shape, scope and identity
fences, revision changes, dependency ABA, cancellation, deadlines, duplicate or
late work and failures after mutation. The shared form-model tests exercise
its actual asynchronous save logic without claiming to render a DOM. Rust tests
exercise the gateway and local VM contracts with a controlled transport.

CI configures portable TypeScript/Rust checks on Linux and macOS, plus a macOS
desktop build and transport tests. A real WebView run remains a separate desktop acceptance check.
Keep the full independent Consumer passing when changing this adapter's source
or protocol boundaries.
