import { createFormView } from "./form/view.js";
import { offlineProvider } from "./agent/offline-provider.js";
import { createAgentChat, element } from "./agent/chat.js";

const form = createFormView({
  input: element<HTMLTextAreaElement>("message"),
  save: element<HTMLButtonElement>("save"), other: element<HTMLButtonElement>("other"),
  saved: element<HTMLPreElement>("saved-value"),
  saveCount: element<HTMLSpanElement>("save-count"), otherCount: element<HTMLSpanElement>("other-count"),
  status: element<HTMLParagraphElement>("status"),
});
createAgentChat({
  application: form,
  offlineProvider,
  offlineHint: "离线模式使用下方任务命令，也支持“保存：文本”。每条指令独立执行。",
  offlineExample: "task fill_and_save:周五完成设计评审",
  modelExample: "读取当前输入框的文本，原样保存，再核对控件",
});
window.addEventListener("pagehide", () => form.dispose(), { once: true });
