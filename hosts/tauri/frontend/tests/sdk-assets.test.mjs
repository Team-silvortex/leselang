import assert from "node:assert/strict";
import { readFile, readdir } from "node:fs/promises";
import test from "node:test";

const installed = new URL(".", import.meta.resolve("@leselang/bridge"));
const bundled = new URL("../dist/sdk/", import.meta.url);

async function modules(directory, excludeNode = false, prefix = "") {
  const result = [];
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    if (entry.isDirectory()) {
      if (excludeNode && !prefix && entry.name === "node") continue;
      result.push(...await modules(new URL(`${entry.name}/`, directory), excludeNode, `${prefix}${entry.name}/`));
    } else if (entry.name.endsWith(".js") || entry.name.endsWith(".d.ts")) result.push(`${prefix}${entry.name}`);
  }
  return result.sort();
}

test("WebView assets preserve the installed browser SDK without the Node carrier", async () => {
  const nodeEntry = new URL(import.meta.resolve("@leselang/bridge/node"));
  await readFile(nodeEntry, "utf8");
  assert.ok((await readdir(installed)).includes("node"), "the installed SDK contains the opt-in carrier");
  assert.ok(!(await readdir(bundled)).includes("node"), "the WebView excludes the Node-only subtree");
  const expected = await modules(installed, true);
  assert.ok(expected.includes("index.js"));
  assert.ok(expected.includes("agent/registered-host.js"));
  assert.deepEqual(await modules(bundled), expected);
  for (const file of expected) {
    assert.equal(await readFile(new URL(file, bundled), "utf8"), await readFile(new URL(file, installed), "utf8"), file);
  }
  assert.equal(await readFile(new URL("../dist/bridge.js", import.meta.url), "utf8"), 'export * from "./sdk/index.js";\n');
});
