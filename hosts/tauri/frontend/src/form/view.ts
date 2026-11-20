import { createRegisteredApplication, type ActionContext, type RegisteredActionContext,
  type ControlState } from "../bridge.js";
import { FormModel } from "./model.js";

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
  let inputContext: RegisteredActionContext | undefined;
  let saveContext: RegisteredActionContext | undefined;
  let savePromise: Promise<void> | undefined;
  let otherContext: RegisteredActionContext | undefined;

  function current(node: HTMLElement): void {
    if (!node.isConnected || document.getElementById(node.id) !== node) {
      throw new Error("registered DOM instance has been replaced");
    }
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
  // Manual interactions have their own authority; workflow callbacks always
  // use the particular Bridge context captured when that action was submitted.
  const direct: RegisteredActionContext = {
    signal: new AbortController().signal,
    commit: <T>(mutation: () => T): T => mutation(),
    changed: (...names) => application.changed(...names),
  };
  input.addEventListener("input", () => {
    current(input);
    model.input(input.value);
    (inputContext ?? direct).changed("input", "save");
    setStatus("Text changed. Save to update the result.");
  });
  save.addEventListener("click", () => {
    const action = saveContext ?? direct;
    const guarded: ActionContext = {
      signal: action.signal,
      commit: <T>(mutation: () => T): T => action.commit(() => {
        // The real input and buttons must still match after the Promise boundary.
        inputState();
        buttonState(save, "save");
        buttonState(other, "other");
        return mutation();
      }),
    };
    savePromise = model.save(guarded, () => {
      render();
      action.changed("save");
      setStatus("Saved. The result is available below.");
    });
    void savePromise.catch(() => setStatus("Save did not complete. Observe current state before trying again.", true));
  });
  other.addEventListener("click", () => {
    const action = otherContext ?? direct;
    action.commit(() => {
      current(other);
      model.other();
      render();
      action.changed("other");
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
  const application = createRegisteredApplication({
    input: {
      read: inputState,
      setText(text, context) {
        context.commit(() => {
          current(input);
          input.value = text;
          inputContext = context;
          try { input.dispatchEvent(new Event("input", { bubbles: true })); }
          finally { inputContext = undefined; }
        });
      },
    },
    save: {
      read: () => buttonState(save, "save"),
      async click(context) {
        current(save);
        savePromise = undefined;
        saveContext = context;
        try { save.click(); } finally { saveContext = undefined; }
        if (!savePromise) throw new Error("save handler did not run");
        await savePromise;
      },
    },
    other: {
      read: () => buttonState(other, "other"),
      click(context) {
        current(other);
        otherContext = context;
        try { other.click(); } finally { otherContext = undefined; }
      },
    },
  });
  render();
  return { ...application, setStatus };
}
