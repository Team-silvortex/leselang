# A Second TypeScript GUI With The Same Agent

The Tauri task board is a second application using the existing JavaScript
agent session, providers, registered-object bridge and Rust VM. Its business
operation completes a task once and disables its button. It has no text input,
Save action or form-specific workflow. The original form remains available.

The reference board contains two in-memory demonstration tasks: `review`
(核对示例报告) and `archive` (整理示例资料). Completing a task changes its status
from `pending` to `done` and its completion count from zero to one; the other
task must remain unchanged. This does not perform the real-world work named in
the sample labels or persist a task database. A fresh application starts both
demonstration tasks as pending.

## Build And Launch

With the pinned dependencies installed, run from the repository root:

```sh
npm run build --prefix hosts/tauri
cargo build --manifest-path hosts/tauri/Cargo.toml --locked --offline --profile lean
target/lean/leselang-tauri-host --describe-agent-board
target/lean/leselang-tauri-host --agent-board
```

The description command works with `--no-default-features` and never opens a
GUI. The interactive command requires the macOS desktop feature and a logged-in
desktop. Both new flags are exclusive, just like the original `--agent` and
description flags. They do not change the original form catalogs or entry.

The page starts with the ordinary offline provider when no model service is
configured. Commands are derived from its actual semantic catalog:

| Task | Result | VM Effects on success |
| --- | --- | --- |
| `task read_review_status` | Observed `string`: `pending` or `done` | 2 |
| `task read_archive_status` | Observed `string`: `pending` or `done` | 2 |
| `task complete_review` | Fixed `string: "done"`; completes review once | 4 |
| `task complete_archive` | Fixed `string: "done"`; completes archive once | 4 |
| `task check_board` | `none`; verifies the board is unchanged | 1 |

Every task takes no input. An extra argument, an undeclared form task or an
invalid later step rejects a complete workflow before its first execution.
Completing an already completed task fails its generated program's guard before
another click. No implicit reset, retry or rollback occurs.

## Compose Observation And Completion

The same `createModelWorkflowProvider` used by the form can select this plan
from the board's catalog. There is no board branch in that provider or in the
workflow executor:

```ts
const plan = prepareSemanticWorkflow(boardCatalog, [
  { task: "read_review_status", arguments: {} },
  {
    task: "complete_review", arguments: {},
    requires: { fromStep: 0, format: "string", equals: "pending" },
  },
  { task: "read_review_status", arguments: {} },
  { task: "check_board", arguments: {} },
]);
```

On a pending review task this plan performs nine VM Effects. On a subsequent
submission, the first read returns `done`; the requirement stops the plan
before completion after only two Effects. A requirement uses the earlier
verified result, not an atomic state lock. Independently, the completion task
checks fresh status, availability, revision and count before dispatch, and the
application commit gate checks its model and DOM again after the Promise.

To exercise the model transport without credits, explicitly launch
[`tests/model-service.mjs`](../hosts/tauri/tests/model-service.mjs), configure its
printed loopback endpoint and model `protocol-workflow-fixture-not-a-model`,
then start `--agent-board`. The exact fixture request is:

> 读取报告任务状态，只有它还是 pending 才完成它，再读取状态并核对任务板。

`测试任务板引用表单任务` returns a plan whose later form task is undeclared;
the earlier archive completion must not run. `测试取消任务板规划` returns a
delayed plan for cancellation checks. This is a labelled deterministic protocol
fixture, not model inference. See [model configuration](model-provider.md) for
the actual service contract and transport bounds.

## Application Boundaries

Native startup selects `AgentApplication::Form` or `AgentApplication::TaskBoard`.
That host-owned value pins the local page, catalog and report profile. The board
uses `agent-board.html` and `tauri.taskboard`; the form retains `agent.html` and
`tauri.typescript`. A model or page request cannot select an alternative profile.
Navigation and IPC callers must match the selected page, and the board rejects
the old form-only `agent_start` runner.

The common chat entry calls the same `agent_describe` shape in both applications:
`{raw: true, taskCatalog: true, observedResults: true}`. The native host supplies
the selected application's declarations. Board requests for a legacy or
non-observed catalog are rejected, rather than returning a form catalog.

Rust embedders can use the additive
`run_prepared_task_for_application(transport, scope, task, application)` API.
It shares the existing prepared task, explicit VM drive and 10,000-Fuel budget.
`run_prepared_task` keeps its original form profile. Application identity does
not grant access: registered controls, live authority and revision checks remain
the transport and host's responsibility.

Both applications use the public SDK's
[`createRegisteredAgentHost`](registered-agent-host.md) for canonical source
comparison, captured initial state, exact execution report matching and fresh
observations; the local `createAgentHost` wrapper only maps Tauri transport calls.
The board only supplies an `attach` / `detach` / `observe` connection. Its
semantic declarations independently verify exact completion transitions and
unchanged unrelated controls. For reads and checks, the finite report vocabulary
checks field types, names, bounded counts and preservation; the TypeScript
model/view separately enforces the valid `pending/enabled/0` or `done/disabled/1`
state combination. Metadata alone does not establish that business invariant.

A completion may disable its own control inside `context.commit`. The bridge
requires actionability before invocation and before each mutation. Settlement
accepts that committed state change only while identity, revision, authority,
cancellation and deadline checks still pass. A separate change outside the
commit remains stale; a second mutation after disabling is refused. Failure
after a domain mutation retains the partial effect.

## Sources And Verification

| Source | Responsibility |
| --- | --- |
| [`board_semantics.rs`](../hosts/tauri/src/board_semantics.rs) | Five task declarations and report predicates |
| [`application.rs`](../hosts/tauri/src/application.rs) | Host-selected application identity |
| [`board/model.ts`](../hosts/tauri/frontend/src/board/model.ts), [`board/view.ts`](../hosts/tauri/frontend/src/board/view.ts) | Domain state, actual DOM handlers and registrations |
| [`agent/chat.ts`](../hosts/tauri/frontend/src/agent/chat.ts) | Shared conversation orchestration for form and board |
| [`shared fixtures`](../fixtures/host-semantics/README.md) | Board catalog and five generated programs |

Run `javascript`, `node-consumer`, `tauri-portable` and `tauri-desktop` check
groups as appropriate; see [testing requirements](testing.md). Dated desktop
results are recorded in the
[task-board acceptance record](acceptance/agents.md#second-typescript-application-task-board).
