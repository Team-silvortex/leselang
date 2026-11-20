import { createRegisteredApplication, type ActionContext, type RegisteredActionContext,
  type RegisteredControl, type ControlState } from "../bridge.js";
import { BoardModel, type BoardTaskId } from "./model.js";

export interface BoardTaskElements {
  readonly title: HTMLHeadingElement;
  readonly button: HTMLButtonElement;
  readonly status: HTMLSpanElement;
  readonly completions: HTMLSpanElement;
}

export interface BoardElements {
  readonly review: BoardTaskElements;
  readonly archive: BoardTaskElements;
  readonly status: HTMLParagraphElement;
}

/** Manual and agent actions call the same real application event handler. */
export function createBoardView(elements: BoardElements) {
  const model = new BoardModel();
  const ids = ["review", "archive"] as const;
  const contexts: Partial<Record<BoardTaskId, RegisteredActionContext>> = {};
  const pending: Partial<Record<BoardTaskId, Promise<void>>> = {};
  const direct: RegisteredActionContext = {
    signal: new AbortController().signal,
    commit: <T>(mutation: () => T): T => mutation(),
    changed: (...names) => application.changed(...names),
  };

  function current(node: HTMLElement): void {
    if (!node.isConnected || document.getElementById(node.id) !== node) {
      throw new Error("registered board DOM instance has been replaced");
    }
  }

  function connected(id: BoardTaskId): void {
    const nodes = elements[id];
    for (const node of [nodes.title, nodes.button, nodes.status, nodes.completions]) current(node);
    current(elements.status);
  }

  function status(message: string, failed = false): void {
    current(elements.status);
    elements.status.textContent = message;
    elements.status.dataset.failed = String(failed);
  }

  function render(id: BoardTaskId): void {
    connected(id);
    const task = model.task(id), nodes = elements[id];
    nodes.title.textContent = task.title;
    nodes.status.textContent = task.status;
    nodes.completions.textContent = String(task.completions);
    nodes.button.disabled = task.status === "done";
    nodes.button.textContent = task.status === "done" ? "已完成" : "标记完成";
  }

  function state(id: BoardTaskId): ControlState {
    connected(id);
    const task = model.task(id), nodes = elements[id];
    if (nodes.title.textContent !== task.title || nodes.status.textContent !== task.status
      || nodes.completions.textContent !== String(task.completions)
      || nodes.button.disabled !== (task.status === "done")
      || nodes.button.textContent !== (task.status === "done" ? "已完成" : "标记完成")) {
      throw new Error("board DOM and application state differ");
    }
    return { role: "button", title: task.title, value: task.status,
      enabled: task.status === "pending", editable: false, clicks: task.completions };
  }

  for (const id of ids) {
    render(id);
    elements[id].button.addEventListener("click", () => {
      const action = contexts[id] ?? direct;
      const guarded: ActionContext = {
        signal: action.signal,
        commit: <T>(mutation: () => T): T => action.commit(() => {
          // Recheck the real instances and all public observations after the await.
          for (const other of ids) state(other);
          return mutation();
        }),
      };
      pending[id] = model.complete(id, guarded, () => {
        render(id);
        action.changed(id);
        status(`已完成「${model.task(id).title}」。另一条任务保持原状态。`);
      });
      void pending[id]!.catch(() => {
        try { status("任务未完成。请核对当前状态；不会自动重试。", true); }
        catch { /* A replaced page cannot publish task completion. */ }
      });
    });
  }

  function control(id: BoardTaskId): RegisteredControl {
    return {
      read: () => state(id),
      async click(context) {
        state(id);
        pending[id] = undefined;
        contexts[id] = context;
        try { elements[id].button.click(); } finally { contexts[id] = undefined; }
        const completion = pending[id];
        if (!completion) throw new Error("board completion handler did not run");
        await completion;
      },
    };
  }
  const application = createRegisteredApplication({ review: control("review"), archive: control("archive") });
  return application;
}
