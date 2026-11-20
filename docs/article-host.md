# A Multi-Input TypeScript GUI With The Same Agent

The Tauri article application exposes two independent fields, a publication
action and an unrelated counter through the existing registered-object bridge.
Its conversation uses the same `createAgentChat`, semantic provider, workflow
session and verifier as the form and task board. The application declares the
meaning of its operations and the evidence required for success.

This is an in-memory reference application. Publication copies the current title
and content into the local preview and increments its count. It does not send an
article to a website or persist a document. A fresh window starts with empty
fields and zero counts.

## Build And Launch

With the pinned dependencies installed, run from the repository root:

```sh
npm run build --prefix hosts/tauri
cargo build --manifest-path hosts/tauri/Cargo.toml --locked --offline --profile lean
target/lean/leselang-tauri-host --describe-agent-article
target/lean/leselang-tauri-host --agent-article
```

`--describe-agent-article` works without desktop features and never opens a GUI.
`--agent-article` requires the macOS desktop feature and a logged-in desktop.
Both flags are exclusive. The original form and task-board entries and catalogs
retain their existing behavior.

## Discover, Execute And Verify

The page discovers its four tasks from the native `tauri.article` v3 catalog.
The offline command examples shown in the conversation are generated from each
task's declared input names:

| Task command | Result | VM Effects on success |
| --- | --- | --- |
| `task read_title` | Observed string equal to the current title | 2 |
| `task read_content` | Observed string equal to the current content | 2 |
| `task publish_article:{"title":"Release notes","content":"Two independent fields."}` | Boolean true and independently verified publication | 12 |
| `task check_article` | None and unchanged complete inventory | 1 |

The argument object may list names in either order. Missing, extra, duplicate or
non-string values fail before application execution. The title and content are
bound as separate string entry arguments; neither is interpolated into source.
See [named task inputs](named-task-inputs.md) for bounds and version behavior.

The publication program finds and observes each field, checks its current role,
availability and revision, sets the corresponding value, observes it again,
then finds and observes the publication button before clicking and reobserving.
Independent report predicates require both current fields and both saved fields
to equal their respective input, the publication count to increase exactly once,
and the unrelated control to stay unchanged. An acknowledgement alone cannot
establish publication.

The application view reads the actual DOM and checks its consistency with the
model. It rejects detached or replaced instances and mismatched displayed
evidence. Manual and agent actions use the same application event handlers.
Publication crosses a Promise boundary and rechecks captured state before its
explicit commit. Cancellation, revocation and stale changes fence a late commit;
already-completed edits remain visible. No automatic retry or rollback occurs.

## Bind Both Verified Observations

The public workflow helper can bind two independent observed results into the
same later task:

```ts
const plan = prepareSemanticWorkflow(articleCatalog, [
  { task: "read_title", arguments: {} },
  { task: "read_content", arguments: {} },
  {
    task: "publish_article",
    arguments: {
      title: { fromStep: 0, format: "string" },
      content: { fromStep: 1, format: "string" },
    },
  },
  { task: "check_article", arguments: {} },
]);
```

This workflow uses seventeen Effects across four ordinary VM sessions. Each
source result must pass semantic and application verification before it supplies
an input. Resolved strings are checked against the target signature and bounds
again. Model mode can select this plan from the catalog using the existing
explicit result-binding option; it requires no article branch in the provider.
The offline command mode selects one declared task per submission.

For a provider-protocol check without credits, start the explicitly labelled
[`model-service.mjs`](../hosts/tauri/tests/model-service.mjs) loopback fixture,
configure its printed endpoint and model `protocol-workflow-fixture-not-a-model`
as described in the [model guide](model-provider.md), then launch the article
window. The two exact fixture messages are:

> 发布标题“文章协议回归 ✓”和正文“两个独立输入分别传入。”，再读取标题和正文并核对文章。

> 读取当前文章标题和正文，将两个已验证结果分别用于发布，再核对文章。

The first selects literal named values, then reads both and checks the inventory.
The second selects the two-result binding plan above. Both use seventeen Effects.
These fixed replies exercise the actual provider transport and VM; they do not
establish natural-language model inference. The production providers remain
independent of article controls and task names.

## Native Application Boundary

Native startup selects `AgentApplication::Article`, which pins
`agent-article.html`, `tauri.article` and the article catalog. The same
`agent_describe` request used by the form and board returns this host-selected
catalog. A page request cannot choose another application's identity.

Every v3 plan, including a zero-input read, uses the explicit
`agent_start_named_task` entry. Article rejects the original form runner and
legacy task route before admitting a session. The named command must appear in
the Tauri build manifest, generated command permission, main-page capability and
invoke handler. Regression checks keep these declarations aligned; a mocked
frontend invocation does not by itself prove the native permission works.

The task uses the existing Rust VM, live grants and 10,000-Fuel budget. Native
transport callbacks carry bounded messages while the owning worker explicitly
drives `poll → authorize → dispatch → accept`. The report remains
`tauri_task_run` schema version 1, with independent `native_before` and
`native_after` evidence.

## Sources And Checks

| Source | Responsibility |
| --- | --- |
| [`article_semantics.rs`](../hosts/tauri/src/article_semantics.rs) | V3 task declarations and independent report predicates |
| [`application.rs`](../hosts/tauri/src/application.rs) | Native application identity |
| [`article/`](../hosts/tauri/frontend/src/article/) | Application state, real DOM handlers and observations |
| [`agent-article-app.ts`](../hosts/tauri/frontend/src/agent-article-app.ts) | Article elements and common chat composition |
| [`shared fixtures`](../fixtures/host-semantics/README.md) | Native catalog and four programs compared across Rust and JavaScript |

Run the `javascript`, `consumer`, `tauri-portable` and `tauri-desktop` check groups
for these layers. The desktop group builds and tests native transport but does
not launch the GUI. Real-window procedures and dated evidence belong in the
[agent acceptance archive](acceptance/agents.md).
