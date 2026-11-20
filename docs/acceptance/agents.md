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

