# 显式接纳执行失败报告

控件不存在时，宿主可能只有一份失败报告，没有成功执行的 Effects、剩余 Fuel 或
返回值。用成功报告规则读取它，会先得到预算不符，掩盖真正原因。

JavaScript SDK 提供可选的 `executionReports: "v1"` 契约。启用后，验证器先核对
报告身份，再识别终止失败，最后才处理成功报告的预算和结果。失败报告即使附带一套
看似正确的成功字段，也不能通过验证。

## 应用显式启用

```ts
import { createRegisteredAgentHost, prepareSemanticCatalog } from "@leselang/bridge";

const host = createRegisteredAgentHost({
  window: "article", epoch: freshEpoch, application, transport,
  observations: { before: "native_before", after: "native_after" },
  executionReports: "v1",
});

// 独立读取报告时也显式选择同一契约。
const catalog = prepareSemanticCatalog(rawCatalog, { executionReports: "v1" });
const checked = catalog.prepare("read_title", {}).verifyDetailed(rawReport);
```

`SemanticReportOptions` 是根入口导出的配置类型。`RegisteredAgentHostOptions` 接受
同一可选值，返回宿主只在启用时提供只读的 `executionReports` 属性。
`AgentWorkflowSession` 在构造时捕获宿主声明，并把它用于自己的来源结果核对。
被后续输入绑定或前置条件引用的任务失败，也会在继续工作流前得到同样的诊断。
检查顺序和 Boolean/详细验证方法的调用规则不变。

自定义 `AgentHost` 可声明 `readonly executionReports?: "v1"`，并在自己的 verifier
中实施此契约。`AgentSession` 仍使用宿主的核对接口，不额外猜测任意报告格式。
provider 不选择报告契约，目录里的业务字段也不能打开它。

缺省行为完全保留：通用语义报告允许应用自有的额外字段，没有启用此契约时，SDK
不为 `status` 或 `execution_failure` 强加新的终止状态含义。旧目录、成功报告、
生成程序和执行协议帧均无需改变。Tauri 参考对话入口显式启用这个选项。

## 报告形状

成功报告保持既有字段，不添加 `status` 或 `execution_failure`。失败报告沿用其
任务声明的 schema、profile 和 kind，并包含：

```json
{
  "schema_version": 1,
  "kind": "tauri_task_run",
  "profile": "tauri.article",
  "status": "failed",
  "error": "unknown registered object",
  "possible_partial_effects": true,
  "execution_failure": { "code": "execution_object_unavailable" }
}
```

`status` 只能为 `failed` 或 `cancelled`。`error` 必须为非空、最多 4096 UTF-8 字节的文本，
`possible_partial_effects` 必须为 Boolean。它们是原始证据，诊断不会复制异常文本。
`execution_failure` 可省略，以兼容原有简化失败报告；若提供，必须恰好是一个
`code` 字段，且来自以下固定集合：

| 码 | 宿主提供的原因 |
| --- | --- |
| `execution_failed` | 执行失败，当前分类没有更具体的原因 |
| `execution_object_unavailable` | 当前注册清单中没有请求的对象 |
| `execution_fuel_exhausted` | 已识别的宿主 Fuel 错误 |
| `execution_deadline_exceeded` | 宿主执行期限已到 |
| `execution_scope_closed` | 当前页面或执行连接已失效 |
| `execution_cancelled` | 宿主已终止这次执行 |

`failed` 不能携带 `execution_cancelled`；`cancelled` 只能携带这个码。
省略标记时分别得到 `execution_failed` 或 `execution_cancelled`。
标记没有配套状态、未知状态/码、额外标记字段或缺失必需字段均返回 `invalid_report`。
原有有界 JSON、重复键、getter 和精确整数规则仍适用。即使携带 `verified: true`、
正确返回值和完整预算，合法终止失败仍优先拒绝。

报告的 schema/profile/kind 不符优先返回身份诊断。registered host 还先核对计划、
原始报告和一次性完成凭据，再检查控件结构身份；失败标记不能绕过这些边界。
工作流对被引用的来源先做语义拒绝，不会因为错误报告含有一个值就把它绑定到下一步。

## Rust 来源与兼容接口

`leselang-registered-host` 新增 `ExecutionFailure`、`ExecutionFailureCode` 和
`run_prepared_registered_task_detailed`。详细入口保留类型化原因和旧错误文本；原有
`run_prepared_registered_task` 仍返回 `Result<Value, String>`，通过同一实现投影。
Tauri 提供对应的 `run_prepared_task_for_application_detailed`。

这些入口共用现有 VM driver。分类发生在 Rust 仍持有错误类型时：已知的宿主对象
缺失和 Fuel 错误可以分类；任意 `Transport(String)` 不通过文字匹配冒充它们。
Tauri 的取消、期限与页面失效来自已有生命周期标记。其他原因保持一般执行失败，
不会推测不存在的类别。旧公开 `Error` enum 不新增 variant，成功报告保持原样。

Tauri 生成报告时对异常文本施加上述字节边界：空文本使用固定说明，超长文本在字符
边界截断并带截断提示。Rust 详细错误和旧字符串入口仍保留原始文本，返回的原始
JSON 报告则由 session 原样保存。语言 VM 中尚未归入具体类别的错误仍使用一般失败。

Node 应用可在 `createNodeAgentTransport` 上另行显式设置 `executionReports: "v1"`，
选择 worker 协议 v3，同时在 registered host 上启用上述报告契约。v3 在任务成功
接纳之后，可通过仍然有效的连接发送真实执行失败报告；它使用固定的
`javascript.registered` / `javascript_task_run` 身份，失败码来自同一个 Rust 详细入口。
控件缺失可以分类，JavaScript handler 异常仍为一般失败，不按异常文本猜测原因。

成功报告保持原样；失败报告不补造结果、Effect 数、Fuel 或完成后的观察。
启动校验失败、断管、载体期限和进程异常仍走原有异常路径，没有完成报告就不生成
报告。载体必须等到子进程正常关闭后才交付报告；后续验证失败会停止工作流。
省略 transport 选项继续使用 worker v1/v2，包括原有错误帧和异常。完整配置、参数
模式和边界见 [Node 嵌入指南](node-agent-host.md#worker-protocol-and-bounds)。

## 失败后的行为

详细验证结果沿用 `{ verified: false, diagnostic: { code } }`。session/workflow
保留原始报告，结果为 `failed`，原有 `error` 仍为 `application_verification_failed`。
界面根据固定码展示执行原因；原始异常留在证据区。后续步骤停止，已有副作用保留，
没有自动重试、回滚或恢复。

远端报告的 `cancelled` 不等于用户取消本地会话。本地没有取消信号时，它仍是一次
失败，带 `execution_cancelled` 诊断；本地确实取消时，原有取消优先和迟到结果拒绝
规则不变。报告声称 `possible_partial_effects: false` 也不能清除 session 已进入
执行后的保守标记。

有关其他核对失败，见[验证诊断](verification-diagnostics.md)；控件重建的结构检查
见[动态控件](dynamic-controls.md)。
