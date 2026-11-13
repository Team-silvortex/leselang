// A separate JavaScript application. Its only LeseLang dependency is the SDK's
// public package export; it never loads Rust, Tauri or their source files.
import { Bridge } from "@leselang/bridge";
import { once } from "node:events";

const args = process.argv.slice(2);
if (args.length > 1 || (args.length === 1 && args[0] !== "--fault-after-commit")) {
  throw new Error("unknown adapter option");
}
const faultAfterCommit = args[0] === "--fault-after-commit";
const bridge = new Bridge(JSON.parse(process.env.LESELANG_BRIDGE_SCOPE ?? "null"));
const model = { draft: "", published: "", status: "idle", publishes: 0, resets: 0, unrelated: 0 };

function button(title, value, clicks) {
  return { role: "button", title, value, clicks, editable: false, enabled: true };
}
function evidence() {
  // Diagnostic evidence is distinct from the wire reply and never grants
  // authority. One bounded record proves the mutation survives a later error.
  const line = JSON.stringify({ kind: "javascript_consumer_state", published: model.published,
    publishes: model.publishes, resets: model.resets, unrelated: model.unrelated, status: model.status });
  if (Buffer.byteLength(line) > 32 * 1024) throw new Error("diagnostic record exceeds limit");
  process.stderr.write(`${line}\n`);
}

// Deliberately neither the reference form's names nor its three-object layout.
bridge.register("unrelated", {
  read: () => button("Unrelated", "", model.unrelated),
  click(context) { context.commit(() => { model.unrelated += 1; }); },
});
bridge.register("status", {
  read: () => ({ role: "text_field", title: "Publication status", value: model.status,
    clicks: 0, editable: false, enabled: true }),
});
bridge.register("draft", {
  read: () => ({ role: "text_field", title: "Draft", value: model.draft,
    clicks: 0, editable: true, enabled: true }),
  setText(text, context) {
    context.commit(() => {
      model.draft = text;
      bridge.changed("draft");
      // Publication depends on the draft even if it changes away and back.
      bridge.changed("publish");
    });
  },
});
bridge.register("reset", {
  read: () => button("Reset", "", model.resets),
  click(context) {
    context.commit(() => {
      model.draft = "";
      model.published = "";
      model.status = "idle";
      model.resets += 1;
      for (const name of ["draft", "publish", "status"]) bridge.changed(name);
    });
  },
});
bridge.register("publish", {
  read: () => button("Publish", model.published, model.publishes),
  async click(context) {
    await Promise.resolve();
    context.commit(() => {
      model.published = model.draft;
      model.publishes += 1;
      model.status = "published";
      bridge.changed("status");
    });
    evidence();
    if (faultAfterCommit) throw new Error("fixture failure after publication");
  },
});

const MAX_LINE_BYTES = 32 * 1024;
async function send(challenge, reply) {
  const bytes = Buffer.from(`${JSON.stringify({ stdio_version: 1, challenge, reply })}\n`);
  if (bytes.length > MAX_LINE_BYTES) throw new Error("wire reply exceeds limit");
  if (!process.stdout.write(bytes)) await once(process.stdout, "drain");
}

try {
  let pending = Buffer.alloc(0);
  // Exactly one request is handled at a time, including its Promise. There is
  // no VM in this process and no operation is retried by the transport.
  requests: for await (const chunk of process.stdin) {
    let start = 0;
    for (let end = 0; end < chunk.length; end += 1) {
      if (chunk[end] !== 10) continue;
      const part = chunk.subarray(start, end + 1);
      if (pending.length + part.length > MAX_LINE_BYTES) throw new Error("wire request exceeds limit");
      const line = Buffer.concat([pending, part]);
      pending = Buffer.alloc(0);
      const carrier = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(line));
      if (!carrier || Array.isArray(carrier) || typeof carrier !== "object" ||
          Object.keys(carrier).length !== 3 || carrier.stdio_version !== 1 ||
          !Object.hasOwn(carrier, "request") || typeof carrier.challenge !== "string" ||
          !/^[0-9a-f]{64}$/.test(carrier.challenge)) throw new Error("invalid stdio carrier envelope");
      const reply = await bridge.handle(carrier.request);
      await send(carrier.challenge, reply);
      if (reply.status !== "ok") {
        bridge.close();
        process.exitCode = 1;
        process.stdin.destroy();
        break requests;
      }
      start = end + 1;
    }
    const remainder = chunk.subarray(start);
    if (pending.length + remainder.length > MAX_LINE_BYTES) throw new Error("wire request exceeds limit");
    pending = Buffer.concat([pending, remainder]);
  }
  if (pending.length !== 0) throw new Error("truncated wire request");
} catch (error) {
  process.stderr.write(`JavaScript adapter: ${error instanceof Error ? error.message : "failed"}\n`);
  process.exitCode = 1;
} finally {
  bridge.close();
}
