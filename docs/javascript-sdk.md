# Register JavaScript Objects For LeseLang

The independent [`@leselang/bridge`](../packages/javascript/package.json)
package lets a JavaScript or TypeScript application register the objects that
LeseLang may observe and operate. It exports browser-compatible ESM and TypeScript
declarations, with no runtime package dependencies. The package is currently a
private, repository-local integration; it has not been published to npm.

The same bridge serves the [real Tauri form](typescript-host.md) and the
[standalone JavaScript consumer](../consumer/javascript/package.json). The
consumer uses a Node.js process with its own application model, without a
WebView or a Tauri runtime. The Rust compiler and VM still execute LeseLang.
The package does not evaluate JavaScript supplied by an agent.

For an application-owned conversation entry, the same package exports
`AgentSession`, `createSemanticProvider`, `prepareSemanticCatalog` and a
provider/host contract. The shared offline provider compiles host-declared
tasks and the host independently verifies their declared report conditions.
See the
[in-app agent guide](agent-entry.md) for message-to-workflow planning, explicit
execution, verification and cancellation across sequential tasks.
The Tauri task carrier now accepts no input or one exact named string and
returns any existing scalar result type. Its raw report preserves full `u64`
values; the in-app host checks declared predicates and fresh observations.
The Node reference below retains its existing Boolean publication contract.

## Register An Application Adapter

Import the public package entry in either JavaScript or TypeScript:

```js
import { Bridge } from "@leselang/bridge";

const bridge = new Bridge({
  window: "node-main",
  epoch: crypto.randomUUID(),
  session: crypto.randomUUID(),
});
let count = 0;

bridge.register("increment", {
  read() {
    return {
      role: "button", title: "Increment", value: String(count),
      enabled: true, editable: false, clicks: count,
    };
  },
  async click(context) {
    await Promise.resolve();
    context.commit(() => { count += 1; });
  },
});
```

The application's transport delivers each decoded request to
`await bridge.handle(request)` and returns its reply to the same Rust session.
Registration alone does not start a VM, poll for tasks or install a scheduler.
`window` is a transport endpoint label in this protocol; a Node endpoint does
not need a graphical window. The Tauri transport separately restricts its
endpoint to the bundled `main` WebView.

An adapter implements `read()` and, when supported, `click(context)` or
`setText(text, context)`. The current control vocabulary provides
`gui.list`, `gui.find`, `gui.observe`, `gui.set_text` and `gui.click`, with
`button` and `text_field` snapshots. Applications choose their own names and
behavior within that vocabulary. This version does not infer arbitrary method
signatures from JavaScript objects.

`read()` must be side-effect free and return the current scalar state. Each
mutation made through an asynchronous adapter must enter the synchronous
`context.commit` gate after its last await. This rechecks the current lifetime,
grant, object generation, observation revision and deadline. A Promise returned
from the commit callback is invalid; place asynchronous preparation outside it.

Call `bridge.changed(name)` when an external application change or dependency
must invalidate an earlier observation, including A → B → A changes. For example,
changing a draft also invalidates a publish control whose action depends on
that draft. Use `replace(name, adapter)` for a new object instance, and
`unregister(name)` when an instance is removed. Do not silently redirect a
registered adapter to a different DOM node or application object.

These are contracts for trusted application code. The bridge cannot stop an
adapter that bypasses `context.commit`, undo network requests, or preempt
arbitrary JavaScript computation. `context.signal` supports application work
that cooperates with cancellation. Existing side effects survive cancellation
and errors; the bridge never retries an uncertain action.

## Generic Rust Execution And Object Lifetimes

`leselang_tauri_host::run_registered(transport, scope, source, text)` provides
the portable execution entry. It is available with `--no-default-features`,
without linking the desktop Tauri runtime. It uses `main(text: string)`, the
existing typed compiler, local object owners, 10,000 initial Fuel and an explicit
`poll → authorize → dispatch → pump → accept` loop.

The registered inventory has at most 64 unique ASCII names of 1–64 characters
(`A–Z`, `a–z`, digits, `_`, `-`). Initialization establishes stable local slots
for that session. Reordering the same inventory is allowed; adding or removing
names requires a new session. A changed inventory retires the current bridge
when an explicit `gui.list` observes it; there is no background inventory
monitor. The frozen name-to-slot mapping never assigns an old reference to a
different object, and finding a name outside it fails. A same-name
replacement requires a fresh `gui.find` and a higher generation; previous
references remain invalid.

The generic report identifies `javascript.registered`, returns the Boolean
program result, Effect count, remaining Fuel and `objects_before` /
`objects_after` snapshots. It does not add a `verified` field or interpret the
application's business postcondition. The calling host must perform those
checks. Initialization and final snapshot reads are extra host queries outside
the reported VM Effects and Fuel. JavaScript work itself is not metered by
LeseLang Fuel.

The original Tauri form entry and `tauri.typescript` report remain available.
That wrapper still requires its three named controls and verifies its specific
Save postcondition. Generic Rust and TypeScript planners use explicit semantic
task declarations; they do not infer a task from a custom inventory's signatures.
`describe_registered()` exposes the shared operation signatures under the
`javascript.registered` profile without opening a session or launching Node.

## Run The Independent JavaScript Consumer

From the repository root, using Node.js 24 and the pinned Rust toolchain:

```sh
npm ci --prefix packages/javascript --ignore-scripts
npm test --prefix packages/javascript
npm ci --prefix consumer/javascript --ignore-scripts
cargo fetch --manifest-path hosts/tauri/Cargo.toml --locked
cargo build --manifest-path hosts/tauri/Cargo.toml --no-default-features --example node_host --locked --offline --profile lean
npm test --prefix consumer/javascript
cargo test --manifest-path hosts/tauri/Cargo.toml --no-default-features --example node_host --locked --offline --profile lean -- --ignored
```

The reference child-process transport currently targets Linux and macOS and
uses the operating system's random source for per-request challenges. Its
offline catalog can be read with `target/lean/examples/node_host --describe`;
this does not start the JavaScript application.
The Node carrier also accepts `--run FILE --text VALUE`, admitting at most
16 KiB of UTF-8 source before starting its child. Omitting `--run` preserves
the original reference program.
The explicit ignored Rust tests launch controlled Node fixtures to check early
and pre-sent future replies and a child that stalls without reading its input.
Ordinary Rust tests remain usable without Node installed.

The SDK tests exercise the installed package surface, including a packed
artifact imported by an external JavaScript/TypeScript consumer. The standalone
application imports only that public entry. Its Rust driver launches the fixed
consumer program with literal arguments and transports JSON over bounded
standard-input/output records. It does not execute task text as JavaScript or
interpolate it into LeseLang source. This carrier adds its own envelope and a
fresh random challenge for each exchange, so a pre-sent future reply cannot
become valid merely because the reader thread is scheduled late. The inner SDK
transport protocol remains version 1. Writes, reads and child lifetime are
bounded; a missing or invalid reply retires the transport without replay and
kills/reaps its direct Node child. This is not a descendant-process supervisor.
The challenge correlates one exchange; it is not a permission grant. This
reference carrier's JSON record limit is 32 KiB including LF, its per-exchange
deadline is five seconds, and its final report limit is 128 KiB. Its fixed
application emits bounded diagnostic state to stderr separately from replies.

The application registers `draft`, `publish`, `status`, `reset` and `unrelated`.
The LeseLang program changes the draft, invokes its asynchronous publish handler
and makes fresh observations. The caller verifies the published value, callback
count and unchanged unrelated controls. A separate failure mode commits the
publish operation and then throws; its state evidence preserves that mutation,
and the process fails without retrying the operation.

The same Consumer also declares its own `publish_article` semantic task in
[`semantic-contract.mjs`](../consumer/javascript/semantic-contract.mjs).
The public `createSemanticProvider` and `AgentSession` generate a program from
`task publish_article:...`, execute that exact source through the Rust carrier,
and verify the actual report. This uses the application's five custom controls,
without importing the Tauri form planner or source. Tests cover literal input
isolation, an undeclared task, committed failure without retry and a report
that passes Rust's reference check but fails the application's stronger
declared status condition.

The Node consumer is a non-GUI acceptance test of the package and cross-process
execution. It does not establish browser or WebView behavior. Keep the
[native Tauri acceptance](typescript-host.md#bounds-and-verification) when
changing the SDK's browser integration.

## Recorded Acceptance

On 2026-11-13, the standalone Node application ran on macOS using the package
entry and the real Rust compiler/VM. A task containing Chinese text, quotes,
a backslash and a newline returned Boolean `true`, nine Effects and 9,149
remaining Fuel. Its five-object report passed the caller's publish, callback
count and unchanged-control checks. The consumer tests also established that a
throw after commit preserves one publication and exits with failure.

The packed-package JavaScript execution and TypeScript declaration checks
passed independently of the repository's source imports. Explicit Node
transport tests passed for pre-sent replies and a stalled child. After migrating
the Tauri frontend to this same package, the real macOS WebView acceptance
passed both successful execution and failure after Save. Linux coverage is
configured in CI; this local record does not claim a Linux run.
