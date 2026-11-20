import type { ActionContext } from "./bridge.js";

export interface FormState {
  draft: string;
  saved: string;
  saveClicks: number;
  otherClicks: number;
}

function next(value: number): number {
  if (!Number.isSafeInteger(value) || value >= Number.MAX_SAFE_INTEGER) {
    throw new Error("form counter exhausted");
  }
  return value + 1;
}

/** Application logic shared by the real DOM handlers and portable tests. */
export class FormModel {
  private state: FormState = { draft: "", saved: "", saveClicks: 0, otherClicks: 0 };
  private draftRevision = 0;

  snapshot(): FormState {
    return { ...this.state };
  }

  input(text: string): void {
    const revision = next(this.draftRevision);
    this.state.draft = text;
    this.draftRevision = revision;
  }

  other(): void {
    this.state.otherClicks = next(this.state.otherClicks);
  }

  /** Save crosses a real Promise boundary before the application commits. */
  async save(context: ActionContext, publish: () => void): Promise<void> {
    const draft = this.state.draft;
    const revision = this.draftRevision;
    await Promise.resolve();
    context.commit(() => {
      if (this.draftRevision !== revision) {
        throw new Error("input changed before save");
      }
      const clicks = next(this.state.saveClicks);
      this.state.saved = draft;
      this.state.saveClicks = clicks;
      publish();
    });
  }
}
