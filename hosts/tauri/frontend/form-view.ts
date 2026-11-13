import { Bridge, type ActionContext, type ControlState } from "./bridge.js";
import { FormModel } from "./form.js";

export interface FormElements {
  input: HTMLTextAreaElement;
  save: HTMLButtonElement;
  other: HTMLButtonElement;
  saved: HTMLPreElement;
  saveCount: HTMLSpanElement;
  otherCount: HTMLSpanElement;
  status: HTMLParagraphElement;
}

/** One application model and real DOM handlers can serve successive workflows. */
export function createFormView(elements: FormElements) {
  const { input, save, other, saved, saveCount, otherCount, status } = elements;
  const model = new FormModel();
  let activeBridge: Bridge | null = null;
  let saveContext: ActionContext | undefined;
  let savePromise: Promise<void> | undefined;
  let otherContext: ActionContext | undefined;

  function current(node: HTMLElement): void {
    if (!node.isConnected || document.getElementById(node.id) !== node) {
      throw new Error("registered DOM instance has been replaced");
    }
  }
  function changed(name: string): void {
    activeBridge?.changed(name);
  }
  function setStatus(message: string, failed = false): void {
    status.textContent = message;
    status.dataset.failed = String(failed);
  }
  function render(): void {
    current(saved);
    current(saveCount);
    current(otherCount);
    const state = model.snapshot();
    saved.textContent = state.saved;
    saveCount.textContent = `${state.saveClicks} save${state.saveClicks === 1 ? "" : "s"}`;
    otherCount.textContent = String(state.otherClicks);
  }
  function publishSave(): void {
    render();
    changed("save");
    setStatus("Saved. The result is available below.");
  }
  // Manual interactions have their own authority; workflow callbacks always
  // use the particular Bridge context captured when that action was submitted.
  const direct: ActionContext = {
    signal: new AbortController().signal,
    commit: <T>(mutation: () => T): T => mutation(),
  };
  input.addEventListener("input", () => {
    current(input);
    model.input(input.value);
    changed("input");
    changed("save");
    setStatus("Text changed. Save to update the result.");
  });
  save.addEventListener("click", () => {
    savePromise = model.save(saveContext ?? direct, publishSave);
    void savePromise.catch(() => setStatus("Save did not complete. Observe current state before trying again.", true));
  });
  other.addEventListener("click", () => {
    (otherContext ?? direct).commit(() => {
      current(other);
      model.other();
      render();
      changed("other");
      setStatus("The other control was clicked.");
    });
  });

  function inputState(): ControlState {
    current(input);
    if (input.value !== model.snapshot().draft) throw new Error("input DOM and application state differ");
    return {
      role: "text_field", title: "Text to save", value: input.value,
      enabled: !input.disabled, editable: !input.readOnly, clicks: 0,
    };
  }
  function buttonState(button: HTMLButtonElement, name: "save" | "other"): ControlState {
    current(button);
    current(saved);
    const state = model.snapshot();
    if (saved.textContent !== state.saved) throw new Error("saved DOM and application state differ");
    return {
      role: "button", title: button.textContent ?? "", value: name === "save" ? state.saved : "",
      enabled: !button.disabled, editable: false,
      clicks: name === "save" ? state.saveClicks : state.otherClicks,
    };
  }
  function observe() {
    return [
      { identifier: "save", ...buttonState(save, "save") },
      { identifier: "other", ...buttonState(other, "other") },
      { identifier: "input", ...inputState() },
    ];
  }
  function detach(bridge: Bridge): void {
    bridge.close();
    if (activeBridge === bridge) activeBridge = null;
  }
  function attach(bridge: Bridge): void {
    if (activeBridge) throw new Error("a form workflow is already attached");
    bridge.register("input", {
      read: inputState,
      setText(text, context) {
        context.commit(() => {
          current(input);
          input.value = text;
          input.dispatchEvent(new Event("input", { bubbles: true }));
        });
      },
    });
    bridge.register("save", {
      read: () => buttonState(save, "save"),
      async click(context) {
        current(save);
        savePromise = undefined;
        saveContext = context;
        try { save.click(); } finally { saveContext = undefined; }
        if (!savePromise) throw new Error("save handler did not run");
        await savePromise;
      },
    });
    bridge.register("other", {
      read: () => buttonState(other, "other"),
      click(context) {
        current(other);
        otherContext = context;
        try { other.click(); } finally { otherContext = undefined; }
      },
    });
    activeBridge = bridge;
  }
  render();
  return { attach, detach, observe, setStatus };
}
