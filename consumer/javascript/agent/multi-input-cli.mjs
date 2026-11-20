#!/usr/bin/env node
// Declared-task CLI adapter for the independent Rust catalog agent.
import { open } from "node:fs/promises";
import { randomUUID } from "node:crypto";
import { AgentSession, createRegisteredAgentHost, prepareSemanticCatalog } from "@leselang/bridge";
import { createNodeAgentTransport } from "@leselang/bridge/node";
import { createArticleApplication } from "./multi-input-application.mjs";
import { createArticleCatalog } from "./multi-input-catalog.mjs";
import { defaultWorker } from "./assistant.mjs";

async function sourceFile(path) {
  const file = await open(path, "r");
  try {
    const buffer = new Uint8Array(16 * 1024 + 1);
    let length = 0;
    while (length < buffer.length) {
      const { bytesRead } = await file.read(buffer, length, buffer.length - length, null);
      if (bytesRead === 0) break;
      length += bytesRead;
    }
    if (length > 16 * 1024) throw new Error("source exceeds 16 KiB");
    return new TextDecoder("utf-8", { fatal: true }).decode(buffer.subarray(0, length));
  } finally { await file.close(); }
}

const transport = createNodeAgentTransport({ executable: defaultWorker, catalog: createArticleCatalog });
let host, session;
try {
  const args = process.argv.slice(2);
  if (args.length === 1 && args[0] === "--describe") {
    process.stdout.write(await transport.describe(new AbortController().signal) + "\n");
  } else {
    if (args.shift() !== "--run") throw new Error("expected --run FILE --entry main [--arg NAME VALUE] --report-json");
    const file = args.shift();
    if (!file) throw new Error("missing source file");
    let entry = false, report = false;
    const arguments_ = Object.create(null);
    while (args.length) {
      const flag = args.shift();
      if (flag === "--entry" && !entry && args.shift() === "main") entry = true;
      else if (flag === "--report-json" && !report) report = true;
      else if (flag === "--arg") {
        const name = args.shift(), value = args.shift();
        if (!name || value === undefined || Object.hasOwn(arguments_, name)) throw new Error("missing or repeated named argument");
        arguments_[name] = value;
        if (Object.keys(arguments_).length > 8) throw new Error("too many named arguments");
      } else throw new Error("invalid declared-task CLI option");
    }
    if (!report) throw new Error("JSON report is required");
    const source = await sourceFile(file);
    const application = createArticleApplication();
    host = createRegisteredAgentHost({ window: "article-cli", epoch: randomUUID(), application, transport,
      observations: { before: "objects_before", after: "objects_after" } });
    session = new AgentSession({ host, provider: { async plan({ catalog }) {
      const admitted = prepareSemanticCatalog(catalog), candidates = [];
      for (const task of admitted.tasks) {
        try {
          const candidate = admitted.prepare(task.id, arguments_).plan;
          if (candidate.source === source) candidates.push(candidate);
        } catch { /* A different declared signature is not a matching task. */ }
      }
      if (candidates.length !== 1) throw new Error("source and arguments must match one declared task");
      if (Object.keys(candidates[0].arguments).length && !entry) throw new Error("named inputs require explicit main entry");
      return candidates[0];
    } } });
    const outcome = await session.submit("Run the explicitly supplied declared task");
    if (outcome.status !== "verified" || typeof outcome.report !== "string") throw new Error(outcome.error ?? "task not verified");
    process.stdout.write(outcome.report + "\n");
  }
} catch (error) {
  process.stderr.write((error instanceof Error ? error.message : "declared task failed") + "\n");
  process.exitCode = 1;
} finally { session?.cancel(); host?.close(); }
