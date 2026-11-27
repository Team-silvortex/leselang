# Agent Acceptance Records

These dated records retain their original application, platform, source and test
scope. Historical counts describe the named milestone, not the current suite.
Follow the linked source guides for current setup and contracts; see the
[acceptance index](README.md) for other components. Paths under `target/` identify
local ignored evidence and are not distributed artifacts. References within a
record to “this guide” or “above” retain the context of its linked source guide.

## Independent Catalog Agent

Source guide: [agent-workflow.md](../agent-workflow.md).

On 2026-11-13, the complete agent command was exercised against the real AppKit
host on a logged-in macOS desktop:

| Task input | Observed outcome |
| --- | --- |
| `保存 ✓` | The program returned `true` after nine Effects, with 433 Fuel remaining. Native input and Save values matched the input, Save's callback count became one, and `other` retained its public scalar state. |
| Text containing quotes, a backslash and a newline | The program returned `true` after nine Effects, with 417 Fuel remaining. The same native postconditions passed. Its generated source was byte-for-byte identical to the first run's source. |
| 128 ASCII `x` characters | The host trace showed text replacement and the Save click pumped and accepted before the final `gui.observe` failed for insufficient Fuel. The agent exited with code 1 and recorded `status: "failed"` and `possible_partial_effects: true`. It produced no success report and did not retry. |

The first generated task also ran through the host's default text-output mode:
both that mode and JSON reporting returned `true` after nine Effects with the
same 433 Fuel remaining. The native AppKit conformance checks also passed.

The third run demonstrates a real partially completed task: accepted input can
fit the text-size bound while exhausting the original Fuel after native work.
The failure outcome retains that distinction instead of inferring that no
action occurred from the missing final result. These are macOS runtime results;
Linux coverage for this workflow remains configured CI checks, not a claim of
a local Linux or GUI run.

## Same Agent Across Tauri And Calculator

Source guide: [agent-workflow.md](../agent-workflow.md).

On 2026-11-13, the same built `target/lean/examples/catalog_agent` executable
completed these three invocations on the macOS desktop, using each host's
actual offline catalog and newly generated source:

| Application and initial state | Observed outcome |
| --- | --- |
| Tauri form, text `统一 Lese ✓` | `true`, nine Effects, 9,421 Fuel remaining. Input and saved value matched, Save ran exactly once, and `other` was unchanged. |
| Calculator displaying `8 + 7 = 15` | `initial display mismatch`, two read Effects, 1,940 Fuel remaining, no presses. Both native snapshots remained `15` / `8 + 7`; the host and agent exited with code 1. |
| Calculator after an explicitly authorized clear to `0` | `"3"`, ten Effects, 1,646 Fuel remaining. Both the native report and independent desktop inspection showed display `3` with last expression `1 + 2`. |

The operator restored Calculator's original `8 + 7 = 15` display from its
history and closed the sidebar after verification. The test's new `1 + 2`
history entry remains; the agent itself performs neither reset nor restoration.
The failed initial-state run records `possible_partial_effects: true` because
the generic subprocess boundary conservatively marks any failed execution;
its report and trace establish that this particular program made no presses.

Local evidence is retained under the ignored build directory in
`target/cross-gui-tauri-20261113`,
`target/cross-gui-calculator-initial-20261113` and
`target/cross-gui-calculator-20261113`. Each includes the actual catalog,
generated source, plan, execution report, trace and independent verification.
These runs demonstrate one discovery/execution/verification pipeline with two
different GUI adapters and known task semantics; they do not establish
automatic understanding of arbitrary applications.

## Catalog-Declared Tasks

Source guide: [agent-workflow.md](../agent-workflow.md).

On 2026-11-13, the generic semantic planner was exercised against the real
AppKit and Tauri hosts. Both plans recorded
`planning_contract: "catalog_semantics_v1"`; their generated LeseLang source
was byte-for-byte identical. The application-specific report kind, input
convention, Fuel ceiling and predicates came from each actual catalog.

| Desktop run | Outcome |
| --- | --- |
| AppKit, text `语义 ✓` | Nine Effects, 448 of the original 1,000 Fuel remaining, and all 36 declared report predicates passed. |
| Tauri, the same text | Nine Effects, 9,456 of the original 10,000 Fuel remaining, and all 36 declared report predicates passed. |
| AppKit, 128 ASCII `x` characters | The trace showed text replacement and Save submitted, pumped and accepted before the final observation exhausted Fuel. The agent exited 1, retained `possible_partial_effects: true` and did not retry. |

Evidence is retained locally in `target/semantic-appkit-20261113`,
`target/semantic-tauri-20261113` and `target/semantic-appkit-fuel-20261113`.
Calculator was closed during this acceptance run and was not reopened. Its
new semantic source ran through the actual VM and Calculator driver with a
fake backend: ten Effects and four presses on success, two read Effects and
zero presses for a nonzero initial state, plus wrong-expression, revocation
and uncertain-action failures that preserve already completed presses.

Portable tests also change the profile, task, operation names, control names
and input name to a synthetic notes application and drive the same subprocess
pipeline without a planner branch. A separate unfamiliar scalar host executes
a generated two-Effect task through the real VM and returns `42`. These tests
establish declaration-driven integration; the synthetic app fixtures are not
additional live GUI applications.

## Application Conversation Entry

Source guide: [agent-entry.md](../agent-entry.md).

See the two conversation-entry records below.

## Generic Task Entry

Source guide: [agent-entry.md](../agent-entry.md).

On 2026-11-13 the rebuilt macOS WebView displayed all four declarations from
the new task catalog. The same conversation entry then generated, executed and
verified these workflows in sequence:

| Submission | Observed outcome |
| --- | --- |
| `task inspect_controls` | Verified `integer: 3`; no Save or Other action |
| `task save_message:命名参数 "message" \\ 路径` plus a newline and `通用返回值 ✓` | Verified `string: "saved"`; literal input and saved content matched; Save count became one |
| `task check_controls` | Verified `none`; saved content and all counts stayed unchanged |
| `保存：兼容入口 ✓` | Verified `boolean: true`; Save count became two |
| `task inspect_controls:不应接受参数` | Rejected before execution: the task takes no input |
| `task save_message` | Rejected before execution: the required input was missing |

The final input and saved value were `兼容入口 ✓`, Save count was two and
Other remained zero. The Accessibility tree showed all four verified typed
results and both admission failures; the page was also inspected visually.
The worker trace recorded 24 VM Effects across the four executed tasks
(3 + 9 + 3 + 9). The two rejected submissions added no Effects. Each workflow
used the same shared provider and an independently described host contract.

The automated Rust tests additionally execute all six scalar result types,
including exact `u64::MAX`, through the real VM; test signature admission before
any transport request; and compare legacy/new Fuel and Effect counts for the
same program. Cancellation, late replies and partial mutations are covered by
the transport and frontend suites, not by manually timed desktop cancellation.

## Original Semantic Provider Entry

Source guide: [agent-entry.md](../agent-entry.md).

On 2026-11-13 the rebuilt macOS `--agent` WebView was exercised through its
conversation UI with the shared semantic provider:

1. The available task list displayed `fill_and_save` and its actual
   host-declared summary before a workflow was submitted.
2. `task fill_and_save:目录驱动的工作流 ✓` produced a verified result and one Save.
3. `保存：第二轮 "引号" \\ 路径` followed by a newline and `语义目录 ✓` produced
   another verified result through the application alias. The input and saved
   content matched exactly, the accumulated Save count became two, and Other
   remained at zero.
4. `task missing_task:不应执行` was rejected because the host had not declared
   that task. The conversation stated that application operations had not
   started; the saved value and counts remained unchanged.

These results were observed in the native Accessibility tree of
`tauri://localhost/agent.html`; the loaded page was also inspected visually.
Cancellation and late-completion coverage comes from SDK, frontend connection
and Rust registry tests. This desktop record does not claim a manually timed
cancellation or a connection to a live model service.

The same public provider also passed the independent Node Consumer's
`publish_article` workflow against the real Rust VM and five application-owned
objects. Its tests include a mutation followed by failure, and a successful VM
report rejected by a stronger declared application postcondition. This is
separate-process application evidence, not another desktop GUI acceptance.

## Model Protocol Transport

Source guide: [model-provider.md](../model-provider.md).

On 2026-11-13, the single-task version of the bundled Tauri agent page was driven
through the first six fixture submissions above, using
`protocol-fixture-not-a-model`. The native HTTP transport contacted the loopback service once
per submission. Save returned verified `string: "saved"`; inspection returned
verified `integer: 3`. Refusal, undeclared selection, cancellation during planning
and HTTP 429 all reported that application operations had not started. The
cancelled request's connection closed before its delayed response completed.

After those requests, an explicit switch to offline mode ran
`task check_controls` and returned verified `none`. The application retained
`模型接入回归 ✓` in both input and saved output, one Save, and zero Other-control
clicks. Native tracing contained exactly 15 authorized/accepted Effects: nine
for Save, three for model-selected inspection and three for the offline check.
There were six model HTTP requests, with no retry after rate limiting and no
model request for the offline command. This records real WebView/VM execution
against predetermined model-protocol replies, not live model inference.

## Sequential Workflows

Source guide: [agent-workflows.md](../agent-workflows.md).

On 2026-11-20, the actual bundled agent WebView used the explicitly labelled
workflow protocol fixture. One HTTP planning request produced the three-step
inspection/save/check sequence. The conversation showed three verified results
in order: `integer: 3`, `string: "saved"`, and `none`. Both the input and saved
value contained `多步骤工作流 ✓`, with one Save and zero Other-control clicks.

The invalid-second-step fixture was rejected before execution, retaining that
same application state. A delayed planning request was cancelled; the fixture
observed its connection close without completing the response. A subsequent
model-selected inspection succeeded, followed by an explicit switch to offline
mode and a successful `task check_controls` command.

The service recorded four model requests in total. Native tracing recorded
exactly 21 authorized and accepted Effects: 15 for the initial three steps,
three for the subsequent inspection, and three for the offline check. Invalid
planning and cancellation added no application Effects, and offline execution
made no model request. Mid-execution failure and cancellation between completed
steps are covered separately by SDK and application-host integration tests;
the recorded desktop run did not inject those faults. This is real GUI/VM
acceptance with predetermined planning replies, not a live-model inference test.

## Verified Result Bindings

Source guide: [agent-workflows.md](../agent-workflows.md).

On 2026-11-20, a fresh actual macOS agent WebView submitted
`先检查控件数量，把这个数量保存为文本，再核对控件。` to the explicitly
labelled workflow protocol fixture. The first step verified an `integer: 3`
result. The second step resolved that accepted result to the string argument
`"3"` and saved it; the third step verified the controls. The conversation
showed all three steps as verified and marked the save as using the first
step's verified result. Both the input and saved value were `3`, with one Save
and zero Other-control clicks.

The integer-to-string format mismatch and forward-reference submissions were
then both rejected before application execution. The saved value and counters
remained unchanged. After explicitly switching to offline mode,
`task check_controls` also verified successfully without another model request.

The fixture recorded exactly three HTTP planning requests: the successful
binding plan and the two rejected plans. Native tracing recorded exactly 18
authorized and accepted Effects: 15 for the successful three-step workflow and
three for the offline check. The rejected plans added no Effects. Automated
application-host integration separately asserts the concrete `message: "3"`
input passed to execution, while the aggregate plan retains its empty template
argument and explicit result binding.

This check exercised the actual GUI, native transport and Rust VM with
predetermined planning replies. It made no paid or external model call and does
not establish live-model interpretation quality. Lossless full-range integer
conversion, cancellation during source verification, failure with earlier
effects, and malformed binding graphs are covered by the SDK tests.

## Dynamic Observation Results

Source guide: [agent-workflows.md](../agent-workflows.md).

On 2026-11-20, a fresh actual macOS agent WebView explicitly selected the v2
catalog and the labelled loopback workflow protocol fixture. The same request,
`读取当前输入框的文本，原样保存，再核对控件。`, ran twice. Only the application
input was changed between submissions:

1. The first input contained `现场读取甲`, quotes, a check mark, a backslash and
   two lines. `read_message` returned that actual text, the result binding passed
   it to `save_message`, and `check_controls` verified the final state. All three
   steps verified; the saved text retained those characters and the line break.
2. The second input was `现场读取乙：{"fromStep":7,"format":"string"}`. The same
   three-step selection read and saved this new text verbatim. The JSON-looking
   content remained data and did not become a workflow reference.

The conversation displayed each actual read result and the save's reference to
that verified result. The final state contained the second text, two Saves and
zero Other-control clicks. The two runs used the same catalog and canonical
programs; matching SDK and host-integration tests also assert this invariant
and inspect the actual resolved `message` input.

`测试动态观察结果类型错误` was then rejected before execution, with no change
to the saved content or counters. After an explicit switch to offline mode,
`task read_message` independently verified the second text without saving again
or making a model request.

The service recorded three planning requests. Native tracing recorded exactly
30 authorized and accepted Effects: 14 for each read/save/check sequence and
two for the final offline read. The rejected plan added no Effects. The local
fixture was stopped after verification, with the application left in offline
mode. These are real WebView, transport and VM checks using predetermined
planning replies; no paid or external model inference was used. Forged or
ambiguous observation evidence, current-state mismatch and cancelled late reads
are covered separately by automated tests.

## Reorganized Module Smoke Check

On 2026-11-20, after the SDK and Tauri frontend modules were moved into their
responsibility-based directories and the generated assets were rebuilt from
clean output directories, a fresh actual macOS agent WebView loaded the five
declared tasks in explicit offline mode. No model service was configured.

With `结构整理回归 ✓` entered into the application, `task read_message` verified
that exact current text. A separate `task save_message:结构整理回归 ✓` submission
then verified `string: "saved"`. The visible saved value matched the input,
Save count was one, and Other-control clicks remained zero. Native tracing
recorded 11 authorized and accepted Effects: two for reading and nine for saving.
This checks the rebuilt conversation page's module loading, adapter connection,
observation, mutation and verification after the structural move. It does not
add a new language behavior or establish online model inference.

## Verified Result Requirements

Source guide: [sequential workflows](../agent-workflows.md#require-a-verified-result-before-continuing).

On 2026-11-20, a rebuilt actual macOS agent WebView used the explicitly named
`protocol-workflow-fixture-not-a-model` loopback service to select one three-step
workflow: `read_message`, `save_message` bound to the read result and requiring
that result to equal `条件执行回归 ✓`, then `check_controls`. The same instruction
and selected plan were exercised twice with different current application text:

| Current input / request | Observed outcome |
| --- | --- |
| `条件执行回归 ✓` | All three steps verified. The condition displayed the matching expected and actual text, Save ran once, saved text matched, and Other remained untouched. Native tracing recorded 14 dispatched and accepted Effects. |
| `当前文本不满足条件 ✓`, same request | The read verified with that actual text. The condition displayed a mismatch and the target Save was not dispatched; the final check remained unstarted. Saved text stayed `条件执行回归 ✓`, Save count stayed one and Other count stayed zero. Only the two read Effects were dispatched and accepted. |
| `测试条件结果类型错误` | A requirement with an incompatible declared result format rejected the complete plan before execution and added no Effects. |

The operator then explicitly selected offline mode and submitted
`task read_message`. It verified `当前文本不满足条件 ✓` without saving or making a
planning request. Across these checks, tracing recorded 18 authorized and
accepted Effects and exactly one `gui.click`; the local service recorded three
planning requests. The fixture was stopped, leaving the application in offline
mode with Save count one and Other count zero.

The failed workflow conservatively retains `possiblePartialEffects` after the
earlier read entered execution; its detailed trace establishes that no mutation
was dispatched in that submission. The comparison is against an earlier
verified observation, not an atomic read-and-write or a lock on current state.
These are real WebView, transport and VM checks using predetermined planning
replies, not evidence of external or paid model inference. Local logs are under
`target/verification/requirements/`; v1 compatibility, exact integers, forged
evidence, cancellation, late replies and catalog changes are covered by the
automated regression suite.

## Second TypeScript Application: Task Board

Source guide: [task board host](../taskboard-host.md).

On 2026-11-20, two rebuilt macOS Tauri applications used the same conversation
entry, agent providers, semantic source generation, VM execution and result
verification. The original form retained its five tasks and `tauri.typescript`
profile. The second application loaded five task-board declarations under
`tauri.taskboard`, with two actual DOM buttons backed by an independent
TypeScript model. Both demonstration tasks initially had status `pending`,
completion count zero and an enabled button.

The task board used the explicitly labelled loopback service
`protocol-workflow-fixture-not-a-model` for these checks:

| Submission | Actual result | Accepted VM Effects |
| --- | --- | --- |
| `读取报告任务状态，只有它还是 pending 才完成它，再读取状态并核对任务板。` | All four steps verified: read `pending`, conditionally complete review, read `done`, check the board. Review count became one and its button disabled; archive stayed pending with count zero. | 9 |
| Identical request again | Read `done` verified; the `pending` requirement stopped the completion and both following steps. Neither task changed. | 2 |
| `测试任务板引用表单任务` | The complete plan was rejected because its later `save_message` task was undeclared. Its earlier archive completion did not start. | 0 |
| `测试取消任务板规划`, then Cancel during planning | Cancellation completed before application execution. The fixture recorded the delayed connection closing without a completed reply; archive remained pending. | 0 |

After explicitly selecting offline mode, the same window exercised:

| Submission | Actual result | Accepted VM Effects |
| --- | --- | --- |
| `task read_archive_status` | Verified `string: "pending"`; neither task changed. | 2 |
| `task complete_archive` | Verified `string: "done"`; archive count became one and its button disabled, while review stayed done with count one. | 4 |
| `task complete_review` | The generated program's precondition guard returned its failure result before another click. Report verification failed, preserving both tasks. | 2 |
| `task check_board` | Verified `none`; both tasks remained done with count one and disabled buttons. | 1 |

Native tracing recorded exactly 20 authorized and accepted Effects, including
two `gui.click` operations. The service recorded four planning requests; offline
commands made none. Successful completion across each Promise retained the
control's own committed disabled state. The repeated direct completion was
rejected even without a workflow requirement. No retry or rollback occurred.

A fresh original form window, with no model endpoint configured, then ran
`task read_message` and verified the empty current input. Its separate
`task save_message:共用对话回归 ✓` submission verified `string: "saved"` and
displayed that exact saved text, Save count one and Other-control count zero.
The form trace recorded 11 authorized and accepted Effects, including one
`gui.click`. Both windows were left in offline mode and the local fixture was
stopped after verification.

All five relevant check groups passed: JavaScript SDK 139 and frontend 60 tests;
portable Tauri 36; desktop Tauri 68; independent Rust Consumer and catalog agent
837; independent Node Consumer 8 plus six Rust carrier checks. Required format
and strict Clippy checks passed. Fourteen existing catalog/source fixtures
retained their SHA-256 hashes; six new board fixtures cover its catalog and all
five generated programs. Automated tests additionally exercise authority
revocation, cancellation, deadlines, late completion, DOM replacement, stale
state and failures after a committed side effect. Local evidence is under
`target/verification/taskboard/`.

This establishes a second real WebView application using the common agent
contract. Planning replies were predetermined protocol fixtures, not external
model inference. The board changes in-memory demonstration status; it does not
perform the real-world work named by its labels or persist a task database.

## Public Registered Application Host

Source guide: [registered application host](../registered-agent-host.md).

On 2026-11-20, the common application admission, Bridge lifetime and independent
observation verifier moved from the Tauri example into the public
`createRegisteredAgentHost` SDK API. Both reference applications used that public
entry through a thin Tauri transport adapter. The original `AgentHost`, `Bridge`,
catalogs, compiler and explicit VM driver remained available.

The packed-package test installed the actual tarball into a separate temporary
Consumer, imported only `@leselang/bridge`, checked its TypeScript declarations
and drove two application-owned Promise actions. Its endpoint, profile, report
kind and `objects_before` / `objects_after` fields were independently configured.
Both actions disabled their own buttons and verified once with independent
before/current observations. This was a scripted in-memory transport using the
real Bridge; it did not run a language VM or use Tauri. The existing Node
Consumer retained its separate real VM carrier and passed its regression suite.

Fresh actual macOS WebViews then exercised the rebuilt assets and Rust VM:

| Application / submission | Actual result | Accepted VM Effects |
| --- | --- | --- |
| Task board: `读取报告任务状态，只有它还是 pending 才完成它，再读取状态并核对任务板。` | Four steps verified: read pending, complete review, read done, check board. Review count became one and its button disabled. Archive remained pending with count zero. | 9 |
| Task board: identical request again | Read done verified; the pending requirement stopped all remaining steps without a second click. | 2 |
| Task board: `测试取消任务板规划`, then Cancel during planning | Cancellation completed before execution. The delayed fixture connection closed without a completed reply; neither task changed. | 0 |
| Task board: explicit offline `task check_board` | Verified none and preserved review done/one and archive pending/zero. | 1 |
| Original form: offline `task read_message` | Verified the actual empty input. | 2 |
| Original form: offline `task save_message:公共宿主接口回归 ✓` | Verified saved, displayed the exact text, Save count one and Other count zero. | 9 |

Tracing recorded 12 authorized and accepted Effects for the board and 11 for
the form, with one `gui.click` in each application. The local service recorded
three planning requests, including the cancelled connection. Both windows were
left in offline mode and the fixture was stopped. Predetermined planning replies
establish transport integration here, not external model inference or
natural-language interpretation quality.

The complete checks passed: SDK 156 tests, frontend 60, portable Tauri 36,
desktop Tauri 68, independent Rust Consumer/catalog agent 837, independent Node
Consumer 8 and Rust carrier 6, plus required format and strict Clippy checks.
The 17 new public-host tests cover configuration admission, application callback
reentry, partial attachment, cancellation while transport remains unsettled,
late reply isolation, cleanup failures, exact report ownership and single-use
evidence. All 20 existing semantic catalog/source fixtures retained their
SHA-256 hashes.

Local evidence is under `target/verification/registered-host/`. The initial
direct `node --test` invocation reached the packed Consumer's TypeScript check
without npm's compiler PATH and failed to launch `tsc`; its log is retained.
The prescribed `javascript` check group supplied that environment and passed
the full suite. No new Rust carrier, background scheduler, retry or rollback
was introduced by this SDK extraction.

## Node-Owned Agent With Real VM Workers

Source guide: [Node-owned agent](../node-agent-host.md).

On 2026-11-20, an independent Node application used the public
`createRegisteredAgentHost` with a real Rust `agent_worker` child for every task.
JavaScript retained its five-object application model across workers; each child
used the original compiler, registered-object gateway, explicit VM driver and
10,000-Fuel budget. This acceptance was headless on macOS, with no Tauri runtime,
GUI interaction, external publication service or model inference.

The application's catalog was composed from the actual worker's offline base
description. Its four declared tasks were admitted through the public SDK and
compiled by the real worker: `read_draft`, `publish_article`, `read_publication`
and `check_publication`. The Node carrier preserved raw typed reports with fixed
`javascript.registered` / `javascript_task_run` identity and only returned them
after complete framing and successful child closure.

| Actual CLI run | Observed result |
| --- | --- |
| `npm run agent-demo --prefix consumer/javascript -- --text 'Node 独立宿主验收 ✓'` | Exit zero; all four steps verified. The actual read result satisfied the requirement and supplied the publication argument. Results were string, Boolean, string and none, with 2 / 9 / 2 / 1 Effects. Published text matched, publication count was one, Reset and Unrelated counts stayed zero. |
| Same entry with `--text '保留已发生的发布 ✓' --fault-after-commit` | Exit one; the read verified, publication failed after its actual commit, and subsequent steps did not start. The output retained the exact published text and count one with `possiblePartialEffects: true`. No retry or rollback occurred. |

Native tracing recorded 14 dispatched and accepted Effects with one click for
the successful run. The failure run dispatched ten Effects and accepted nine;
its one click reached the application mutation but could not produce an accepted
successful reply. The final application snapshot independently established that
publication survived the failure.

The real-worker integration suite also exercised a later publication and read
in the same Node model, observing the publication count increase from one to two
across fresh workers. A mismatched observed-result requirement stopped before
publication with only the two read Effects. Cancelling during a delayed
publication retained the earlier draft write, prevented the late publish commit
and allowed a separately requested read after worker settlement. A successful
VM report was refused when the independent current observation differed.
The actual worker refused invalid source and arguments before application
requests, preserved the exact `18446744073709551615` result bytes, and terminated
both missing-start and missing-reply cases within its own five-second deadlines.

Separate synthetic-worker fixtures checked strict JSON/UTF-8, duplicate keys,
oversized and truncated records, non-string challenges, foreign scope/serial,
reports followed by nonzero exit or extra output, terminal reports followed by
a hang, cancellation of a SIGTERM-resistant process and late old-scope delivery.
These fixture checks do not substitute for the real VM integration above.

The complete Node group passed 31 JavaScript tests: ten new real-worker
integration tests, thirteen process fixtures and eight retained original
Consumer tests. Its Rust checks passed five new worker tests and all six old
carrier tests. The other checks passed SDK 156, frontend 60, portable Tauri 36,
desktop Tauri 68 and independent Rust Consumer/catalog agent 837, including
required formatting and strict Clippy. A Rust compatibility assertion compared
the new registered task report with the old form report after changing only
identity and observation-field names; result, Effect and Fuel content matched.
All 20 existing semantic catalog/source fixtures retained their hashes.

Local evidence is under `target/verification/node-agent/`. The first integration
run exposed a test expectation that omitted the workflow's recorded target
refusal; the assertion was corrected to require an unexecuted target with no
report or partial effects, preserving the existing workflow contract. That log
and the later passing runs are retained. The publication example remains an
in-memory application, and the new process carrier remains a repository
reference, without restart recovery or a remote/durable protocol claim.

## Public Node Transport And Installed Consumer

Source guide: [Node-owned agent](../node-agent-host.md#compose-an-application-entry).

On 2026-11-20, the Node carrier moved from the reference application into the
optional public `@leselang/bridge/node` entry, with TypeScript declarations for
`createNodeAgentTransport` and `NodeAgentTransportOptions`. The reference
application now imports that entry. The root browser entry retained its existing
exports and has no transitive Node runtime or declaration imports. Tauri's
generated WebView assets contain the unchanged browser SDK module tree and
exclude the Node subtree.

An external Consumer packed the SDK, installed its tarball offline into a fresh
temporary directory with an empty npm cache, and copied only application-owned
model, semantic catalog and composition files. It selected the real Rust worker
by an explicit absolute path. Neither source imports, a linked SDK installation
nor a copied carrier supplied execution. The installed package had no runtime
npm dependencies; strict NodeNext TypeScript compilation succeeded without
installed Node typings.

That installed Consumer completed the four-step workflow through real Rust VMs:
read, conditional publication with a verified-result binding, read publication,
then check the inventory. The steps used 2 / 9 / 2 / 1 Effects, with 14 native
dispatches and accepts, one publication, preserved unrelated state and the
expected string / Boolean / string / none results. It separately exercised a
post-commit failure that retained one publication without retry, then verified
a fresh read. Cancellation during a delayed publication retained the earlier
draft write, refused the late publication commit and allowed a later verified
read after settlement. All runs were headless on macOS with Node 24.6.0; no GUI
or online model inference was performed for this packaging milestone.

The public carrier's 15 fixture tests retain malformed-frame, terminal/exit,
deadline, cancellation, old-scope and exact-integer coverage. Added configuration
checks reject invalid executable paths and arguments before spawn, and prove
that later mutation of a caller's argument array cannot change the captured
launch configuration. Full results passed SDK 172, Tauri frontend 61, Node
Consumer 19, both Rust carriers 11, and independent Rust Consumer/catalog agent
837, including the Consumer's formatting and strict Clippy checks. The Node
group also generated its offline Tauri plan successfully. All 20 existing
semantic catalog/source fixture hashes remained unchanged.

Local evidence is under `target/verification/node-sdk/`. This extraction adds no
scheduler, automatic retry, rollback or changed Fuel policy. The npm package is
still private, and the portable Rust worker remains an explicitly built and
configured reference executable rather than a binary distributed by the SDK.

## Standalone Registered Host And Installed Worker

Source guides: [registered-object core](../../hosts/registered/README.md) and
[Node worker build/install](../node-agent-host.md#build-and-run).

On 2026-11-20, `hosts/registered` became an independent Cargo workspace containing
the `leselang-registered-host` library and `leselang-agent-worker` binary. The
shared catalog, Gateway, Host, task admission and VM driver moved out of Tauri.
The new workspace's full `--all-features` resolved dependency graph and lockfile
contain no Tauri or WebView framework. Tauri depends on this core, re-exports
the same Rust types and retains its application identities, semantics and report
wrappers. The historical `agent_worker` example delegates to the same worker.

The new binary built independently and installed successfully with `cargo install
--path hosts/registered --bin leselang-agent-worker --locked --offline --profile
lean` into a dedicated local prefix. The Node application then selected that
installed executable by absolute path and ran the complete offline publication
workflow. A report and trace captured from the original worker before the
extraction were compared with the installed standalone worker afterwards:

| Step | VM Effects | Remaining Fuel, before and after |
| --- | --- | --- |
| Read draft | 2 | 9928 |
| Conditional publication from the verified read | 9 | 9208 |
| Read published value | 2 | 9926 |
| Check complete inventory | 1 | 9959 |

All four full reports, native traces and final application snapshots matched.
The literal text was `Standalone worker extraction ✓`; publication count was
one, and reset/unrelated counts were zero. The packed external JS Consumer also
ran through the new standalone binary, retaining its real-VM success,
post-commit failure, cancellation and late-commit checks. New and compatibility
executables emitted byte-identical base catalog descriptions.

A separate Rust Consumer workspace imported only the new library's public APIs
and executed two `gui.list` Effects with 9995 Fuel remaining. It distinguished
the initial inventory request from the two VM Effects and refused further
requests after transport authority closed. Compatibility tests passed prepared
tasks, scopes and transport implementations across old and new Rust paths and
required identical execution reports.

The shared checks passed seven standalone library/worker tests plus that
independent Rust program, SDK 172, frontend 61, Node Consumer 20, retained
Rust-launched carrier six, portable Tauri 36, desktop Tauri 68 and complete
independent Rust Consumer/catalog agent 837. Required formatting and strict
Clippy checks passed. All 20 existing semantic catalog/source fixtures retained
their hashes. Desktop tests exercised the existing local transport/model
fixtures without launching a GUI.

Local evidence is under `target/verification/standalone-worker/`, including the
before/after report and trace pairs and local installation log. These were
headless macOS runs with Node 24.6.0 and the pinned Rust 1.98.0 toolchain. No
desktop workflow or online model inference was performed. The worker remains a
locally built/installed binary; this milestone does not publish a crate or npm
package, bundle prebuilt runtimes or introduce automatic downloads.

## Typed Semantic Authoring And Exact Catalog Composition

Source guide: [JavaScript/TypeScript semantic authoring](../semantic-authoring.md).

On 2026-11-20, `@leselang/bridge` added `defineSemanticCatalog` and readonly
declaration types for the existing semantic v1/v2 vocabulary. The helper owns
the native base and application declaration, refuses replacement of an existing
semantic catalog, validates their complete relationship, then returns bounded
exact JSON. The ordinary catalog/report object readers retain their prior
contract; only authored declarations additionally accept exact unsigned bigint.

The Node reference now shares a typed publication declaration between its legacy
v1 entry and its directly declared four-task v2 catalog. Both complete raw
catalogs, all five generated programs, inputs, predicate lists and expected
Effect counts matched captures taken before the migration byte for byte.
The reference author modules passed strict TypeScript checkJs using public
types, including from the external installed SDK tarball without Node typings.

The installed Consumer completed the real-worker four-step, 14-Effect workflow
and retained the existing post-commit failure, cancellation, late-commit refusal
and fresh-read acceptance. A separate authored task used the bigint literal
`18446744073709551615`, generated a LeseLang program, executed one actual
`gui.list` Effect in the standalone Rust VM and verified the original exact u64
result bytes. Changing the reported integer by one failed verification.

The guide's complete TypeScript example was extracted, type-checked and executed
against the real worker. It read `Guide authoring ✓` with two Effects, verified
the independent observed result and preserved the complete application state.
Neither this guide run nor the Consumer used an online model or opened a GUI.

All checks passed: SDK 185 (including 13 authoring boundary tests), frontend 61,
Node Consumer 21, retained native carrier six, and complete independent Rust
Consumer/catalog agent 837. The Node group also generated the offline Tauri
plan. Authoring tests cover invalid references/types/versions, duplicate keys,
nonexecuted accessors/toJSON, owned data, exact integers and combined byte/node
bounds. Packed TypeScript checks refuse observed results in v1 and mutation of
readonly declarations. All 20 existing semantic catalog/source fixture hashes
remained unchanged; no protocol version, scheduler or execution policy changed.

Local evidence is under `target/verification/semantic-authoring/`. The first
capture comparison mixed live null-prototype argument records with plain
JSON-loaded records and reported prototype differences. Comparing both captures
in the same serialized form established exact parity; the initial explanation
and both captures are retained.

## Public Registered Application Connections

Source guide: [registered application host](../registered-agent-host.md).

On 2026-11-20, `@leselang/bridge` added `createRegisteredApplication` and public
control, action-context and observation types. A fixed declaration map now
supplies exclusive attachment, cleanup and fresh application observations.
The Node publication application and both Tauri form/task-board views use this
same helper. Models, real DOM handlers, instance/readback checks and explicit
mutation commits remain application-owned. The lower-level Bridge and custom
connection interfaces remain available.

Dependency notifications from agent handlers run inside the captured action's
synchronous commit. Manual application events notify the currently attached
connection separately. A cancelled or detached action cannot notify a replacement
session. Tests cover declaration/data ownership, partial attachment, reader
reentry, invisible dependency changes and ABA, self-disabling actions, revoked
authority, late commits, asynchronous/reentrant commit refusal and retained
partial effects without retries or rollback.

Review also reproduced and fixed Bridge callback-boundary defects: a reader
could revoke authority before a mutation without a second check, a reentrant
replacement could overwrite an inner replacement using its generation, and an
observation could return the obsolete instance after its reader replaced or
unregistered it. Authority and object identity are now checked after application
readers return. Regression cases require zero mutation after pre-commit
revocation and reject obsolete targets/snapshots. Form saves additionally
recheck the input and both buttons after their asynchronous preparation.

The real Rust-worker four-task workflow still completed 14 Effects with Fuel
remaining `[9927, 9203, 9925, 9959]` for `Registered application parity ✓`.
Its complete CLI result, including final model and per-task results, was byte
identical before and after the migration. Installed-package Consumers exercised
the helper through public exports, including real worker success, partial
failure, cancellation and subsequent fresh reads. The application adapter now
also passes strict TypeScript `checkJs` locally and from the installed tarball.

Checks passed SDK 209 (including 24 new boundary tests), frontend 68, Node
Consumer 21, retained native carrier six and the complete independent Rust
Consumer/catalog agent 837 with formatting and strict Clippy. The Tauri desktop
build, its 68 native tests and strict Clippy also passed with the updated frontend
assets. All 20 existing
semantic catalog/source fixture hashes remained unchanged. The guide's two
complete TypeScript blocks compiled; its control example ran through the real
Bridge, disabled its completed button and verified a manual reset. These were
offline, headless checks; no GUI was launched and no model service was called.

Local evidence is under `target/verification/registered-application/`. The first
frontend run exposed a variable-shadowing error in a newly added test fixture;
after correction all 68 frontend tests passed in `frontend.log`. The initial
combined log is retained alongside the final results and parity captures.

## Registered GUI Task Construction

Source guide: [registered GUI tasks](../registered-tasks.md).

On 2026-11-20, the public SDK added `defineRegisteredTask` and readonly definition
and step types. `read_control`, `set_control_text` and `click_control` expand to
ordinary find/observe/action calls and guards. Tasks can mix these steps with
existing raw calls and guards. The application explicitly supplies the result,
guard failure value and independent verification predicates; the constructor
does not infer business success from an acknowledgement or button title.

Owned declarations are deeply frozen. Internal bindings avoid all supplied
names and references, including undeclared and forward references, so expansion
cannot accidentally make an invalid reference valid. Expanded tasks retain the
21-step and 16-call bounds and must pass `defineSemanticCatalog` against the
actual host description. No semantic, transport or VM version changed. Actions
still use the observed revision, and a successful action may disable its button
or make its text field read-only.

The independent Node application's four v2 tasks now use the constructor while
its original v1 publication declaration remains unchanged. The real worker
completed the same read/publish/read/check workflow with Effects `[2, 9, 2, 1]`
and identical business results and final application state. For input
`Registered task construction ✓`, Fuel remaining changed from
`[9928, 9208, 9926, 9959]` to `[9928, 9198, 9926, 9959]`: the expanded publication
program retains a separate editable-draft guard and consumes ten more Fuel.
The existing 10,000 Fuel budget was not raised. Existing cancellation, late
commit refusal and post-commit failure tests passed after migration, including
from an offline-installed external SDK tarball.

A different review/note application model used the same public constructor,
provider and real Rust worker. Its read/complete/read workflow performed eight
Effects, completed once, disabled the button and preserved the note. Missing,
wrong-role, disabled and stale controls caused zero application writes. Wrong
business values or counts failed independent verification after one mutation,
without retry or rollback. A false acknowledgement was rejected by the actual
Rust gateway; its already-completed action remained visible. A text task also
verified successfully after making its field read-only and disabled.

All checks passed: SDK 223, frontend 68, Node Consumer 31, retained native carrier
six, and complete independent Rust Consumer/catalog agent 838 with formatting
and strict Clippy. The 13 new constructor tests cover expansion, mixed steps,
data ownership, malicious data shapes, variable capture, bounds, exact u64 and
actual-catalog incompatibility. The new publication catalog and four source
fixtures match both Rust and JavaScript planners; Rust independently type-checks
all four generated programs. All 20 pre-existing fixture hashes are unchanged.

The guide's complete TypeScript example compiled and ran through the actual
worker with eight Effects and independently verified pending/done/done results.
The package Consumer also constructed tasks through installed public exports.
Local evidence is under `target/verification/registered-tasks/`. All these runs
were offline and headless; no new native GUI session or model-service call was
used to establish this acceptance.

## Named String Tasks And Versioned Execution

Source guide: [named task inputs](../named-task-inputs.md).

On 2026-11-20, explicit semantic v3 declarations added up to eight named string
inputs. Every value remains data bound to its declared entry parameter, with
4096-byte per-value and 16 KiB aggregate limits. V1/v2 declarations, the original
Rust execution-style enum, `prepare_task`, the original Tauri task command and
worker protocol v1 retain their zero-or-one-input contracts. All 25 earlier
catalog/source fixture hashes remained unchanged.

The SDK exposes readonly v3 types and `defineRegisteredTaskV3`; offline and model
providers select exactly the declared input names. All v3 plans carry an explicit
`inputMode`, including zero-input tasks. The registered host rebuilds the declared
plan before attaching the application. Node pins worker protocol v2 for the
complete marked execution; Tauri routes to `agent_start_named_task`. Inner Bridge
and execution-report schemas stay at version 1. Unknown, downgraded or mixed
versions cannot fall back into a different execution convention.

A separate article application model receives title and content, then publishes
both. The actual Rust worker completed twelve Effects and independently checked
each input, each saved field, the publication count and the unrelated control.
A four-task workflow read both current fields, bound the two verified results
into publication and checked the complete inventory. It performed Effects
`[2, 2, 12, 1]`, with Fuel remaining `[9927, 9909, 9109, 9968]` for the recorded
Unicode/quote/newline inputs. The trace contains seventeen explicit dispatches
and seventeen accepts. The original 10,000 Fuel budget was not raised.

The independent Rust catalog agent also invoked the declared-task Node CLI
directly, without a shell. Discovery, Rust source generation, separate repeated
`--arg` values, real VM execution and independent Rust report verification all
passed. Publication used twelve Effects and 9175 remaining Fuel for its distinct
recorded inputs. Zero-input `read_title` and `check_article` passed through the
same CLI with two and one Effects, respectively, without an entry/input override.
Rust and JavaScript generated identical publication source.

End-to-end failure tests reject invalid input maps before application attachment,
keep the first edit when the second field fails, retain both edits while fencing
a late publication after cancellation or revocation, and retain a completed
publication when its handler subsequently fails. No path retries or rolls back.
The installed SDK tarball Consumer ran the same two-result binding workflow
through public exports and the real worker. Local model-provider fixtures select
two values without supplying source or making a model-service request.

Real execution exposed a source-generation limit that isolated declaration tests
did not catch: fixed nine-step chunks introduced enough helper parameter wrappers
to exceed the host's sixteen bindings. V3 now deterministically minimizes helper
captures within the existing chunk/function limits; the article source needs
fifteen bindings. A real registered-host compilation regression locks this in.
V1/v2 source partitioning and all parser, VM and host bounds remain unchanged.

Checks passed: language workspace 2408, complete independent Rust Consumer/catalog
agent 847, SDK 242, frontend 71, Node Consumer 42, standalone registered host 12,
retained native Node carrier six and portable Tauri 36. The Tauri desktop build,
69 native tests and strict Clippy also passed, including the named-task entry.
Formatting and strict Clippy checks passed for these Rust workspaces. Local evidence is under
`target/verification/named-inputs/`, including final logs, raw reports, generated
source and the unchanged-fixture hash check. Earlier failed logs are retained;
the final Consumer example and Clippy logs cover the iterator-only lint cleanup
after its complete 847-test run.

These checks use application models, actual Rust VMs and local transports. The
existing form and taskboard GUI catalogs remain v2; this milestone does not claim
a new multi-input native GUI session or live-model inference.

## Real Multi-Input Article GUI

Source guide: [article application](../article-host.md).

On 2026-11-20, the macOS Tauri article application ran its native v3 catalog
through the same conversation, semantic/model providers, workflow session,
registered-object bridge and Rust VM used by the form and task board. Native
startup pins `agent-article.html` and `tauri.article`. Its four declarations are
`read_title`, `read_content`, `publish_article(title, content)` and `check_article`.
Publication is local in-memory preview state; no article is sent to a website.

The native integration review found that `agent_start_named_task` had been added
to the invoke handler but omitted from the Tauri build manifest and page
capability. These declarations and the generated permission are now complete,
with a regression checking command/handler/permission agreement. The Article app
also rejects the legacy task route before compilation and session admission,
including zero-input requests. Existing form and board routes remain supported.

A dedicated `LeseLang Article.app` window discovered the native catalog and
displayed generated named-argument commands. Through its actual conversation UI,
the offline planner read the initial title, published distinct title/content
values supplied in reversed JSON key order, read the content again and checked
the inventory. These tasks performed `[2, 12, 2, 1]` Effects. The title was
`Native Article 标题 ✓`; the content contained a newline and `"quote"`. The
displayed publication report used twelve Effects and 9207 remaining Fuel, while
the real fields and preview matched both values. Publication count was one and
the unrelated control remained zero.

Missing content, an extra argument, numeric content and a duplicate title key
were then submitted through that same UI. All four were rejected before
application execution. The VM trace remained at seventeen dispatches and
seventeen accepts, and the displayed publication state was preserved.

The window then selected the explicitly named
`protocol-workflow-fixture-not-a-model` loopback provider. One request read both
live fields, separately bound the two verified results into publication and
checked the inventory. The UI showed all four steps verified and both source
references on the publication step. Its `[2, 2, 12, 1]` Effects advanced the
publication count to two without changing the values or unrelated control.
A second request supplied distinct literal values, `文章协议回归 ✓` and
`两个独立输入分别传入。`, then read both fields and checked the inventory. All four
steps passed with `[12, 2, 2, 1]` Effects. The publication report showed 9108
remaining Fuel; the final visible fields and preview matched those inputs,
publication count was three and the unrelated count remained zero.

The combined native trace contains exactly 51 dispatches and 51 accepts:
six text edits, three publication clicks, fifteen finds, twenty-four observations
and three inventory reads. The loopback log records two completed requests.
Actual UI/Accessibility observations and a final screenshot verified the visible
results; source display showed separate `content` and `title` entry parameters.
The UI tool truncates long report text, so the retained observation record
contains observed report snippets and state, rather than claiming full raw-report
exports. Fixed provider replies test protocol/execution behavior, not model
inference; no external model service or credits were used.

Checks passed: complete independent Rust Consumer/catalog agent 848, frontend
and provider-protocol tests 98, portable Tauri 41 and desktop Tauri 76, with
formatting, desktop build and strict Clippy. Rust and JavaScript compare all
four generated article programs; the actual registered host compiles them with
the unchanged sixteen-binding limit. Application tests cover both draft revisions
and ABA, real DOM identity/readback, cancellation, revocation, detached sessions,
late commits, invalid later fields and retained post-commit failures. All 27
pre-existing catalog/source fixture hashes stayed unchanged.

Local evidence is under `target/verification/article-gui.8WVHUp/`: native traces,
provider log, test logs, catalog, UI observations, hash check and summary. The
dedicated test window/process and temporary loopback service were closed. Its
retained local application bundle now launches in ordinary offline mode. The
existing form and task-board windows were not used for this acceptance.

## Structured Verification Diagnostics

Source guide: [verification diagnostics](../verification-diagnostics.md).

On 2026-11-20, the JavaScript semantic verifier, registered host, single-task
session and sequential workflow gained additive structured verification failures.
`PreparedSemanticTask.verifyDetailed` uses the same decision engine as Boolean
`verify`. An optional detailed host hook supplies bounded, frozen diagnostics;
Boolean-only hosts retain their existing outcomes and error strings. Failed
steps and workflows expose `verificationFailure` without upgrading a returned
value to success or enabling retries.

The independent Node Consumer exercised a real Rust worker with a host-declared
`requires_review` predicate. Publication completed twelve Effects, returned true
and retained one publication, but the predicate rejected its report. The failed
step and workflow named that condition, preserved the report and partial effects,
and never started the subsequent read. A later explicit read succeeded without
repeating publication. The installed npm tarball Consumer exercised the same
failure through public exports in a fresh external directory.

Two additional real-worker cases changed application state before execution or
after the worker report. Their semantic reports remained valid, while the
registered host distinguished `before_observation_mismatch` from
`after_observation_mismatch` using its independent snapshots. A post-commit
execution fault retained publication without inventing a verification diagnostic.

Regression coverage includes ordered semantic failures, malformed and duplicate
JSON, exact u64 evidence, missing/ambiguous predicate evidence, immutable owned
results, mismatched plans/reports, single-use completion evidence, callback
reentry, cancellation and late results. Review found that directly awaiting a
synchronous malformed thenable could run its getter before admission. The
detailed path now admits synchronous records first and accepts asynchronous
settlement only from a current-realm Promise. Both session and workflow tests
confirm rejected thenables cannot invoke that getter or grant success.

Tauri presentation maps the seventeen codes to specific Chinese explanations,
including the declared predicate ID when applicable. It preserves raw report
sections and stopped-step/partial-effect information; malformed diagnostics never
grant success, and successful or cancelled outcomes ignore stray failure detail.

Checks passed: SDK 262, frontend/provider 104, independent Node Consumer 46,
retained native Node carrier six, complete independent Rust Consumer/catalog
agent 848, portable Tauri 41 and desktop Tauri 76. Desktop build, Rust formatting
and strict Clippy passed. All 32 pre-existing catalog/source fixture hashes were
unchanged. Local logs and the final summary are under
`target/verification/diagnostics.xAA6Wl/`.

This milestone validates real VM execution, installed-package behavior and GUI
presentation tests. It does not claim a new native GUI interaction session or a
live model-service call. Language/transport versions, generated programs and the
explicit poll → authorize → dispatch → accept protocol remain unchanged.

## Shared DOM Control Bindings

Source guide: [application DOM controls](../dom-controls.md).

On 2026-11-20, the optional `@leselang/bridge/dom` entry added
`createDomTextControl` and `createDomButtonControl`. The form, task board and
article views now use these helpers for their shared manual/agent handlers.
Application readers, semantic catalogs and domain checks remain application
declarations. The helpers replace repeated event-context, Promise and error
slots with exact event identity and per-invocation completion. Text assignment
and input handling share one synchronous commit; asynchronous button work still
reenters an explicit commit gate. Views dispose their bindings on page exit.

Eighteen focused SDK tests cover duplicate bindings, synchronous nested and
cross-control events, callbacks after completion/disposal, manual interaction
during an agent await, earlier-listener cancellation, reader replacement,
validation-hook reentry and asynchronous misuse, missing handlers, ABA and
retained partial effects. Import/packed-declaration checks confirm that DOM
support remains an optional public entry without Node or third-party runtime
dependencies; importing it without browser globals is supported.

An independent Node Consumer uses an explicitly labelled EventTarget fixture
with the public helpers and a real Rust worker. It reads both fields, binds their
verified results into publication and checks the inventory in seventeen Effects.
It also covers cancellation, revocation, late callbacks, post-commit exceptions
and replacement of the pending button. The same application module runs from an
installed npm tarball in a fresh external directory. These fixture tests establish
event routing and VM behavior, without claiming to emulate browser rendering.

A dedicated native `LeseLang Article.app` then exercised the rebuilt WebView.
Manual title and multiline content input followed by a manual publication
produced one publication. Through the actual conversation UI, four separate
offline commands then read the title, published distinct named inputs, read the
new content and checked the inventory. All four commands verified successfully,
with `[2, 12, 2, 1]` Effects. The publication report showed 9016 remaining Fuel.
The final title was `Agent DOM 标题 ✓`; the content was `Agent 正文` followed by
a newline and `通过通用 DOM 绑定发布。`. Both draft controls and the published
preview matched, publication count was two and the unrelated count remained zero.

Pasting 1366 copies of `文` into the real content control tested a manual input
that fits the HTML character limit but exceeds the application's 4096-byte bound.
The application rejected it, restored the previous draft and retained the
published content/count. A subsequent agent inventory check still verified.
The native trace contains exactly seventeen dispatches and seventeen accepts:
five finds, eight observations, two text updates, one publication click and one
inventory read. Manual actions are not counted as VM Effects.

Checks passed: SDK 280, frontend/provider 104, independent Node Consumer 51,
retained native Node carrier six, complete independent Rust Consumer/catalog
agent 848, portable Tauri 41 and desktop Tauri 76. Desktop build, Rust formatting
and strict Clippy passed. The first desktop run exposed a race in the existing
local HTTP test server: a request read could return `WouldBlock`. Accepted test
connections now explicitly use blocking reads with their existing deadline;
the final desktop run passed. Production model transport was unchanged.

All 32 pre-existing catalog/source fixtures are byte-for-byte unchanged. Local
evidence is under `target/verification/dom-adapter.xwvemq/`, including both the
initial failed and final passing desktop logs, native trace, bundle hash,
UI observation summary, fixture comparison and final summary. Accessibility
truncates long report text, so UI evidence records observed snippets rather than
claiming full raw-report exports. A final screenshot also showed matching draft
and published values. The dedicated app was quit and its process was confirmed
absent; no model service or credits were used. Existing user windows were not
used for these interactions.

## Dynamic Registered Controls And Remounted Article Editor

Source guide: [dynamic application controls](../dynamic-controls.md).

On 2026-11-20, `createDynamicRegisteredApplication` added fixed logical control
slots with explicit `mount`, `replace`, `unmount` and `bindingRevision`. Empty
slots are absent from discovery and observation; successful structural changes
advance a local application revision, including same-value replacement and ABA.
The static factory retains its original four-method object shape. Both factories
share one lifecycle engine and the existing Bridge registration/generation path.

The registered host optionally captures that identity before execution and
checks it at request/reply boundaries and around independent verification.
Same-value replacement after a complete report now yields
`application_bindings_changed` verification detail, retaining the original report
and already committed effects while stopping subsequent workflow steps. Changes
detected during execution stop that attempt without inventing a report. These
boundary checks do not atomically retract an already dispatched Effect; partial
effects remain possible and no retry or scheduler was added.

Fifteen SDK lifecycle tests cover fixed slots and capacity, same-value replacement,
empty-slot dependencies, stale contexts, reader reentry, failure retirement and
fresh attachment. Host tests exercise invalid revisions, preflight and request
changes, teardown and verification-reader changes, precise report/plan correlation,
cancellation priority and old-scope isolation. Five Article view tests cover
remount, old targets, manual and agent continuations, failed binding creation
recovery and disposal while in preview.

Six independent real-worker Consumer cases exercise the dynamic DOM EventTarget
fixture, including the seventeen-Effect bound workflow, unchanged canonical
programs, same-value and ABA changes after a valid report, mid-handler replacement,
and a change before the first request. One case starts a fresh worker task while
the old handler remains suspended, then releases that old handler: it cannot
publish or invalidate the new task, which subsequently publishes exactly once.
The installed npm tarball Consumer independently verifies dynamic success,
same-value replacement during verification and a subsequent read. The fixture
does not claim browser rendering; native GUI acceptance is separate below.

The rebuilt dedicated native `LeseLang Article.app` added “预览草稿 / 继续编辑”.
Manual input set title `重挂载草稿 ✓` and multiline content. Preview removed all
four editor controls from the accessibility tree while displaying the preserved
draft. An offline `task read_title` while absent failed with the original report
error `unknown registered object`, made no publication and stopped. Its current
generic failure report omits normal budget fields, so the existing detailed
verifier displayed `report_budget_mismatch`; this is a known diagnostic-precision
limit, not successful lookup or a new missing-control diagnostic.

Returning to editing created fresh controls with the original draft. Through the
same conversation entry, four successful offline submissions read the title,
published new named inputs, read the content and checked the inventory. Between
publication and the final two tasks, preview/continue recreated the editor again
and retained the published values and count. The publication report visibly
contained twelve Effects and 9070 remaining Fuel. The final draft and published
title were `动态控件发布 ✓`, with content `新实例上的正文` followed by a newline
and `保持统一 Lese 流程。`; publication count was one and unrelated count zero.
A screenshot showed matching values and the final verified status.

One accidentally concatenated command during UI input was refused by the offline
planner before application execution; after clearing the field, the valid command
above succeeded. This refusal is retained in the observation summary. Across the
session the native trace records eighteen dispatches and seventeen accepts: the
absent-control lookup was dispatched but not accepted, and the four successful
tasks account for `[2, 12, 2, 1]` accepted Effects. There were three explicit action
pumps (two text writes and one publication); preview navigation used manual
application handlers.

Checks passed: SDK 302, frontend/provider 110, independent Node Consumer 57,
retained native Node carrier six, complete independent Rust Consumer/catalog
agent 848, portable Tauri 41 and desktop Tauri 76. Rust formatting, strict Clippy
and the native build passed. All 32 pre-existing catalog/source fixtures are
byte-for-byte unchanged. Logs, bundle hash, trace counts, UI observations and
summary are under `target/verification/dynamic-controls.MSRpcR/`.
Long AX report text was truncated, so the GUI evidence contains observed report
snippets and state summaries, not full raw-report exports. The dedicated app was
quit and its process confirmed absent. No model service or credits were used.

## Explicit Execution Failure Reports

Source guide: [execution failure reports](../execution-reports.md).

On 2026-11-20, the registered Rust host added an optional detailed task entry
that retains `ExecutionFailure` before errors become strings. The existing
string-error API delegates to the same driver, keeps its original text and
produces identical successful reports and Fuel. The public `Error` enum remains
unchanged. Tests distinguish a native missing-object error from a transport
message containing the exact same text; only the former gets the native category.
An independent Cargo Consumer exercises the new public entry and continues to
exhaustively match the old enum. Node worker error frames were not extended.

The JS SDK added the explicit `executionReports: "v1"` configuration on semantic
report preparation and registered hosts. Workflow sessions capture this trusted
host choice for their own referenced-source checks. Without it, existing generic
reports retain their own `status` business-field meaning. With it, a bounded
failed/cancelled report is refused after identity checks and before successful
budget/result checks, even if all successful fields are present. Six fixed codes
cover general failure, unavailable objects, identified host Fuel errors,
execution deadline, closed scope and remote cancellation. Raw error text does
not select a category or become display instructions. A remote cancelled report
does not replace the local cancellation signal.

Tauri explicitly enables this contract and emits `execution_failure: {code}`
from typed native errors and existing lifecycle flags. Empty error text receives
a fixed explanation; overlong text is truncated on a UTF-8 boundary with an
explicit suffix, keeping the report field within 4096 bytes. This report bound
does not change the legacy Rust error string. Native tests cover error bounds,
exact failure identity, cancellation/deadline/retirement priority and subsequent
session admission.

Thirteen new SDK tests and frontend integration/display tests cover strict
configuration, all semantic versions, malformed and unknown markers, contradictory
cancellation, complete-success-field disguise, correlation and structural identity,
old generic reports, and failed sources used by both bindings and requirements.
Installed npm tarball acceptance uses the public option and refuses a terminal
failure before propagating a source value. Independent JS tests explicitly
decorate real worker reports with injected failure markers to verify admission
and retained publications; these are not claimed as native cause-classification
tests. A separate uninjected real-worker test removes the publication slot,
retains two earlier text edits, fails lookup through the unchanged Node error
channel, then succeeds once after an explicit remount and new submission.

The dedicated native Article app reproduced the previous milestone's missing
control case. Manual draft title was `失败诊断回归 ✓`, with multiline content.
After preview removed the editor controls, `task read_title` failed and the UI
displayed “目标控件当前不存在或尚未挂载”. The complete short failure report was
visible in AX, including `execution_failure.code: execution_object_unavailable`;
the separate verification diagnostic carried the same code. The old budget
mismatch explanation no longer appeared. Publication and unrelated counts
remained zero.

After returning to edit, four explicit offline submissions read the preserved
title, published new title/content, read the resulting content and checked the
inventory. All four verified. The final draft and published title were
`失败后新任务 ✓`; content was `原因已明确` followed by a newline and
`本次只发布一次。`. Publication count was one and unrelated count zero. The
publication report showed twelve Effects and 9094 remaining Fuel. The native
trace recorded eighteen dispatches and seventeen accepts: one failed lookup
followed by `[2, 12, 2, 1]` successful Effects. No action was retried automatically.

Checks passed: SDK 315, frontend/provider 115, independent Node Consumer 60,
retained native Node carrier six, complete independent Rust Consumer/catalog
agent 848, registered host 16 plus its external Cargo Consumer, portable Tauri 42
and desktop Tauri 79. Format checks, strict Clippy and desktop build passed;
all 32 existing catalog/source fixtures remained byte-for-byte unchanged.
The initial full Consumer run encountered an existing process-capture deadline
test failure: its expected stdout prefix was empty. That unchanged test passed
in isolation, and the complete Consumer group subsequently passed. Both logs
are retained rather than treating the first run as successful.

Evidence is under `target/verification/execution-failures.h4llr1az/`, including
test logs, native trace, observed failure JSON, UI observations, bundle hash and
final summary. The short failed report was fully visible; long successful reports
remain AX snippets rather than full raw exports. Screenshots showed the precise
failure and final matching draft/publication. The dedicated app was quit and its
process confirmed absent. No model service or credits were used.

## Node Execution Failure Reports

Source guides: [Node embedding](../node-agent-host.md) and
[execution failure reports](../execution-reports.md).

On 2026-11-20, the optional Node transport added `executionReports: "v1"`.
Together with the same explicit registered-host setting, it carries typed
execution failures from the standalone Rust worker into the shared agent
verification path. Worker protocol v3 requires a separate `input_mode` field;
single-string and named-string plans retain their respective input bounds.
Default protocol v1/v2 frames, argument rules and exception behavior remain
unchanged. The option is captured once and no downgrade or retry is attempted.

Only an admitted execution with a live stdio connection can produce a failed
report. Startup rejection, broken outer framing or correlation, deadlines and
process failure retain their exception paths. A correlated reply with invalid
inner application data may instead yield a generic execution failure. Successful
reports stay unchanged, and failed reports do not invent a result, Effect count,
Fuel balance or final observation. The carrier requires clean child closure
before delivering either kind of report; report/plan correlation, one-use
completion and host verification remain required.

Real-worker tests remove an Article publication control before execution. The
first read verifies; the publication task commits both draft edits, fails lookup
with `execution_object_unavailable`, and prevents the final read from starting.
The original failed report is retained. After an explicit remount and separate
submission, publication verifies once with twelve Effects. A separate test
commits publication before the callback fails: the publication remains, the
workflow stops, and a new read observes it without publishing again.

Misleading valid Bridge error codes and malformed error prose both remain
`execution_failed`; neither can impersonate the typed missing-object cause.
Further real-worker checks cover raw-report correlation, one-use verification,
single/no-input composition, cancellation, revocation, late commits and admission
failure followed by a fresh process. The installed npm tarball Consumer exercises
the same missing-control fault through both default and opted-in transports,
preserving the old exception and checking the new report respectively.

The offline reference CLI now opts in explicitly and records each raw report
and diagnostic. Its successful four-step workflow completed `[2, 9, 2, 1]`
Effects and published once. With `--fault-after-commit`, its first read verified,
publication failed with `execution_failed`, the committed publication remained
at one, and the two later steps did not run. The command exited with status one,
while the worker had cleanly delivered its failed execution report.

Checks passed: SDK 320, frontend/provider 115, independent Node Consumer 68,
retained native Node carrier six, complete independent Rust Consumer/catalog
agent 848, registered host 22 plus its external Cargo Consumer, and portable
Tauri 42. Formatting, strict Clippy and worker/portable builds passed. Worker
tests compare an identical two-Effect successful report and Fuel across all
three protocol versions. All 32 existing catalog/source fixtures are unchanged.

The first Node check exposed an incorrect expectation in a new test: prose
containing spaces was already rejected by the existing inner error-code grammar,
so its report correctly said `invalid wire outcome`. The test was corrected to
cover both valid misleading codes and invalid prose separately; no production
validation was weakened. The complete Node group then passed.

Evidence is under `target/verification/node-execution-reports.khkl9s8x/`, including
initial and final check logs, original CLI JSON reports, CLI trace and summary,
fixture hashes and a final check summary. This acceptance used real worker
processes and independent JavaScript applications; its DOM fixture is an
EventTarget test fixture, not a native GUI run. All planning and execution were
offline, without a model service or credits.

## 2026-11-27 — application-declared JS/TS operations

On macOS arm64 with Node 24.6.0, the independent
[stock Consumer](../../consumer/javascript/agent/operations.mjs) submitted
`stock.reserve |> stock.remaining` with the item `Spare parts`. Its offline
provider read both named signatures from the application catalog and generated
a Lese `bind`. The first handler committed one reservation and returned ticket
`9007199254740993`. The second accepted that exact ticket and returned
`9007199254740998` remaining. The real v4 worker reported two Effects and 9,988
Fuel remaining. The application's count, balance and item checks passed.

The same provider ran a translation lookup followed by an archive write.
Its optional-string result carried both a translated word and a missing entry
through the VM. When a faulty archive silently deduplicated a repeated request,
both VM reports were identical, but only the first run increased the record
count and passed verification. Other real-worker tests denied a required grant
before any callback, revoked authority or cancelled while a handler was waiting,
and retained the committed archive entry after the handler returned a value of
the wrong type. Subsequent explicitly submitted tasks ran in fresh workers.

The SDK tests passed (333), along with the Tauri frontend (115), independent
Node Consumer (76), independent Rust Consumer and catalog agent (848), and
registered Rust host and worker (27 plus its external Rust Consumer). The Node
group installed the packed SDK into a temporary project, type-checked the new
public entries and ran the stock example there. Portable and macOS desktop
Tauri check groups passed. The desktop group's existing model transport tests
used a local loopback fixture. This run exercised deterministic planning and
application-owned state in Node.

## 2026-11-27 — Rust, Node and .NET operation conformance

On macOS arm64 with .NET SDK 10.0.201, the independent C# application read
its operation catalog, generated the shared meter program and executed it in
the Rust worker. Its balance increased from `9007199254740992` to
`9007199254740993`. The application observed one add and one read. The
in-process Rust Consumer and the Node Consumer matched the same fixture,
including the exact final integer, two Effects and 9,994 Fuel remaining.
Node and C# used the explicit `lese.operations` profile. Existing Node
applications continued to use `javascript.operations` by default.

The C# suite exercised all six scalar types, including `ulong.MaxValue`, an
absent optional string and a list containing Unicode text. Missing grants
prevented both handlers. Cancellation and revocation while a handler was
awaiting work prevented its commit. A handler returning an invalid result
after a write left the balance incremented once and prevented the read. An
old context remained fenced when another submission ran. Two deliberately
misbehaving worker processes exercised output arriving before a reply and
nonzero exit after a valid report. Neither delivered success evidence.
The .NET build completed with zero warnings and errors. See the
[C# Consumer and reproducible check](../dotnet-host.md).

## 2026-11-27 — native Windows worker and C# Consumer

The Windows Server 2022 Datacenter x64 job built the Rust 1.98.0 MSVC worker
and ran the independent C# Consumer on .NET 10.0.15 (OS build 20348). All
19 selected worker tests passed, including generation of fresh 64-character
hexadecimal challenges and rejection of mismatched replies. The .NET build
reported zero warnings and zero errors.
[Windows job and log](https://github.com/Team-silvortex/leselang/actions/runs/45986249213/job/140462507890)
· [Tested source](https://github.com/Team-silvortex/leselang/commit/34502cb3a9b621f55f760426b28af1673487f9e4)

The native executables ran the shared meter program in two Effects. The C#
application observed one increment, one read, the exact balance
`9007199254740993`, and 9,994 Fuel remaining. The same job confirmed the
following application and transport outcomes.

- Missing authority reached zero callbacks. A write followed by an invalid
  result committed exactly once and prevented the subsequent read.
- Revocation and cancellation while awaiting a write fenced the pending
  mutation and its saved context. A fresh submission could still complete.
  Catching a denied commit and restoring the grant kept the old context retired.
- Six scalar types crossed the VM boundary, including an absent optional
  string and the largest unsigned 64-bit integer.
- An out-of-order request produced no second callback or late commit. The same
  well-formed report was accepted after exit code zero and rejected after
  nonzero exit.

The Ubuntu and macOS jobs ran the same Consumer at this revision.
[Ubuntu job](https://github.com/Team-silvortex/leselang/actions/runs/45986249213/job/140462507900)
· [macOS job](https://github.com/Team-silvortex/leselang/actions/runs/45986249213/job/140462508054)

## 2026-11-27 — using a host list to decide whether to write

On macOS arm64, independent Rust, Node and C# Consumers ran the same
[Lese program and five input cases](../../fixtures/application-operations/list-membership.json).
Each application supplied its own `filters.available` and `filters.select`
handlers and checked the returned boolean, number of reads and selected values.
Rust used its embedded VM. Node and C# used the native worker.

| Available options and request | Observable result on each host |
| --- | --- |
| `["草稿", "已发布", "已发布"]`, request `"已发布"` | `true`, one read and one selection of `"已发布"`. |
| `["草稿", "已发布"]`, request `"发布"` | `false`, one read and zero selections. |
| Empty list, request `""` | `false`, one read and zero selections. |
| `["", "草稿"]`, request `""` | `true`, one read and one selection of the empty string. |
| `["é"]`, request `"e\u0301"` | `false`, one read and zero selections. |

The CLI also rejected an invalid operand type in an unchosen branch at
compile time. With insufficient Fuel, a program that tested membership before
calling `std.print` exited with a Fuel error and empty stdout.

## 2026-11-27 — shared operation failure vectors

The Rust, Node and C# Consumers executed the same `main` from the
[failure fixture](../../fixtures/application-operations/failure-vectors.json)
on macOS 27.0.1 arm64. Rust 1.98.0 embedded the VM directly. Node 24.6.0
and .NET 10.0.5 (SDK 10.0.201) used the native v4 `lese.operations` worker.
The fixture SHA-256 was `4c52d3fa08075c329e9466e55f2ff5b928e6d98b5e224f2b97f732f33e7278d4`.

Each case started with a fresh meter at `9007199254740992`. Each application
recorded its outcome, write and read callbacks, committed writes and final
balance. All three observations matched each other and the fixture field for field.

| Case | Outcome | Write callbacks | Read callbacks | Committed writes | Final balance |
| --- | --- | ---: | ---: | ---: | ---: |
| `missing-grant` | Failed | 0 | 0 | 0 | `9007199254740992` |
| `post-write-invalid-result` | Failed | 1 | 0 | 1 | `9007199254740993` |

With only `meter.read` granted, Rust observed a compile-time capability
denial. With both grants, the write handler incremented the meter and returned
a string for its declared integer result. Rust observed a typed receipt
rejection, polled the still-pending session and cancelled it explicitly. Node
returned a failed submission without a report. The C# call raised
`InvalidDataException`. After recording each failure, C# restored full grants
and a valid reply, then verified a fresh two-Effect run on the same host.

The complete `consumer`, `node-consumer` and `dotnet-consumer` check groups
passed. The independent Rust group ran 851 tests, the Node suite ran 81 tests,
and the .NET group passed 19 Rust worker tests and the C# Consumer.

## 2026-11-27 — losing a write request or its successful reply

On macOS 27.0.1 arm64, Rust 1.98.0, Node 24.6.0 and .NET 10.0.5
ran the [same two programs and loss cases](../../fixtures/application-operations/lost-reply.json).
The fixture SHA-256 was `528bb63246c6bd713a02da42ed31bab87319905b0d4950c5e94be7861eb83bf8`.
Each application started a fresh in-memory ledger at `9007199254740992`.
Its write handler recorded the new balance under a caller-supplied operation key.

| Cut | Write callbacks | Committed writes | Tail reads | Receipt under the submitted key | Final balance |
| --- | ---: | ---: | ---: | --- | ---: |
| Request withheld before the handler | 0 | 0 | 0 | `null` | 9007199254740992 |
| Successful reply withheld after commit | 1 | 1 | 0 | `"9007199254740993"` | 9007199254740993 |

Rust dropped the authorization or the typed receipt. Both embedded sessions
remained pending until explicitly cancelled. For Node and C#, a test relay
started the native v4 worker. On the first cut it withheld the worker's write
request. On the second it read the application's successful reply, checked
its scope, serial, challenge and decimal value, then terminated the worker
before forwarding it. Each harness verified an independent cut marker. Node
reported `failed` with `possiblePartialEffects`, and C# observed EOF. Neither
cut produced a run report.

Each Consumer opened fresh sessions with only `ledger.lookup` granted. The
original key produced the receipts above. An unrelated key returned `null`
in both cases. After recording those observations, all three resubmitted the
same write program with the already-committed key as a control. Each recorded
two total writes and a balance of `9007199254740994`.

The application stayed alive throughout, and the interrupted writer had
stopped before each lookup. The experiment covers that ordering. Receipt
durability across application restarts and lookups racing a pending write
need separate evidence. Reproduce with the `consumer`, `node-consumer` and
`dotnet-consumer` groups of `sh scripts/check.sh`.

## 2026-11-27 — same-key retry after an interrupted write

The independent Rust, Node and C# Consumers ran the expanded
[lost-reply fixture](../../fixtures/application-operations/lost-reply.json)
on macOS 27.0.1 arm64 with Rust 1.98.0, Node 24.6.0 and .NET 10.0.5
(SDK 10.0.201). The fixture SHA-256 was
`113349da6422d9937e309db06976a3f37f6bd35ceaf26807a28d7eb9c2ecb2e3`.
Each host implemented `ledger.add_once` with an application-owned receipt
per business key. The program called it, read the balance, then returned
the write receipt.

After each cut a fresh lookup-only session checked the original key, and
another checked an unrelated key. The interrupted program produced no result
or tail read. Once it had stopped, a fresh write-authorized session submitted
the same program and key. Rust drove the embedded VM explicitly. Node and C#
used the native v4 worker with the same two cuts described above.

| Cut | Receipt before retry | Total write callbacks after retry | Total commits | Tail reads | Accepted Effects in retry | Returned receipt |
| --- | --- | ---: | ---: | ---: | ---: | --- |
| Request withheld | `null` | 1 | 1 | 1 | 2 | `9007199254740993` |
| Successful reply withheld | `9007199254740993` | 2 | 1 | 1 | 2 | `9007199254740993` |

In both cases a second key advanced the balance to `9007199254740994`.
Calling the original key again returned `9007199254740993`, with two commits
total across both keys. The ordinary `ledger.add` control committed twice
when resubmitted with one key after reply loss. All three Consumers' retry
and second-key observations matched the fixture field for field.

The application and its receipt store stayed alive between submissions.
These sequential runs exercise a settled first attempt. Reproduce with
`sh scripts/check.sh consumer`, `sh scripts/check.sh node-consumer` and
`sh scripts/check.sh dotnet-consumer`.

## 2026-11-27 — scalar host with an externally supplied evaluator

On macOS 27.0.1 arm64, Node 24.6.0 ran the
[wire probe](../../consumer/javascript/agent/operations-wire.test.mjs) in a
temporary directory containing the probe, a copy of the
[meter fixture](../../fixtures/application-operations/two-effects.json) and
the v4 reference worker renamed to `evaluator`. Node was invoked by absolute
path. The child environment set `PATH` to that directory and supplied the two
file paths. Both checks ran using the Node standard library.

The application observed its meter increment to `9007199254740993` and
accepted the report after two Effects, with 9,994 Fuel remaining and a clean
process exit. With only the read grant, the worker returned an error and
exited unsuccessfully with zero callbacks and the original meter balance.
This run exercises the stdio binding between an independent host and the
reference evaluator. The planned Nuis host has a separate acceptance target.

## 2026-11-27 — registered GUI host and gateway

The [publication tests](../../hosts/registered/tests/registered_gui_workflow.rs)
ran the existing article program against a stateful in-memory application through
the public registered gateway and the VM. Nine Effects found the draft, observed
and edited it, found the publish button, clicked it and observed the result. The
host then read both controls again. The application verified the published text
and counted one edit and one click. An acknowledged edit that left the text
unchanged stopped the program before the click. A concurrent edit after the
program returned true caused the final publication check to fail.

The failure cases checked the last request and the application's actual writes.
A rejected click left the draft edited. A click that committed and then lost
its acknowledgement published exactly once. A write on the last permitted wire
request survived the inability to verify it. The
[local session tests](../../hosts/registered/tests/local_gui_session.rs)
also exercised revocation, object replacement, stale observations, disabled and
read-only controls, cancellation and late replies across dispatch and pump.
Two tests reproduced a defect in explicit replacement. The host now validates
the replacement snapshot against the slot's original identifier and metadata
rules. A failed replacement retires only the affected slot.

The table reports covered production lines divided by instrumented production
lines on macOS arm64 with Rust 1.98. Each column pair uses the same test
selection on both source snapshots. The first pair includes the registered
crate, the independent Rust Consumer, the Node wire probe and the C# Consumer.
The second pair also includes the existing 23 portable Tauri gateway tests.
Counts are deduplicated by source path and line within each snapshot and
exclude test code. Sharing the snapshot check reduced the host denominator by
two lines.

| Production source | Registered before | Registered after | With Tauri before | With Tauri after |
| --- | ---: | ---: | ---: | ---: |
| `host.rs` | 296/685 (43.21%) | 585/683 (85.65%) | 543/685 (79.27%) | 591/683 (86.53%) |
| `gateway.rs` | 163/320 (50.94%) | 291/320 (90.94%) | 304/320 (95.00%) | 307/320 (95.94%) |
| All of `hosts/registered/src` | 1300/1972 (65.92%) | 1724/1970 (87.51%) | 1734/1972 (87.93%) | 1788/1970 (90.76%) |

Run `sh scripts/check.sh registered` for both new suites and the independent
Rust Consumer. The `tauri-portable`, `node-consumer` and `dotnet-consumer`
groups also passed. All checks in this record run without a desktop window.

## 2026-11-27 — registered GUI publication across Rust, Node and C#

On macOS 27.0.1 arm64, the three independent Consumers executed the same
15 cases with Rust 1.98.0, Node 24.6.0 and .NET SDK 10.0.201.
Rust used the embedded registered host. Node and C# each owned an in-memory
article and served GUI requests from a separate v3 worker. For every case
each Consumer ran the publication once and read both controls using two new
read-only Lese sessions. All 45 primary observations and 90 readback sessions
matched the fixture. The three per-language records matched field for field
including the ordered primary request trace.
[Fixture and fault definitions](../../fixtures/registered-gui/README.md)

| Primary run | Result | Program commits (edits, clicks) | Subsequent readback |
| --- | --- | ---: | --- |
| Successful publication | True, verified, 9 Effects | 1, 1 | Draft and publication contain the supplied text |
| Edit acknowledged without changing the value | False, 5 Effects | 0, 0 | Both empty |
| Draft disabled or read-only | False, 3 Effects | 0, 0 | Both empty |
| Wrong publish target, old revision on an edited draft, or click rejected | Execution failed | 1, 0 | Draft contains the supplied text, publication empty |
| Committed click with false acknowledgement, or invalid final observation | Execution failed | 1, 1 | Both contain the supplied text |
| Publication superseded after the program inspected it | True, verification failed, 9 Effects | 1, 1 | Draft contains the supplied text, publication contains the outside edit |
| Cancel before or after committing the edit | Cancelled at the same write request | 0, 0 or 1, 0 | Publication empty in both cases, draft retains only the committed edit |
| Draft replaced or write permission withdrawn at the edit | Execution failed | 0, 0 | Both empty |
| Outside edit during the pending write | Execution failed | 0, 0 | Draft contains the outside edit, publication empty |

All 15 cases matched again with different Unicode input and replacement text.
As a negative control, setting the successful case's expected click count to
two made each Consumer fail that case before emitting its observation record.

Run `sh scripts/check.sh registered-gui` to regenerate the three observation
files. The `registered`, `node-consumer` and `dotnet-consumer` groups also
exercise the fixture through their existing independent entry points.

## 2026-11-27 — scalar hosts in Python, C and C++

On macOS 27.0.1 arm64, `sh scripts/check.sh scalar-consumers` ran three
cases in each of three applications against the reference worker v4.
Python 3.14.3 used its standard library. The C11 and C++17 applications used
Apple Clang 21.0.0, cJSON 1.7.18 and the same native stdio peer. Each
executable linked its own meter handlers and held its own balance. The
runner compared nine observations with the shared fixtures.
[Two Effects](../../fixtures/application-operations/two-effects.json) ·
[Failure vectors](../../fixtures/application-operations/failure-vectors.json)

| Case | Worker outcome | Add / read callbacks | Committed writes | Balance observed by the application |
| --- | --- | ---: | ---: | ---: |
| Two Effects with both grants | Integer `9007199254740993`, 2 Effects, 9994 Fuel remaining | 1 / 1 | 1 | 9007199254740993 |
| Missing write grant | Failed | 0 / 0 | 0 | 9007199254740992 |
| Invalid result after commit | Failed | 1 / 0 | 1 | 9007199254740993 |

All three applications also reached the maximum unsigned 64-bit value
(`18446744073709551615`) with a temporary fixture starting one below it.
Changing the post-write fixture to expect zero commits made each Consumer
fail the case.

The local observations are in `target/verification/scalar-consumers.ia3ukd72/`
as `python.json`, `c.json` and `cpp.json`. A rerun creates a new directory
with the same three files. [Conformance runner](../../scripts/check-scalar-consumers.py)
