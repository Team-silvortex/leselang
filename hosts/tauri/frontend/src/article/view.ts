import { createDynamicRegisteredApplication, type ActionContext, type ControlState } from "../bridge.js";
import { createDomButtonControl, createDomTextControl, type DomControlBinding } from "../dom-bridge.js";
import { ArticleModel, articleText, type ArticleField } from "./model.js";

export interface ArticleEditorElements {
  title: HTMLTextAreaElement;
  content: HTMLTextAreaElement;
  titleLabel: HTMLLabelElement;
  contentLabel: HTMLLabelElement;
  publish: HTMLButtonElement;
  other: HTMLButtonElement;
}

export interface ArticleElements extends ArticleEditorElements {
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
  const names = ["title", "content", "publish", "other"] as const;
  const labels = { title: "Article title", content: "Article content" } as const;
  type Editor = { readonly elements: ArticleEditorElements; readonly bindings: Record<typeof names[number], DomControlBinding> };
  let editor: Editor | null = null;
  let disposed = false;

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
  function requireEditor(owner: ArticleEditorElements): void {
    if (disposed || editor?.elements !== owner) throw new Error("article editor is unavailable");
  }
  function inputState(owner: ArticleEditorElements, field: ArticleField): ControlState {
    requireEditor(owner);
    const input = owner[field], label = owner[field === "title" ? "titleLabel" : "contentLabel"];
    current(input);
    current(label);
    if (input.value !== model.snapshot()[field] || label.textContent !== labels[field]) {
      throw new Error("article input DOM and application state differ");
    }
    return { role: "text_field", title: label.textContent, value: input.value,
      enabled: !input.disabled, editable: !input.readOnly, clicks: 0 };
  }
  function buttonState(owner: ArticleEditorElements, id: "publish" | "other"): ControlState {
    requireEditor(owner);
    const button = owner[id], state = model.snapshot();
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

  const changed = (...names: string[]) => application.changed(...names);
  function fieldControl(owner: ArticleEditorElements, field: ArticleField) {
    return createDomTextControl({
      element: owner[field], read: () => inputState(owner, field), changed,
      validateText: articleText,
      onInput(text, action) {
        try {
          requireEditor(owner);
          current(owner[field]);
          model.input(field, text);
          action.changed(field, "publish");
          status("草稿已更新。发布后可分别核对标题和正文。");
        } catch (error) {
          // This callback still owns its synchronous input commit.
          owner[field].value = model.snapshot()[field];
          throw error;
        }
      },
      onError(_error, origin) {
        if (origin === "manual") {
          // Invalid manual text has no domain effect and restores the prior draft.
          try { requireEditor(owner); current(owner[field]); owner[field].value = model.snapshot()[field]; }
          catch { /* A replaced page cannot restore a draft. */ }
        }
        if (editor?.elements === owner && !disposed) failed();
      },
    });
  }
  function bindEditor(nodes: ArticleEditorElements): Editor {
    // Retained callbacks must never follow a caller's mutable element map.
    const owner: ArticleEditorElements = { title: nodes.title, content: nodes.content,
      titleLabel: nodes.titleLabel, contentLabel: nodes.contentLabel,
      publish: nodes.publish, other: nodes.other };
    const created: DomControlBinding[] = [];
    const keep = (binding: DomControlBinding) => { created.push(binding); return binding; };
    const onError = () => { if (editor?.elements === owner && !disposed) failed(); };
    try {
      const title = keep(fieldControl(owner, "title")), content = keep(fieldControl(owner, "content"));
      const publish = keep(createDomButtonControl({
        element: owner.publish, read: () => buttonState(owner, "publish"), changed,
        beforeCommit() {
          // Keep this particular editor identity across asynchronous preparation.
          for (const field of fields) inputState(owner, field);
          buttonState(owner, "publish");
          buttonState(owner, "other");
        },
        onClick(action) { return model.publish(action, () => {
          render();
          action.changed("publish");
          status("已发布。标题、正文与发布次数已更新。");
        }); }, onError,
      }));
      const other = keep(createDomButtonControl({
        element: owner.other, read: () => buttonState(owner, "other"), changed,
        onClick(action) { action.commit(() => {
          buttonState(owner, "other");
          model.other();
          render();
          action.changed("other");
          status("已点击无关控件。");
        }); }, onError,
      }));
      return { elements: owner, bindings: { title, content, publish, other } };
    } catch (error) {
      for (const binding of created) binding.dispose();
      throw error;
    }
  }
  editor = bindEditor(elements);
  const application = createDynamicRegisteredApplication({
    title: editor.bindings.title.control, content: editor.bindings.content.control,
    publish: editor.bindings.publish.control, other: editor.bindings.other.control,
  });
  function unmountEditor(): void {
    if (disposed) throw new Error("article view is disposed");
    const previous = editor;
    if (!previous) throw new Error("article editor is unavailable");
    // Retire agent targets before removing native nodes or their listeners.
    for (const name of names) application.unmount(name);
    editor = null;
    for (const binding of Object.values(previous.bindings)) binding.dispose();
    status("正在预览草稿。继续编辑后可再次修改或发布。");
  }
  function mountEditor(owner: ArticleEditorElements): void {
    if (disposed) throw new Error("article view is disposed");
    if (editor) throw new Error("article editor is already available");
    const draft = model.snapshot();
    for (const field of fields) { current(owner[field]); owner[field].value = draft[field]; }
    const next = bindEditor(owner);
    const mounted: typeof names[number][] = [];
    editor = next;
    try {
      // Validate independent rendering before publishing any replacement slot.
      for (const field of fields) inputState(next.elements, field);
      buttonState(next.elements, "publish");
      buttonState(next.elements, "other");
      for (const name of names) {
        application.mount(name, next.bindings[name].control);
        mounted.push(name);
      }
      status("可继续编辑。草稿和已发布结果已保留。");
    } catch (error) {
      try { for (const name of mounted.reverse()) application.unmount(name); }
      finally {
        editor = null;
        for (const binding of Object.values(next.bindings)) binding.dispose();
      }
      throw error;
    }
  }
  render();
  return {
    attach: application.attach, detach: application.detach,
    observe: application.observe, changed: application.changed, bindingRevision: application.bindingRevision,
    unmountEditor, mountEditor,
    draft() {
      if (disposed) throw new Error("article view is disposed");
      if (editor) for (const field of fields) inputState(editor.elements, field);
      const { title, content } = model.snapshot();
      return Object.freeze({ title, content });
    },
    dispose() {
      if (disposed) return;
      const previous = editor;
      if (previous) {
        try { unmountEditor(); }
        finally {
          editor = null;
          for (const binding of Object.values(previous.bindings)) binding.dispose();
          disposed = true;
        }
      } else disposed = true;
    },
  };
}
