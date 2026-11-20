# 验证失败诊断

JavaScript SDK 可以在验证失败时返回有界的结构化原因，供应用解释哪个检查没有通过。
诊断覆盖语义报告核对、registered host 的本次执行凭据和独立应用状态核对。
它沿用现有的成功、失败、取消状态，不改变任务执行、授权或工作流的推进条件。

公共入口 `@leselang/bridge` 导出 `VerificationFailureCode`、
`VerificationDiagnostic` 和 `VerificationResult`：

```ts
interface VerificationDiagnostic {
  readonly code: VerificationFailureCode;
  readonly predicate?: string;
}

type VerificationResult =
  | { readonly verified: true }
  | { readonly verified: false; readonly diagnostic: VerificationDiagnostic };
```

`code` 来自下文列出的固定集合。`predicate` 只用于 `predicate_failed` 和
`predicate_evidence_invalid`，这两类诊断必须携带格式合法的检查 ID。
语义 verifier 和 registered host 从已经接纳的任务声明中取得该 ID。
通用自定义 `AgentHost` 可以使用非语义目录；SDK 只验证 ID 格式和保留名称限制，
不会替自定义宿主确认该 ID 是否属于某个已声明目录。
诊断不携带报告路径、输入内容、期望值、实际值或异常文本，也不接受其他字段。
返回记录由 SDK 持有并冻结。

## 语义报告与宿主接口

`prepareSemanticCatalog(rawCatalog).prepare(task, arguments)` 返回的
`PreparedSemanticTask` 同时提供两个入口：

```ts
import { prepareSemanticCatalog, type VerificationResult } from "@leselang/bridge";

const prepared = prepareSemanticCatalog(rawCatalog).prepare("save_message", {
  message: "待保存内容",
});

const accepted: boolean = prepared.verify(rawReport);
const checked: VerificationResult = prepared.verifyDetailed(rawReport);
```

语义任务的这两个函数只检查所给报告和已经接纳的声明，没有执行副作用。
`verify(report)` 保留原来的 Boolean 结果；`verifyDetailed(report)` 给出同一判断的
结构化结果。它不会从报告声明中推导新的权限，也不替代宿主独立读取应用状态。
含有 `u64` 的报告仍应传入原始 JSON 文本，沿用有界、无损和拒绝重复键的解码规则。

`AgentHost.verify` 仍为必需接口。应用可以额外实现以下可选方法：

```ts
verifyDetailed?(
  report: AgentJson,
  plan: WorkflowPlan,
  signal: AbortSignal,
): VerificationResult | Promise<VerificationResult>;
```

如果提供了该方法，`AgentSession` 只调用它一次，不再调用 Boolean 方法作为第二次核对。
没有提供时，session 继续使用 `verify`，只有严格的 `true` 能通过；旧宿主的
`false` 仍得到 `application_verification_failed`，不会凭空增加诊断字段。
`createRegisteredAgentHost` 已提供详细入口，其 Boolean 方法投影同一次详细核对的结果。

详细入口接受上述同步精确记录，或当前 JavaScript realm 的真正 `Promise`；
SDK 会拒绝直接返回的任意 thenable，不读取其 `then` getter 或等待它。
未知错误码、额外字段、getter、错误的检查 ID、
`{ verified: false }`，以及直接返回 `true` 或 `false`，都不能成为有效详细结果。
session 将这些情况归为 `invalid_verification_result` 并停止。
执行或回调抛出的普通异常仍沿用原来的错误通道，因此并非每个失败都会带有诊断。
取消优先于迟到的验证结果，最终取消状态不带失败诊断。

## 错误码分组

语义报告核对按报告解码、身份、资源记录、宿主核对标记、返回值、返回值证据和声明检查
的顺序进行，返回首先遇到的失败。显式启用 `executionReports: "v1"` 时，身份检查
后先接纳终止失败报告，再处理成功预算和结果。缺省行为不为应用自有的 `status`
字段增加含义，见[执行失败报告](execution-reports.md)。

| 报告身份与基础记录 | 含义 |
| --- | --- |
| `invalid_report` | 报告无法按有界 JSON 和报告字段约束读取 |
| `report_identity_mismatch` | 报告 schema、profile 或 kind 与接纳的任务不符 |
| `report_budget_mismatch` | 报告的 Effects 或剩余 Fuel 不符合声明 |
| `host_verification_missing` | 声明要求宿主核对标记，但报告未给出严格的 `verified: true` |

| 显式启用的执行失败报告 | 含义 |
| --- | --- |
| `execution_failed` | 宿主报告执行失败，未提供更具体的已知类别 |
| `execution_object_unavailable` | 请求的对象不在当前注册清单中 |
| `execution_fuel_exhausted` | 宿主识别到 Fuel 耗尽 |
| `execution_deadline_exceeded` | 宿主执行期限已到 |
| `execution_scope_closed` | 当前页面或执行连接已失效 |
| `execution_cancelled` | 远端执行已终止；不代替本地取消信号 |

| 返回值与声明检查 | 含义 |
| --- | --- |
| `result_mismatch` | 返回值类型、固定值或 observed result 的值格式不符 |
| `result_evidence_invalid` | observed result 选中的独立报告证据缺失、含糊或类型无效 |
| `result_evidence_mismatch` | 返回值与独立报告证据不相等 |
| `predicate_evidence_invalid` | 无法读取或计算某个声明检查的证据；携带该检查的 `predicate` ID |
| `predicate_failed` | 声明检查已完成比较，但条件不满足；携带该检查的 `predicate` ID |

registered host 先确认完成凭据与本次计划、原始报告对应，再核对可选的控件结构版本、
已经计算的语义结果和独立应用观察。重新观察后还会再次检查结构版本。
报告相关性、控件身份和执行后再次读取的可见状态是不同检查：

| 本次执行与独立观察 | 含义 |
| --- | --- |
| `completion_unavailable` | 当前没有可消费的完成凭据，例如尚未完成或凭据已被使用 |
| `plan_mismatch` | 待核对计划与该完成凭据所属计划不符 |
| `report_mismatch` | 待核对的原始报告与本次执行返回的报告不符 |
| `application_bindings_changed` | 提供结构版本的应用在执行至验证期间挂载、替换或卸载控件，即使可见值完全相同 |
| `observation_invalid` | 无法取得或规范化独立应用观察，或报告观察记录无效 |
| `before_observation_mismatch` | 报告中的执行前记录与宿主执行前独立读取的状态不一致 |
| `after_observation_mismatch` | 报告中的执行后记录与验证时再次读取的应用状态不一致 |
| `verification_interrupted` | 核对时宿主已关闭，或本次执行的有效生命周期已经失效 |

| 详细接口契约 | 含义 |
| --- | --- |
| `invalid_verification_result` | 自定义宿主没有返回合法的详细验证记录 |

例如，`predicate_failed` 配合 `predicate: "content_saved"` 表示应用声明的
`content_saved` 检查未通过。诊断只说明这一检查的结果，不包含正文或其期望值。
`after_observation_mismatch` 表示报告与验证时的当前状态不同；即使报告内部的
声明检查全部通过，应用仍不能将该次执行标记为已验证。

`application_bindings_changed` 检查来自应用可选的 `bindingRevision()`，并不改变
报告或观察格式。它也可能作为执行错误出现：若结构变化在请求/回复边界被发现，宿主
停止本次执行，没有完整报告时不附加验证诊断。已有完整报告时保留报告，并按上述
详细验证路径拒绝成功。两种情况都保留可能已发生的效果，见[动态控件](dynamic-controls.md)。

## 单次完成凭据与停止行为

registered host 只在执行和清理都完成后发布完成凭据。核对取出该凭据时立即清除它；
计划不符、报告不符或其他验证失败都不会恢复凭据。每次执行应只选择
`verify` 或 `verifyDetailed` 其中一个入口调用一次。先调用 Boolean 入口，再为取得
原因调用详细入口，会因为凭据已消费得到 `completion_unavailable`。
后续的新目录描述、新执行或关闭宿主也不会让旧凭据再次生效。

session 通过详细入口取得失败时，仍使用 `application_verification_failed` 作为
原有 `error`，并在失败的 `AgentOutcome`、最终 `AgentState` 上附加可选
`verificationFailure: VerificationDiagnostic`。该字段不会出现在成功或取消结果上。

`AgentWorkflowSession` 保留失败步骤的诊断，并将其传播到整体
`AgentWorkflowOutcome` 和失败的工作流状态。需要绑定或前置条件的来源结果仍须
通过语义核对和宿主核对。后续步骤不执行，早先报告和可能已经发生的效果保留，
`possiblePartialEffects` 的含义不变。旧 Boolean 宿主、没有诊断的普通错误以及
`workflow_requirement_not_met` 等工作流前置条件错误保留原有行为。

诊断不会触发自动重试、回滚、补偿或从某一步恢复。再次提交请求是一轮新的执行，
可能重复先前副作用；它需要应用根据当前状态作出明确决定。

## 展示与权限边界

Tauri 工作流展示在失败步骤和整体结果中给出简短中文解释。声明检查只显示检查 ID，
不把报告里的返回值当成成功，也不把诊断中的任意文本直接作为说明。
未开始的后续步骤和部分效果说明继续保留。无效诊断只能显示无法取得有效核对结论，
不能提升成功状态；成功和取消结果忽略多余的失败诊断。

原始报告仍作为独立证据保留。诊断 JSON 可以放在证据区，但只含错误码和允许的检查 ID，
不会复制报告字段或推测不存在的报告。原始报告本身的现有内容不由诊断接口改写或脱敏。
面向用户的说明应将错误码映射为可理解的原因，而不是只展示内部码。

诊断并不授权修复。它不会让 provider 改写任务、降低声明检查、替换报告、跳过失败步骤
或扩大对象权限。后续操作仍需走应用已有的任务声明、计划接纳、实时授权和独立验证。
模型 provider 的生产接口不因诊断增加修复计划或重试协议。

相关实现位于 [`verification.ts`](../packages/javascript/src/verification.ts)、
[`semantics/report.ts`](../packages/javascript/src/semantics/report.ts) 和
[`agent/registered-host.ts`](../packages/javascript/src/agent/registered-host.ts)。
工作流和 Node 应用组合方式见[顺序工作流](agent-workflows.md)与
[Node 应用宿主](node-agent-host.md)。
