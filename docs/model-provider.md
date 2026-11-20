# Model Planning Through A Host Semantic Catalog

The Tauri conversation entry can use a configured model service to interpret
natural language. The model selects declared tasks and their arguments
from the current host's semantic catalog. The JavaScript SDK generates the
corresponding LeseLang programs, and the application independently admits,
executes and verifies them through the existing host boundary.

```text
Natural-language request + host semantic catalog
  → application-owned model transport
  → declared task selection + arguments + optional result requirements
  → catalog-generated LeseLang programs
  → host plan comparison, compilation and authorization
  → explicit VM Effect delivery
  → declared predicates + fresh application observations
  → verified result in the conversation
```

The reusable provider has no form, control-name or application-profile branches.
It uses the same semantic v1, opt-in v2 and explicit v3 declarations as the
[offline provider](agent-entry.md) and [independent catalog agent](agent-workflow.md).
`createModelProvider` retains its single-task contract. The additive
`createModelWorkflowProvider` selects an ordered list of one to eight declared
tasks for an [`AgentWorkflowSession`](agent-workflows.md), as used by the Tauri
conversation. Legacy v1/v2 tasks accept zero inputs or one named string input;
v3 tasks accept zero to eight independently named strings. Neither
provider invents tasks from arbitrary operations or supplies conversation history.
A declared task may itself contain several host Effects and guards.
Versions 2 and 3 can bind a task's returned scalar to a declared observed report value,
allowing tasks such as reading current application text. The model supplies no
expected runtime value for such a task; the SDK and application verify its
actual report. See [observed result contracts](host-semantics.md#version-2-observed-results).
Workflow arguments are literal by default. An application can enable explicit
references to verified earlier results with `resultBindings: true`; the Tauri
conversation enables that mode. See [verified result bindings](agent-workflows.md#bind-a-verified-result-to-a-later-input).
The independent `resultRequirements: true` option allows a later step to require
an earlier verified result to equal specified text. Tauri enables both options;
a failed requirement stops execution before that target task.

## SDK Integration

The public `@leselang/bridge` entry exports both model provider factories with
the same `ModelRequest` transport. `ModelWorkflowProviderOptions` extends the
single-task `ModelProviderOptions` with optional `resultBindings` and
`resultRequirements`, each defaulting to `false`. Existing single-task callers
can continue using `createModelProvider` with `AgentSession`:

```ts
import {
  AgentSession, createModelProvider,
  type ModelRequest, type AgentHost,
} from "@leselang/bridge";

function createAssistant(
  host: AgentHost,
  complete: (request: ModelRequest, signal: AbortSignal) => Promise<string>,
) {
  return new AgentSession({ host, provider: createModelProvider({ complete }) });
}

// The application supplies a transport to its credential-owning native backend
// or server. It returns the model's complete JSON text, not the API envelope.
const session = createAssistant(host, applicationModelTransport);
const outcome = await session.submit("请把“周五完成设计评审”保存到应用中。");
```

`ModelRequest` has two immutable strings, `system` and `user`. The first describes
the selection contract. The second encodes the current message, validated task
summaries and the complete host catalog. Raw catalog JSON remains a string inside
that request, preserving exact `u64` values rather than rounding through
JavaScript `Number`. Prefer original catalog JSON from `host.describe`.
V3 task summaries include `inputNames`, including an empty array for no-input
tasks; legacy summaries retain optional `inputName`. The existing provider
factories read this catalog contract without an additional multi-input option.

For `createModelProvider`, the transport must return exactly one JSON object in
one of these forms:

```json
{"task":"save_message","arguments":{"message":"周五完成设计评审"}}
```

```json
{"task":"inspect_controls","arguments":{}}
```

```json
{"error":"The requested operation is not declared by this application."}
```

With a v3 catalog, the same selection shape carries every declared name:

```json
{"task":"publish_article","arguments":{"title":"Release notes","content":"The complete text."}}
```

The provider derives `inputMode: "named_strings"` from the admitted declaration,
including for no-input v3 tasks. The model cannot choose that marker or a worker
version. Missing, extra or non-string inputs fail before execution. See
[named task inputs](named-task-inputs.md) for declarations, the offline provider's
JSON-map command syntax and the complete reference workflow. The bundled Tauri
form and board catalogs remain v2 and retain their existing GUI procedures.

For `createModelWorkflowProvider`, a successful reply instead contains `steps`:

```json
{"steps":[{"task":"inspect_controls","arguments":{}},{"task":"save_message","arguments":{"message":"周五完成设计评审"}},{"task":"check_controls","arguments":{}}]}
```

The refusal form remains `{error}`. The two providers reject each other's success
shape; this is an explicit API choice, not an automatic fallback. See the
[workflow integration example](agent-workflows.md#model-planning) for constructing
an `AgentWorkflowSession` and retaining each step's result.

When `createModelWorkflowProvider` is constructed with `resultBindings: true`,
a task argument may instead contain a backward reference and explicit format:

```json
{"steps":[{"task":"inspect_controls","arguments":{}},{"task":"save_message","arguments":{"message":{"fromStep":0,"format":"decimal"}}},{"task":"check_controls","arguments":{}}]}
```

This references step 0's verified integer result; it does not supply the number
to save. The SDK checks the complete dependency graph before execution and
resolves the target input from the actual accepted report only after both
semantic and application verification succeed for the source. Supported formats
are `string`, `decimal` and `boolean`, matching string, integer and Boolean
results respectively. References to later steps, unsupported source types,
arbitrary field paths, implicit conversions and a top-level `bindings` field
are rejected. The opt-in does not change the single-task provider's contract.

For a v2/v3 source with `verification.result.kind: "observed"`, reference admission
uses its declared `scalar_type`. For example, the Tauri `read_message` task
returns a verified current string, so a later Save can use
`{"fromStep":0,"format":"string"}`. The source's actual typed result must equal
its declared observation, pass the remaining predicates and pass the host's
fresh-state verification before it can feed the binding. A model is neither
asked to guess that text nor allowed to replace it with a predicted result.
For a v3 target, `resultBindings: true` permits separate earlier-result
references for different named inputs. Every resolved value and the combined
argument map are rechecked before that target executes.

With `resultRequirements: true`, each selected step may additionally contain one
`requires` object. This example also needs `resultBindings: true` for its input:

```json
{"steps":[{"task":"read_message","arguments":{}},{"task":"save_message","arguments":{"message":{"fromStep":0,"format":"string"}},"requires":{"fromStep":0,"format":"string","equals":"条件执行回归 ✓"}},{"task":"check_controls","arguments":{}}]}
```

`equals` is explicit expected text, not a claimed observation. The SDK admits
the complete requirement graph against declared result types before executing
any step, then compares the actual earlier result only after semantic and host
verification. A mismatch stops the workflow without dispatching the target.
`requires` is rejected when its option is disabled; top-level `requirements`
and model-supplied `actual` or `satisfied` fields are always rejected. Decimal
expectations use exact canonical `u64` text, Boolean expectations use `"true"`
or `"false"`, and strings preserve exact text. This condition does not make a
later action atomic with the earlier observation. See the complete
[requirement contract](agent-workflows.md#require-a-verified-result-before-continuing).

These identifiers are examples from the Tauri reference catalog. Another
application can declare different tasks, parameters and operations without
changing the provider. The prompt asks the model to refuse missing or ambiguous
input and unsupported work. Local validation rejects undeclared tasks, wrong
argument names or types, additional fields, duplicate JSON keys, malformed text
and oversized replies. It does not strip Markdown, repair JSON, retry or switch
to an offline provider after failure. Refusal text and transport exception bodies
are not copied into application errors.

The model cannot supply source, execution modes, permissions, verification predicates or a
`verified` flag. `catalog.prepare(task, arguments)` produces canonical source and
the host-authored summary for each task. Literal and resolved input stays in the
argument map. A reference is normalized by `prepareSemanticWorkflow` into a
canonical template plus a binding; its runtime value never becomes source text. The Tauri
host independently rebuilds that plan from its own described catalog before
execution and compares the source and arguments. A successful model response
alone performs no application work. A workflow admits all selected tasks before
its first execution, then verifies one step before advancing to the next.

Verification establishes that execution met the selected declaration and the
application's current observations. It does not prove that a model interpreted
the natural-language request correctly; an otherwise valid selection can still
misunderstand the user's intent or text. The conversation displays the selected
task and exposes the generated workflow and execution evidence for inspection.

## Configure The Tauri Transport

Build the frontend and desktop host as described in the
[agent-entry guide](agent-entry.md#run-the-interactive-reference). Model settings
are read by the Rust process when the application starts:

| Environment variable | Meaning |
| --- | --- |
| `LESE_MODEL_ENDPOINT` | Complete Chat Completions URL, including its path; no path is appended |
| `LESE_MODEL_NAME` | Model identifier accepted by that endpoint |
| `LESE_MODEL_API_KEY` | Bearer token; required for remote endpoints, optional for explicit loopback endpoints |

The endpoint accepts HTTPS, or HTTP for `localhost`, `127.0.0.1` and `[::1]`.
User-info, query strings and fragments are rejected. Redirects are not followed.
The configuration is application-owned: the page receives availability and model
name, but neither the endpoint nor the credential. Requests go through the Rust
transport; the WebView does not receive network access or an API key.

For a remote service, the following zsh example reads the key without displaying
it or placing its value in shell history. Replace the model placeholder with one
enabled by your service:

```sh
export LESE_MODEL_ENDPOINT='https://api.openai.com/v1/chat/completions'
export LESE_MODEL_NAME='your-enabled-chat-model'
read -rs 'LESE_MODEL_API_KEY?API key: '
print
export LESE_MODEL_API_KEY
target/lean/leselang-tauri-host --agent
unset LESE_MODEL_API_KEY
```

For an already running compatible local service:

```sh
export LESE_MODEL_ENDPOINT='http://127.0.0.1:8080/v1/chat/completions'
export LESE_MODEL_NAME='your-local-model'
unset LESE_MODEL_API_KEY
target/lean/leselang-tauri-host --agent
```

With all three variables absent, the reference starts with the offline command
provider. A partial or invalid configuration shows a configuration failure and
disables model selection; the clearly labelled offline entry remains available.
The conversation lets the user explicitly select model or offline
planning; a failed model call never silently executes an offline interpretation.
Provider selection is fixed while a task is active.

The native transport submits a non-streaming Chat Completions request with
system/user messages and `response_format: {"type":"json_object"}`. The prompt
explicitly asks for JSON. It accepts one completed assistant text choice with
`finish_reason: "stop"`; refusal fields, tool/function calls, truncated replies
and other unsupported response forms fail before execution. A compatible service
must support this request shape. JSON mode supplies JSON syntax, so the SDK still
checks the complete task/argument contract. See the
[official Chat Completions request reference](https://developers.openai.com/api/reference/resources/chat/subresources/completions/methods/create).

Each configured call sends the submitted user message, task summaries and full
host catalog to the selected service. The service is responsible for its own
handling of that data. The reference does not send prior conversation turns or
use them to resolve phrases such as “do the same again.” Application state still
persists between independently submitted tasks.

## Bounds, Cancellation And Failure

Model planning is separate from VM execution and does not consume VM Fuel or
grant a capability. The original explicit
`poll → authorize → dispatch → pump → accept` flow, session Fuel, revocation,
late-reply checks and partial-side-effect behavior remain in the host.

| Boundary | Limit |
| --- | --- |
| User message | 8 KiB UTF-8 |
| Host catalog | 256 KiB UTF-8 |
| Model system prompt / encoded user request | 16 KiB / 2 MiB UTF-8 |
| Native HTTP response envelope / assistant text | 128 KiB / 16 KiB UTF-8 |
| SDK model selection JSON | v1/v2: 16 KiB; v3: 128 KiB UTF-8 |
| Inputs per task | v1/v2: zero or one string; v3: zero to eight named strings |
| Task string argument / generated source | 4,096 bytes / 16 KiB UTF-8 per step |
| Combined v3 argument values | 16 KiB UTF-8 per task; strings require valid Unicode and no NUL |
| Workflow steps / complete model selection | 1–8 steps sharing the applicable SDK and transport limits |
| Native model request | 45 seconds, with a 10-second connection limit |

All steps and their arguments share the applicable selection limit. The v3 SDK
allows 128 KiB of JSON to accommodate several escaped strings; the reference
Tauri HTTP transport still caps assistant text at 16 KiB. Additional input slots
do not enlarge that native transport budget. The larger encoded
request bound accounts for the catalog being escaped inside
JSON. Each page admits at most 64 model request identities. It retains retired
identities, including a cancel that arrives before the corresponding start, so
an old request cannot be replayed. Only one model request can be active. A
cancelled request keeps its slot until the native work has settled; page closure
also retires that work. Late plans cannot start the VM. Stopping the local request
does not establish whether the remote service has already completed inference.

Network failures, service errors, rate limits, invalid JSON, refusal and invalid
task selection fail during planning with zero application Effects. The native
request is attempted once. After execution starts, cancellation or failure may
leave earlier mutations in place; the conversation reports that possibility and
never automatically retries the workflow. Multi-step execution retains earlier
reports and stops the remaining steps after failure or cancellation. A single
model request supplies the complete sequence; the session does not replan or
call the model again between steps.

## Verification Scope

SDK tests inject a completion function to exercise the protocol without a paid
model call. They cover renamed applications and inputs, no-input tasks, exact
literal text, malformed selections, credential-bearing exception sanitization,
refusal, cancellation and late completion. The packed SDK is also installed and
type-checked from an independent consumer.

Native HTTP tests and a local protocol test service can establish the actual
request shape, response admission, cancellation and failure handling. Driving
the Tauri page against that service additionally establishes real WebView and
VM behavior. A fixture that returns a predetermined selection is not a language
model and does not establish natural-language interpretation quality.

The repository includes an explicitly labelled
[loopback protocol fixture](../hosts/tauri/tests/model-service.mjs). After building
the application, start it in one terminal:

```sh
node hosts/tauri/tests/model-service.mjs 19081
```

Use another terminal to launch the desktop host against that fixture:

```sh
unset LESE_MODEL_API_KEY
LESE_MODEL_ENDPOINT='http://127.0.0.1:19081/v1/chat/completions' \
LESE_MODEL_NAME='protocol-workflow-fixture-not-a-model' \
target/lean/leselang-tauri-host --agent
```

The service accepts the following exact test messages and returns predetermined
responses. Other messages fail, because this fixture does not perform inference.
The current page must show `protocol-workflow-fixture-not-a-model` as its selected
model. This fixture mode wraps single-task examples in `steps` and also accepts
the multi-step examples below. The legacy `protocol-fixture-not-a-model` id
continues to return the original `{task, arguments}` shape for single-task SDK
callers; it is not the current conversation page's workflow response format.

| Submission | Expected observation |
| --- | --- |
| `请把“模型接入回归 ✓”保存到应用里。` | `save_message`, verified `string: "saved"`, matching input/saved content, one Save and nine VM Effects |
| `看看这个应用有多少个控件，不要修改内容。` | `inspect_controls`, verified `integer: 3`, unchanged content/counts and three VM Effects |
| `请帮我发送一封邮件。` | Explicit model refusal; no new application Effects |
| `测试未声明任务` | Invalid task selection rejected; no new application Effects |
| `测试取消模型请求` | Fixture delays 15 seconds; press Cancel during planning, then confirm no new Save and no late execution |
| `测试模型服务限流` | HTTP 429 becomes planning failure; no retry and no new application Effects |
| `先检查控件，再保存“多步骤工作流 ✓”，最后核对控件。` | Three verified steps in order: `integer: 3`, `string: "saved"`, `none`; one additional Save and 15 VM Effects |
| `测试后续步骤无效` | Invalid second task rejects the whole plan before the otherwise valid first save; no new application Effects |
| `先检查控件数量，把这个数量保存为文本，再核对控件。` | Verify the integer count, bind its decimal text into the save input, then check controls; saved text is `3`, with one additional Save and 15 VM Effects |
| `测试结果引用类型错误` | An integer source requested with string format rejects the complete plan before execution |
| `测试引用尚未完成的步骤` | A forward reference rejects the complete plan before execution |
| `读取当前输入框的文本，原样保存，再核对控件。` | The v2 read task returns the current input; a verified string binding supplies Save; the final check completes with no state changes beyond the one Save (14 VM Effects) |
| `读取当前文本，只有它等于“条件执行回归 ✓”时才原样保存，再核对控件。` | Matching input permits one Save and final verification (14 VM Effects); different input stops after the verified read, with no Save (two VM Effects) |
| `测试条件结果类型错误` | A requirement with the wrong source-result format rejects the whole plan before any Effect |

Keep the fixture terminal open for the test and stop it with Ctrl-C afterward.
Its log records request identity, response status and connection completion,
without printing request messages, the catalog or authorization headers. A
separate real-model test must use a genuine inference service and evaluate its
natural-language choices; substituting this fixture does not satisfy that test.

No online-model acceptance is claimed by these fixture checks. Live-service
acceptance remains a separate check with a genuine endpoint, model and usable
credentials. It should record the selected task,
generated source, actual application result and independently verified report,
without recording credentials.

### Recorded macOS WebView Check

The dated evidence is preserved in [Model Protocol Transport](acceptance/agents.md#model-protocol-transport).
