import type { ActionContext } from "../bridge.js";

export type ArticleField = "title" | "content";
export interface ArticleState {
  title: string;
  content: string;
  savedTitle: string;
  savedContent: string;
  publishes: number;
  otherClicks: number;
}

export function articleText(value: string): string {
  if (typeof value !== "string" || value.includes("\0") || new TextEncoder().encode(value).length > 4096) {
    throw new Error("invalid article text");
  }
  for (let index = 0; index < value.length; index++) {
    const unit = value.charCodeAt(index);
    if (unit >= 0xd800 && unit <= 0xdbff) {
      const next = value.charCodeAt(++index);
      if (!(next >= 0xdc00 && next <= 0xdfff)) throw new Error("invalid article text");
    } else if (unit >= 0xdc00 && unit <= 0xdfff) throw new Error("invalid article text");
  }
  return value;
}

function next(value: number): number {
  if (!Number.isInteger(value) || value < 0 || value >= 0xffff_ffff) throw new Error("article counter exhausted");
  return value + 1;
}

/** The application owns both drafts, their revisions, and the published pair. */
export class ArticleModel {
  readonly #state: ArticleState = { title: "", content: "", savedTitle: "", savedContent: "", publishes: 0, otherClicks: 0 };
  readonly #revisions = { title: 0, content: 0 };
  readonly #beforePublish: (context: ActionContext) => void | Promise<void>;

  constructor(options: { beforePublish?: (context: ActionContext) => void | Promise<void> } = {}) {
    this.#beforePublish = options.beforePublish ?? (() => Promise.resolve());
  }

  snapshot(): ArticleState { return { ...this.#state }; }

  input(field: ArticleField, text: string): void {
    if (field !== "title" && field !== "content") throw new Error("unknown article field");
    const value = articleText(text), revision = next(this.#revisions[field]);
    this.#state[field] = value;
    this.#revisions[field] = revision;
  }

  other(): void { this.#state.otherClicks = next(this.#state.otherClicks); }

  /** Both drafts must still match after preparation, including edits back to the same text. */
  async publish(context: ActionContext, render: () => void): Promise<void> {
    const draft = this.snapshot(), revisions = { ...this.#revisions };
    await this.#beforePublish(context);
    context.commit(() => {
      if (this.#revisions.title !== revisions.title || this.#revisions.content !== revisions.content
        || this.#state.publishes !== draft.publishes) throw new Error("article changed before publication");
      const publishes = next(this.#state.publishes);
      this.#state.savedTitle = draft.title;
      this.#state.savedContent = draft.content;
      this.#state.publishes = publishes;
      // A later rendering failure retains this publication. There is no retry or rollback.
      render();
    });
  }
}
