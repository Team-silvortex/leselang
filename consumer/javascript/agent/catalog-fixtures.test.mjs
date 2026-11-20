import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import test from "node:test";
import { createPublicationAgentCatalog } from "./catalog.mjs";
import { defaultWorker } from "./assistant.mjs";

test("the public registered task builder reproduces the shared publication catalog", async () => {
  const base = spawnSync(defaultWorker, ["--describe"], { encoding: "utf8", timeout: 5000 });
  assert.ifError(base.error);
  assert.equal(base.status, 0, base.stderr);
  const expected = await readFile(new URL("../../../fixtures/host-semantics/javascript-agent-semantics-v2.json", import.meta.url), "utf8");
  assert.equal(createPublicationAgentCatalog(base.stdout) + "\n", expected);
});
