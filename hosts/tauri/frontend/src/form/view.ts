import { createRegisteredApplication, type ControlState } from "../bridge.js";
import { createDomButtonControl, createDomTextControl } from "../dom-bridge.js";
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
  const changed = (...names: string[]) => application.changed(...names);
  const inputBinding = createDomTextControl({
    element: input, read: inputState, changed,
    onInput(text, action) {
      current(input);
      model.input(text);
      action.changed("input", "save");
      setStatus("Text changed. Save to update the result.");
    },
  });
  const saveBinding = createDomButtonControl({
    element: save, read: () => buttonState(save, "save"), changed,
    beforeCommit() {
      // Recheck application state after the Promise boundary.
      inputState();
      buttonState(save, "save");
      buttonState(other, "other");
    },
    onClick(action) {
      return model.save(action, () => {
        render();
        action.changed("save");
        setStatus("Saved. The result is available below.");
      });
    },
    onError() { setStatus("Save did not complete. Observe current state before trying again.", true); },
  });
  const otherBinding = createDomButtonControl({
    element: other, read: () => buttonState(other, "other"), changed,
    onClick(action) {
      action.commit(() => {
        current(other);
        model.other();
        render();
        action.changed("other");
        setStatus("The other control was clicked.");
      });
    },
  });
  const application = createRegisteredApplication({
    input: inputBinding.control, save: saveBinding.control, other: otherBinding.control,
  });
  render();
  return { ...application, setStatus,
    dispose() { inputBinding.dispose(); saveBinding.dispose(); otherBinding.dispose(); } };
}
