import { createSemanticProvider } from "./bridge.js";
import { createAgentChat, element } from "./agent/chat.js";
import { createBoardView } from "./board/view.js";

const task = (id: string) => ({
  title: element<HTMLHeadingElement>(`${id}-title`),
  button: element<HTMLButtonElement>(`${id}-complete`),
  status: element<HTMLSpanElement>(`${id}-state`),
  completions: element<HTMLSpanElement>(`${id}-count`),
});
const board = createBoardView({ review: task("review"), archive: task("archive"),
  status: element<HTMLParagraphElement>("status") });
createAgentChat({
  application: board,
  offlineProvider: createSemanticProvider(),
  offlineHint: "离线模式使用下方任务命令。完成后的任务不会再次执行；每条指令独立核对。",
  offlineExample: "task read_review_status",
  modelExample: "读取报告任务状态，只有它仍是 pending 时才完成，再核对任务板",
});
