import { Bridge } from "./bridge.js";
import { createFormView } from "./form-view.js";
import type {} from "./tauri-api.js";

function element<T extends HTMLElement>(id: string): T {
  const value = document.getElementById(id);
  if (!value) throw new Error("form element missing");
  return value as T;
}

const input = element<HTMLTextAreaElement>("message");
const save = element<HTMLButtonElement>("save");
const other = element<HTMLButtonElement>("other");
const saved = element<HTMLPreElement>("saved-value");
const saveCount = element<HTMLSpanElement>("save-count");
const otherCount = element<HTMLSpanElement>("other-count");
const status = element<HTMLParagraphElement>("status");
const form = createFormView({ input, save, other, saved, saveCount, otherCount, status });
const scope = { window: "main", epoch: crypto.randomUUID(), session: crypto.randomUUID() };
const bridge = new Bridge(scope);
form.attach(bridge);
let bridgeLive = true;
const setStatus = form.setStatus;
function closeBridge(): void {
  bridgeLive = false;
  form.detach(bridge);
}

async function connect(): Promise<void> {
  const api = window.__TAURI__;
  if (!api) throw new Error("this page requires the Tauri reference host");
  const channel = new api.core.Channel<unknown>();
  channel.onmessage = (message) => {
    if (message && typeof message === "object" && "kind" in message && message.kind === "finished") {
      closeBridge();
      const report = "report" in message ? message.report : null;
      const failed = report && typeof report === "object" && (
        ("status" in report && report.status === "failed") ||
        ("verified" in report && report.verified === false)
      );
      const verified = report && typeof report === "object" && "verified" in report && report.verified === true;
      setStatus(failed
        ? "The task failed. Some actions may already have happened; inspect the report."
        : verified
          ? "Verified. The saved message matches the task result."
          : "LeseLang completed. Inspect the saved value and the host report.", Boolean(failed));
      return;
    }
    void bridge.handle(message)
      .then((reply) => api.core.invoke("bridge_reply", { reply }))
      .catch(() => {
        closeBridge();
        setStatus("The host connection failed. This task will not be retried.", true);
      });
  };
  window.addEventListener("pagehide", () => {
    closeBridge();
    void api.core.invoke("bridge_closed").catch(() => {});
  }, { once: true });
  await api.core.invoke("bridge_ready", {
    onRequest: channel, epoch: scope.epoch, session: scope.session,
  });
  if (bridgeLive) setStatus("Connected. Ready for LeseLang operations.");
}

void connect().catch(() => {
  closeBridge();
  setStatus("Unable to connect to the Tauri host.", true);
});
