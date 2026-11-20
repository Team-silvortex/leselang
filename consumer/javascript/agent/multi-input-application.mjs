import { createRegisteredApplication } from "@leselang/bridge";

/** @param {string} value */
function bounded(value) {
  if (typeof value !== "string" || new TextEncoder().encode(value).length > 4096
    || value.includes("\0")) throw new Error("invalid article text");
  for (let index = 0; index < value.length; index++) {
    const unit = value.charCodeAt(index);
    if (unit >= 0xd800 && unit <= 0xdbff) {
      const next = value.charCodeAt(++index);
      if (!(next >= 0xdc00 && next <= 0xdfff)) throw new Error("invalid article text");
    } else if (unit >= 0xdc00 && unit <= 0xdfff) throw new Error("invalid article text");
  }
  return value;
}

/** A separate application model retained across independent task workers.
 * @param {{title?: string, content?: string, rejectContent?: boolean, faultAfterCommit?: boolean,
 * beforePublish?: (context: import("@leselang/bridge").RegisteredActionContext) => void | Promise<void>}} [options]
 */
export function createArticleApplication(options = {}) {
  const model = { title: bounded(options.title ?? ""), content: bounded(options.content ?? ""),
    savedTitle: "", savedContent: "", publishes: 0, otherClicks: 0 };
  /** @param {string} title @param {string} value */
  const field = (title, value) => ({ role: "text_field", title, value, enabled: true, editable: true, clicks: 0 });
  const application = createRegisteredApplication({
    title: { read: () => field("Article title", model.title), setText(value, context) {
      context.commit(() => { model.title = bounded(value); context.changed("publish"); });
    } },
    content: { read: () => field("Article content", model.content), setText(value, context) {
      if (options.rejectContent) throw new Error("content rejected by application");
      context.commit(() => { model.content = bounded(value); context.changed("publish"); });
    } },
    publish: { read: () => ({ role: "button", title: model.publishes ? model.savedTitle : "Publish article",
      value: model.savedContent, enabled: true, editable: false, clicks: model.publishes }),
    async click(context) {
      await options.beforePublish?.(context);
      context.commit(() => {
        if (model.publishes === 0xffff_ffff) throw new Error("publication counter exhausted");
        model.savedTitle = model.title;
        model.savedContent = model.content;
        model.publishes++;
      });
      if (options.faultAfterCommit) throw new Error("failure after article publication");
    } },
    other: { read: () => ({ role: "button", title: "Unrelated action", value: "",
      enabled: true, editable: false, clicks: model.otherClicks }), click(context) {
      context.commit(() => {
        if (model.otherClicks === 0xffff_ffff) throw new Error("unrelated counter exhausted");
        model.otherClicks++;
      });
    } },
  });
  return { ...application, snapshot: () => ({ ...model }) };
}
