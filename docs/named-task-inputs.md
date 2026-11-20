# Named String Inputs In Semantic Version 3

Semantic v3 allows one declared task to accept up to eight independently named
strings, such as an article's title and content. Values remain entry arguments;
they are never substituted into generated source.

This is an explicit opt-in. Semantic v1/v2, `defineRegisteredTask`, Rust
`prepare_task`, the original Tauri command and worker protocol v1 retain their
zero-or-one-string contracts and existing serialized output.

## Declaration And Selection

Use `SemanticDescriptionV3` / `SemanticTaskDescriptionV3`, or the optional
`defineRegisteredTaskV3` constructor. Validate the result against the real base
catalog with `defineSemanticCatalog`. A task declares, for example:

```ts
inputs: [
  { name: "title", scalar_type: "string" },
  { name: "content", scalar_type: "string" },
],
execution: "named_strings",
```

Each action and report predicate selects its input by name, such as
`{ kind: "input", name: "content" }`. All names must be supplied exactly once;
missing, extra, duplicate or non-string arguments are errors. A v3 task with
zero inputs uses `execution: "no_text"`. Result and verification contracts retain
v2's fixed and observed scalar results.

The complete [article declaration](../consumer/javascript/agent/multi-input-catalog.mjs)
offers two reads, publication and an unchanged-state check. Publication fills
two controls and clicks a third, using twelve Effects. Its predicates separately
verify saved title and content, require one publication and preserve an unrelated
control. The [application model](../consumer/javascript/agent/multi-input-application.mjs)
uses the public registered-application interface.

For the offline provider, v3 arguments use a strict JSON object:

```text
task publish_article:{"title":"Release notes","content":"The complete text."}
task read_title
```

Only v3 parses that object. In v1/v2, text after the colon stays literal text,
including JSON-looking text. The model provider selects the same declared names
and string values; it cannot supply source, execution modes or verification
claims. The reference tests use local provider fixtures, without a model service.

`prepareSemanticCatalog(raw).prepare(task, arguments)` marks every v3 plan with
`inputMode: "named_strings"`, including no-input tasks. The host rejects a
missing or mismatched marker. The argument type remains
`Readonly<Record<string, string>>`.

## Bind Several Verified Results

Workflow inputs can independently reference earlier verified results:

```ts
prepareSemanticWorkflow(rawCatalog, [
  { task: "read_title", arguments: {} },
  { task: "read_content", arguments: {} },
  { task: "publish_article", arguments: {
    title: { fromStep: 0, format: "string" },
    content: { fromStep: 1, format: "string" },
  } },
  { task: "check_article", arguments: {} },
]);
```

The complete plan is admitted first. Each binding is resolved only after its
source passes semantic and application verification, and resolved values are
checked again before execution. This reference workflow performs seventeen
Effects. Failed conditions, cancellation and verification failures still stop
later tasks without undoing earlier changes.

## Versioned Execution

The Node transport maps the v3 plan marker to worker protocol v2. Startup keeps
the source and named-string-map shape, but all outer request, reply and terminal
records carry `agent_worker_version: 2`. Version, scope, serial and challenge
must agree throughout the child process. Unsupported versions fail without
fallback. The inner Bridge protocol and execution-report schema remain version 1.

Rust hosts use `prepare_named_task(source, arguments)` with the existing
prepared-task driver. Compilation and complete binding precede application
requests. Tauri exposes `agent_start_named_task`, selected by the frontend from
the admitted marker. Its original form and board catalogs remain v2, preserving
their existing commands and conversation syntax.

The separate [Tauri article application](article-host.md) uses a native v3 catalog
and the same conversation entry for title/content publication. It supplies a real
WebView reference for this path while the independent Node model remains usable
without Tauri.

V3 source generation chooses bounded helper boundaries that minimize captured
parameters. This keeps the two-field example within the original host's sixteen
bindings; parser, helper, Fuel and VM limits remain authoritative. V1/v2 keep
their original source partitioning and golden bytes.

The independent Rust catalog agent accepts repeated `--arg NAME VALUE` for v3
named tasks. After the [Node setup](node-agent-host.md#build-and-run), run with a
fresh output directory:

```sh
cargo run --locked --offline --profile lean --target-dir target \
  --manifest-path consumer/Cargo.toml --example catalog_agent -- \
  --host consumer/javascript/agent/multi-input-cli.mjs \
  --task publish_article --arg title 'Release notes' --arg content 'Body text' \
  --output target/verification/article-v3 --execute
```

The CLI adapter accepts only source and arguments matching a declared task. It
creates a local application model, executes the actual Rust VM and checks fresh
application evidence before returning its report. The Rust Consumer then
independently verifies each named-input predicate. It does not launch a GUI.
Legacy tasks continue to use `--text`; mixing the conventions is rejected.

## Bounds And Failures

There are at most eight strings, each at most 4096 UTF-8 bytes, with valid Unicode
and no NUL. Values total at most 16 KiB. Existing identifier rules still reserve
`body` and `_lese_` prefixes. Message, JSON, source, worker-frame and helper limits
also apply; additional parameter slots do not enlarge those budgets. Boolean,
integer, optional-string and list inputs are outside this version.

Input admission precedes application attachment. Once execution starts, a later
field failure can leave an earlier edit committed. Cancellation or revocation
fences a late publication while retaining completed edits. The VM and host keep
Fuel, live grants and explicit `poll → authorize → dispatch → accept`; this
extension adds no scheduler, retry or rollback.
