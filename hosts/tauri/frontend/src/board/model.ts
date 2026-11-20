import type { ActionContext } from "../bridge.js";

export type BoardTaskId = "review" | "archive";
export interface BoardTask {
  readonly id: BoardTaskId;
  readonly title: string;
  readonly status: "pending" | "done";
  readonly completions: 0 | 1;
}

const titles = { review: "核对示例报告", archive: "整理示例资料" } as const;

/** Application state is independent of the agent and its selected tasks. */
export class BoardModel {
  readonly #tasks: Record<BoardTaskId, BoardTask> = {
    review: { id: "review", title: titles.review, status: "pending", completions: 0 },
    archive: { id: "archive", title: titles.archive, status: "pending", completions: 0 },
  };
  readonly #revisions: Record<BoardTaskId, number> = { review: 0, archive: 0 };

  snapshot(): readonly BoardTask[] {
    return [this.task("review"), this.task("archive")];
  }

  task(id: BoardTaskId): BoardTask {
    if (id !== "review" && id !== "archive") throw new Error("unknown board task");
    return Object.freeze({ ...this.#tasks[id] });
  }

  /** The application's real completion handler crosses a Promise before commit. */
  async complete(id: BoardTaskId, context: ActionContext, publish: () => void): Promise<void> {
    const original = this.task(id);
    if (original.status !== "pending" || original.completions !== 0) throw new Error("board task already completed");
    const revision = this.#revisions[id];
    await Promise.resolve();
    context.commit(() => {
      const current = this.#tasks[id];
      if (this.#revisions[id] !== revision || current.status !== "pending" || current.completions !== 0) {
        throw new Error("board task changed before completion");
      }
      this.#tasks[id] = { ...current, status: "done", completions: 1 };
      this.#revisions[id] = revision + 1;
      // Publication can fail after the domain mutation; no implicit rollback.
      publish();
    });
  }
}
