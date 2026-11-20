import { createSemanticProvider } from "./bridge.js";
import { createAgentChat, element } from "./agent/chat.js";
import { createArticleView, type ArticleEditorElements } from "./article/view.js";

function editorElements(): ArticleEditorElements {
  return {
    title: element<HTMLTextAreaElement>("title"), content: element<HTMLTextAreaElement>("content"),
    titleLabel: element<HTMLLabelElement>("title-label"), contentLabel: element<HTMLLabelElement>("content-label"),
    publish: element<HTMLButtonElement>("publish"), other: element<HTMLButtonElement>("other"),
  };
}
const article = createArticleView({
  ...editorElements(),
  savedTitle: element<HTMLPreElement>("saved-title"), savedContent: element<HTMLPreElement>("saved-content"),
  publishCount: element<HTMLSpanElement>("publish-count"), otherCount: element<HTMLSpanElement>("other-count"),
  status: element<HTMLParagraphElement>("status"),
});
const editor = element<HTMLDivElement>("editor-fields");
const editorTemplate = editor.cloneNode(true) as HTMLDivElement;
const preview = element<HTMLDivElement>("draft-preview");
const previewTitle = element<HTMLPreElement>("draft-preview-title");
const previewContent = element<HTMLPreElement>("draft-preview-content");
const toggle = element<HTMLButtonElement>("editor-toggle");
let editing = true;
toggle.addEventListener("click", () => {
  try {
    if (editing) {
      const draft = article.draft();
      article.unmountEditor();
      editor.replaceChildren();
      previewTitle.textContent = draft.title;
      previewContent.textContent = draft.content;
      editor.hidden = true;
      preview.hidden = false;
      toggle.textContent = "继续编辑";
      editing = false;
    } else {
      const fresh = editorTemplate.cloneNode(true) as HTMLDivElement;
      editor.replaceChildren(...Array.from(fresh.childNodes));
      try { article.mountEditor(editorElements()); }
      catch (error) { editor.replaceChildren(); throw error; }
      editor.hidden = false;
      preview.hidden = true;
      toggle.textContent = "预览草稿";
      editing = true;
    }
  } catch {
    const status = element<HTMLParagraphElement>("status");
    status.textContent = "视图切换未完成。草稿与已发布内容会保留，请检查当前状态。";
    status.dataset.failed = "true";
  }
});
createAgentChat({
  application: article,
  offlineProvider: createSemanticProvider(),
  offlineHint: "离线模式使用下方任务命令。发布命令分别接收标题和正文；每条指令独立核对。",
  offlineExample: 'task publish_article:{"title":"周五评审","content":"设计稿已准备完成。"}',
  modelExample: "读取当前标题和正文，把它们原样发布，再核对文章",
});
window.addEventListener("pagehide", () => article.dispose(), { once: true });
