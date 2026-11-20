import { createRegisteredApplication, createDynamicRegisteredApplication } from "@leselang/bridge";
import { createDomButtonControl, createDomTextControl } from "@leselang/bridge/dom";

// This is an EventTarget fixture, not a browser or a WebView emulator. It uses
// the public DOM helpers unchanged and leaves real browser behavior to Tauri.
class ElementFixture extends EventTarget {
  constructor(id, tagName, ownerDocument) {
    super();
    this.id = id; this.tagName = tagName; this.ownerDocument = ownerDocument;
  }
  type = "button";
  isConnected = true;
  disabled = false;
  readOnly = false;
  value = "";
  textContent = "";
  click() { if (!this.disabled) this.dispatchEvent(new Event("click", { bubbles: true })); }
}

/** An independent DOM-backed application with the existing Article catalog. */
export function createDomArticleApplication(options = {}) {
  const nodes = new Map();
  const document = { defaultView: { Event }, getElementById: (id) => nodes.get(id) ?? null };
  const elements = Object.fromEntries([
    ["title", "TEXTAREA"], ["content", "TEXTAREA"], ["publish", "BUTTON"], ["other", "BUTTON"],
    ["savedTitle", "PRE"], ["savedContent", "PRE"], ["publishCount", "SPAN"], ["otherCount", "SPAN"],
  ].map(([id, tag]) => {
    const node = new ElementFixture(id, tag, document); nodes.set(id, node); return [id, node];
  }));
  const state = { title: options.title ?? "", content: options.content ?? "", savedTitle: "", savedContent: "",
    publishes: 0, otherClicks: 0 };
  const events = { title: 0, content: 0, publish: 0, other: 0 };
  const errors = [];
  elements.title.value = state.title; elements.content.value = state.content;
  elements.publish.textContent = "Publish article"; elements.other.textContent = "Unrelated action";
  let application;
  const changed = (...names) => application.changed(...names);
  function current(element) {
    if (!element.isConnected || nodes.get(element.id) !== element) throw new Error("fixture DOM instance replaced");
  }
  function render() {
    elements.savedTitle.textContent = state.savedTitle;
    elements.savedContent.textContent = state.savedContent;
    elements.publishCount.textContent = String(state.publishes);
    elements.otherCount.textContent = String(state.otherClicks);
  }
  function inputState(name) {
    current(elements[name]);
    if (elements[name].value !== state[name]) throw new Error("fixture input DOM and model differ");
    return { role: "text_field", title: `Article ${name}`, value: elements[name].value,
      enabled: !elements[name].disabled, editable: !elements[name].readOnly, clicks: 0 };
  }
  function buttonState(name) {
    for (const element of [elements[name], elements.savedTitle, elements.savedContent, elements.publishCount, elements.otherCount]) current(element);
    if (elements.savedTitle.textContent !== state.savedTitle || elements.savedContent.textContent !== state.savedContent
      || elements.publishCount.textContent !== String(state.publishes) || elements.otherCount.textContent !== String(state.otherClicks)) {
      throw new Error("fixture publication DOM and model differ");
    }
    return { role: "button", title: name === "publish" && state.publishes ? elements.savedTitle.textContent : elements[name].textContent,
      value: name === "publish" ? elements.savedContent.textContent : "", enabled: !elements[name].disabled,
      editable: false, clicks: name === "publish" ? state.publishes : state.otherClicks };
  }
  function beforeCommit() {
    inputState("title"); inputState("content"); buttonState("publish"); buttonState("other");
  }
  const onError = (error, origin) => errors.push({ error, origin });
  const fields = Object.fromEntries(["title", "content"].map((name) => [name, createDomTextControl({
    element: elements[name], read: () => inputState(name), changed,
    validateText(text) {
      if (text.includes("\0") || (name === "content" && options.rejectContent)) throw new Error("content rejected");
    },
    onInput(text, context) {
      events[name]++;
      state[name] = text;
      context.changed(name, "publish");
    }, onError,
  })]));
  const makePublish = () => createDomButtonControl({ element: elements.publish, read: () => buttonState("publish"), changed,
    beforeCommit, onError,
    async onClick(context) {
      events.publish++;
      await options.beforePublish?.(context);
      context.commit(() => {
        state.savedTitle = state.title; state.savedContent = state.content; state.publishes++;
        render(); context.changed("publish");
      });
      if (options.faultAfterCommit) throw new Error("failure after fixture publication");
    },
  });
  let publish = makePublish();
  const other = createDomButtonControl({ element: elements.other, read: () => buttonState("other"), changed,
    beforeCommit, onError,
    onClick(context) {
      events.other++;
      context.commit(() => { state.otherClicks++; render(); context.changed("other"); });
    },
  });
  const bindings = new Set([fields.title, fields.content, publish, other]);
  const createApplication = options.dynamic ? createDynamicRegisteredApplication : createRegisteredApplication;
  application = createApplication({ title: fields.title.control, content: fields.content.control,
    publish: publish.control, other: other.control });
  render();
  function mountPublish(replacing) {
    if (!options.dynamic) throw new Error("dynamic fixture required");
    const previous = elements.publish;
    const next = new ElementFixture("publish", "BUTTON", document);
    next.textContent = "Publish article";
    elements.publish = next; nodes.set("publish", next);
    const binding = makePublish();
    try {
      if (replacing) application.replace("publish", binding.control);
      else application.mount("publish", binding.control);
    } catch (error) {
      binding.dispose(); elements.publish = previous;
      if (previous.isConnected) nodes.set("publish", previous); else nodes.delete("publish");
      throw error;
    }
    previous.isConnected = false;
    publish.dispose(); bindings.delete(publish);
    publish = binding; bindings.add(binding);
  }
  return { ...application, elements,
    snapshot: () => ({ ...state }), eventCounts: () => ({ ...events }), errors: () => [...errors],
    manualInput(name, value) {
      elements[name].value = value;
      elements[name].dispatchEvent(new Event("input", { bubbles: true }));
    },
    manualClick(name) { elements[name].click(); },
    replaceNode(name) {
      const previous = elements[name]; previous.isConnected = false;
      nodes.set(previous.id, new ElementFixture(previous.id, previous.tagName, document));
    },
    ...(options.dynamic ? {
      remountPublish() { mountPublish(true); },
      unmountPublish() {
        application.unmount("publish"); publish.dispose(); bindings.delete(publish);
        elements.publish.isConnected = false; nodes.delete("publish");
      },
      mountPublish() { mountPublish(false); },
    } : {}),
    dispose() { for (const binding of bindings) binding.dispose(); },
  };
}

/** Same EventTarget fixture, opting into fixed logical slots and identity checks. */
export function createDynamicDomArticleApplication(options = {}) {
  return createDomArticleApplication({ ...options, dynamic: true });
}
