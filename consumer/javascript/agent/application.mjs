// The Node application owns this model across worker processes and tasks.
// Adapters expose only bounded observations and gate every actual mutation.
import { createRegisteredApplication } from "@leselang/bridge";

const MAX_COUNT = 0xffff_ffff;
const encoder = new TextEncoder();

/** @param {unknown} value */
function text(value) {
  if (typeof value !== "string" || encoder.encode(value).length > 4096 || value.includes("\0")) {
    throw new Error("publication text must be a bounded string");
  }
  for (let index = 0; index < value.length; index++) {
    const unit = value.charCodeAt(index);
    if (unit >= 0xd800 && unit <= 0xdbff) {
      const next = value.charCodeAt(++index);
      if (!(next >= 0xdc00 && next <= 0xdfff)) throw new Error("publication text must be valid Unicode");
    } else if (unit >= 0xdc00 && unit <= 0xdfff) throw new Error("publication text must be valid Unicode");
  }
  return value;
}

/** @param {number} value */
function increment(value) {
  if (value === MAX_COUNT) throw new Error("publication counter exhausted");
  return value + 1;
}

/** @param {{ initialDraft?: string, beforePublish?: (context: import("@leselang/bridge").RegisteredActionContext) => void | Promise<void>, faultAfterCommit?: boolean }} [options] */
export function createPublicationApplication(options = {}) {
  const { initialDraft = "", beforePublish, faultAfterCommit = false } = options;
  if ((beforePublish !== undefined && typeof beforePublish !== "function") || typeof faultAfterCommit !== "boolean") {
    throw new Error("invalid publication application options");
  }
  const model = { draft: text(initialDraft), published: "", status: "idle", publishes: 0, resets: 0, unrelated: 0 };

  /** @type {(title: string, value: string, clicks: number) => import("@leselang/bridge").ControlState} */
  const button = (title, value, clicks) => ({ role: "button", title, value, clicks, editable: false, enabled: true });
  const states = {
    unrelated: () => button("Unrelated", "", model.unrelated),
    status: () => ({ role: "text_field", title: "Publication status", value: model.status,
      clicks: 0, editable: false, enabled: true }),
    draft: () => ({ role: "text_field", title: "Draft", value: model.draft,
      clicks: 0, editable: true, enabled: true }),
    reset: () => button("Reset", "", model.resets),
    publish: () => button("Publish", model.published, model.publishes),
  };

  const application = createRegisteredApplication({
    unrelated: {
      read: states.unrelated,
      click(context) { context.commit(() => { model.unrelated = increment(model.unrelated); }); },
    },
    status: { read: states.status },
    draft: {
      read: states.draft,
      setText(value, context) {
        const value_ = text(value);
        context.commit(() => {
          model.draft = value_;
          // Publication depends on the draft, including unseen ABA edits.
          context.changed("draft", "publish");
        });
      },
    },
    reset: {
      read: states.reset,
      click(context) {
        context.commit(() => {
          const resets = increment(model.resets);
          model.draft = "";
          model.published = "";
          model.status = "idle";
          model.resets = resets;
          context.changed("draft", "publish", "status");
        });
      },
    },
    publish: {
      read: states.publish,
      async click(context) {
        // The test hook can hold completion at the real asynchronous boundary.
        // It receives no model reference and cannot bypass the commit gate.
        await (beforePublish?.(context) ?? Promise.resolve());
        context.commit(() => {
          const publishes = increment(model.publishes);
          model.published = model.draft;
          model.publishes = publishes;
          model.status = "published";
          context.changed("status");
        });
        if (faultAfterCommit) throw new Error("fixture failure after publication");
      },
    },
  });
  return {
    ...application,
    snapshot() { return { ...model }; },
  };
}
