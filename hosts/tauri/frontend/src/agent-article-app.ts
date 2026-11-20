import { createSemanticProvider } from "./bridge.js";
import { createAgentChat, element } from "./agent/chat.js";
import { createArticleView } from "./article/view.js";

const article = createArticleView({
  title: element<HTMLTextAreaElement>("title"), content: element<HTMLTextAreaElement>("content"),
  titleLabel: element<HTMLLabelElement>("title-label"), contentLabel: element<HTMLLabelElement>("content-label"),
  publish: element<HTMLButtonElement>("publish"), other: element<HTMLButtonElement>("other"),
  savedTitle: element<HTMLPreElement>("saved-title"), savedContent: element<HTMLPreElement>("saved-content"),
  publishCount: element<HTMLSpanElement>("publish-count"), otherCount: element<HTMLSpanElement>("other-count"),
  status: element<HTMLParagraphElement>("status"),
});
createAgentChat({
  application: article,
  offlineProvider: createSemanticProvider(),
  offlineHint: "离线模式使用下方任务命令。发布命令分别接收标题和正文；每条指令独立核对。",
  offlineExample: 'task publish_article:{"title":"周五评审","content":"设计稿已准备完成。"}',
  modelExample: "读取当前标题和正文，把它们原样发布，再核对文章",
});
