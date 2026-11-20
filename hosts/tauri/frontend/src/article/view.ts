import { createRegisteredApplication, type ActionContext, type RegisteredActionContext,
  type RegisteredControl, type ControlState } from "../bridge.js";
import { ArticleModel, articleText, type ArticleField } from "./model.js";

export interface ArticleElements {
  title: HTMLTextAreaElement;
  content: HTMLTextAreaElement;
  titleLabel: HTMLLabelElement;
  contentLabel: HTMLLabelElement;
  publish: HTMLButtonElement;
  other: HTMLButtonElement;
  savedTitle: HTMLPreElement;
  savedContent: HTMLPreElement;
  publishCount: HTMLSpanElement;
  otherCount: HTMLSpanElement;
  status: HTMLParagraphElement;
}

/** Real DOM events and bridge actions share the same application handlers. */
export function createArticleView(elements: ArticleElements, options: {
  beforePublish?: (context: ActionContext) => void | Promise<void>;
} = {}) {
  const model = new ArticleModel(options);
  const fields = ["title", "content"] as const;
  const labels = { title: "Article title", content: "Article content" } as const;
  const contexts: Partial<Record<ArticleField | "publish" | "other", RegisteredActionContext>> = {};
  let inputFailure: unknown;
  let otherFailure: unknown;
  let publishPromise: Promise<void> | undefined;
  const direct: RegisteredActionContext = {
    signal: new AbortController().signal,
    commit: <T>(mutation: () => T): T => mutation(),
    changed: (...names) => application.changed(...names),
  };

  function current(node: HTMLElement): void {
    if (!node.isConnected || document.getElementById(node.id) !== node) {
      throw new Error("registered article DOM instance has been replaced");
    }
  }
  function status(message: string, failed = false): void {
    current(elements.status);
    elements.status.textContent = message;
    elements.status.dataset.failed = String(failed);
  }
  function failed(): void {
    try { status("操作未完成。请核对保留的草稿和发布结果；不会自动重试。", true); }
    catch { /* A replaced page cannot publish a status message. */ }
  }
  function render(): void {
    for (const node of [elements.savedTitle, elements.savedContent, elements.publishCount, elements.otherCount]) current(node);
    const state = model.snapshot();
    elements.savedTitle.textContent = state.savedTitle;
    elements.savedContent.textContent = state.savedContent;
    elements.publishCount.textContent = String(state.publishes);
    elements.otherCount.textContent = String(state.otherClicks);
  }
  function inputState(field: ArticleField): ControlState {
    const input = elements[field], label = elements[field === "title" ? "titleLabel" : "contentLabel"];
    current(input);
    current(label);
    if (input.value !== model.snapshot()[field] || label.textContent !== labels[field]) {
      throw new Error("article input DOM and application state differ");
    }
    return { role: "text_field", title: label.textContent, value: input.value,
      enabled: !input.disabled, editable: !input.readOnly, clicks: 0 };
  }
  function buttonState(id: "publish" | "other"): ControlState {
    const button = elements[id], state = model.snapshot();
    for (const node of [button, elements.savedTitle, elements.savedContent, elements.publishCount, elements.otherCount]) current(node);
    if (elements.savedTitle.textContent !== state.savedTitle || elements.savedContent.textContent !== state.savedContent
      || elements.publishCount.textContent !== String(state.publishes) || elements.otherCount.textContent !== String(state.otherClicks)
      || button.textContent !== (id === "publish" ? "Publish article" : "Unrelated action")) {
      throw new Error("article result DOM and application state differ");
    }
    return { role: "button", title: id === "publish" && state.publishes > 0
      ? elements.savedTitle.textContent : button.textContent,
    value: id === "publish" ? elements.savedContent.textContent : "",
    enabled: !button.disabled, editable: false,
    clicks: Number(id === "publish" ? elements.publishCount.textContent : elements.otherCount.textContent) };
  }

  for (const field of fields) {
    elements[field].addEventListener("input", () => {
      const action = contexts[field] ?? direct;
      const mutate = () => {
        current(elements[field]);
        model.input(field, elements[field].value);
        action.changed(field, "publish");
        status("草稿已更新。发布后可分别核对标题和正文。");
      };
      try {
        // Agent input dispatch already runs inside its authorized commit.
        if (contexts[field]) mutate(); else action.commit(mutate);
      } catch (error) {
        inputFailure = error;
        // Invalid manual text has no domain effect and restores the prior draft.
        elements[field].value = model.snapshot()[field];
        failed();
      }
    });
  }
  elements.publish.addEventListener("click", () => {
    const action = contexts.publish ?? direct;
    const guarded: ActionContext = {
      signal: action.signal,
      commit: <T>(mutation: () => T): T => action.commit(() => {
        // Check all real instances and readback after the asynchronous preparation.
        for (const field of fields) inputState(field);
        buttonState("publish");
        buttonState("other");
        return mutation();
      }),
    };
    publishPromise = model.publish(guarded, () => {
      render();
      action.changed("publish");
      status("已发布。标题、正文与发布次数已更新。");
    });
    void publishPromise.catch(failed);
  });
  elements.other.addEventListener("click", () => {
    const action = contexts.other ?? direct;
    try {
      action.commit(() => {
        buttonState("other");
        model.other();
        render();
        action.changed("other");
        status("已点击无关控件。");
      });
    } catch (error) { otherFailure = error; failed(); }
  });

  function fieldControl(field: ArticleField): RegisteredControl {
    return { read: () => inputState(field), setText(text, context) {
      articleText(text);
      context.commit(() => {
        inputState(field);
        elements[field].value = text;
        inputFailure = undefined;
        contexts[field] = context;
        try { elements[field].dispatchEvent(new Event("input", { bubbles: true })); }
        finally { contexts[field] = undefined; }
        if (inputFailure !== undefined) throw inputFailure;
      });
    } };
  }
  const application = createRegisteredApplication({
    title: fieldControl("title"), content: fieldControl("content"),
    publish: { read: () => buttonState("publish"), async click(context) {
      buttonState("publish");
      publishPromise = undefined;
      contexts.publish = context;
      try { elements.publish.click(); } finally { contexts.publish = undefined; }
      if (!publishPromise) throw new Error("article publication handler did not run");
      await publishPromise;
    } },
    other: { read: () => buttonState("other"), click(context) {
      buttonState("other");
      otherFailure = undefined;
      contexts.other = context;
      try { elements.other.click(); } finally { contexts.other = undefined; }
      if (otherFailure !== undefined) throw otherFailure;
    } },
  });
  render();
  return application;
}
