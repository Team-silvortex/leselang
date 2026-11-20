# 动态挂载应用控件

`createDynamicRegisteredApplication` 是可选的高级接口，用于编辑器、路由页面和条件
面板重新创建控件的应用。应用预先声明固定的逻辑名称，再显式挂载、替换或卸载各槽位。
agent 使用原有语义目录和 Lese 程序；宿主额外检查控件结构的身份是否延续到验证完成。
静态应用继续使用 `createRegisteredApplication`，其返回对象和行为保持兼容。

## 固定名称与结构版本

```ts
import {
  createDynamicRegisteredApplication,
  type RegisteredControl,
} from "@leselang/bridge";

const application = createDynamicRegisteredApplication({
  draft: null,
  save: null,
});

function mountEditor(draft: RegisteredControl, save: RegisteredControl) {
  application.mount("draft", draft);
  application.mount("save", save);
}

function unmountEditor() {
  application.unmount("draft");
  application.unmount("save");
}
```

返回的 `DynamicRegisteredApplication` 实现原有 `attach`、`detach`、`observe` 和
`changed`，因此可直接传入 `createRegisteredAgentHost`。新增接口如下：

| 方法 | 契约 |
| --- | --- |
| `mount(name, control)` | 为已声明的空槽挂载控件 |
| `replace(name, control)` | 为已挂载槽替换控件，包括可见值完全相同的实例 |
| `unmount(name)` | 卸载已挂载槽，保留其逻辑名称 |
| `bindingRevision()` | 读取本应用生命周期内的结构版本 |

名称在构造时确定，最多 64 个，沿用 Bridge 的 ASCII 名称规则。初始值可以是
`RegisteredControl` 或 `null`。不能在运行时增加名称，也不能覆盖已挂载槽来代替
显式 `replace`。空槽不出现在观察、Bridge 注册或 `gui.list` 中；挂回后仍按最初
声明顺序观察。目录声明任务的能力，并不承诺目标控件此刻存在，执行仍需实际查找。

版本从 0 开始，每次成功的结构操作递增一次。同值替换、卸载后挂回原对象都递增；
普通值更新、`changed`、`attach` 和 `detach` 不递增。版本是非负安全整数，耗尽后
拒绝进一步结构变更。它是应用级的结构身份，和协议中对象的 generation/revision
分开，不加入观察记录、JSON 消息或语义目录。

`changed(...names)` 可以通知已经声明但当前为空的依赖槽；空槽跳过通知，后续挂载
会获得新对象身份。未知名称仍拒绝。agent 回调继续使用其捕获的 `context.changed`
和同步 `context.commit`；结构变化会使旧回调的结构版本失效。

读状态时不得重新挂载或改变注册结构。结构更新也不允许重入。若连接中的 Bridge 更新
失败，当前连接会关闭，声明表和版本不提交这次变化；后续可建立新连接。单次接口调用
有此提交边界，多次 mount/unmount 并非批量事务。应用负责清理部分创建的 DOM 绑定。

## 与 DOM 绑定配合

`@leselang/bridge/dom` 绑定固定的元素实例，不会自动追随相同 ID。界面卸载时先调用
`application.unmount(name)`，再 `binding.dispose()` 并移除 DOM。重新进入界面时，
先创建并插入新元素，为它创建新的 DOM 绑定，再 `application.mount(name, binding.control)`。
替换已挂载槽则显式使用 `replace`，成功后释放旧绑定。失败时释放候选绑定并恢复应用
自己的视图状态。应用仍负责模型、渲染、业务处理器和独立读取。

参考实现 [Article view](../hosts/tauri/frontend/src/article/view.ts) 使用固定四槽。
“预览草稿”卸载全部编辑控件并移除其 DOM；“继续编辑”从模板创建新元素，保留草稿、
发布结果和计数。每代处理器捕获自己的元素，旧异步操作及其错误展示不能写入新界面。
预览按钮属于手工应用导航，本例没有为它声明 agent 任务。

这层不负责框架生命周期，也不自动等待 React、Vue 或其他框架完成渲染。应用应在真实
元素挂载后注册，在卸载时释放，并通过 reader 检查当前 DOM 与模型是否一致。
详见 [DOM 控件指南](dom-controls.md)。

## 执行与独立验证

`RegisteredApplicationConnection` 增加可选同步方法 `bindingRevision(): number`。
动态工厂自动提供它；自定义连接也可提供，并必须保证每次结构变化单调递增、不复用。
宿主捕获方法身份一次，检查每次结果为非负安全整数。静态连接不需要实现此方法。

宿主在执行前保存结构版本，并在初始观察、连接、源码回调、请求进入以及异步回复边界
复查。检测到变化会停止当前尝试和后续 Effect，普通执行错误为
`application_bindings_changed`。没有完整报告时不会虚构验证诊断或成功结果。

这些检查不构成原子撤销：已经交给 Bridge 的当前 Effect 仍可能留下部分副作用，
例如某个无关控件在派发与处理之间被替换。发现变化后不再用成功回复推进当前尝试；
迟到请求仍可收到拒绝回复，已经发生的修改不会撤回。adapter 仍必须在每次异步等待后
的修改处使用 `context.commit`。

若完整原始报告已返回，宿主先核对本次计划和报告的精确关联，再检查结构版本，并在
独立重新观察后再次检查。即使新控件和旧控件的每个可见字段都相同，结构变化也会使
本次验证失败：

```json
{
  "status": "failed",
  "error": "application_verification_failed",
  "verificationFailure": { "code": "application_bindings_changed" }
}
```

上面只列出相关结果字段。原始报告、早先步骤和可能已发生的效果继续保留；完成凭据
仍只消费一次。后续工作流步骤停止。应用可以在状态稳定后明确提交新任务，新任务重新
发现、执行和验证当前实例；SDK 不自动重试、修复计划或重复已完成操作。

结构版本不会增加权限，也不能证明业务结果。原有实时权限、取消、迟到回复、Fuel、
语义报告和独立状态比较仍然生效，Rust VM 继续显式的
`poll → authorize → dispatch → accept`。

相关契约见 [registered host](registered-agent-host.md)、
[验证失败诊断](verification-diagnostics.md) 和 [顺序工作流](agent-workflows.md)。
