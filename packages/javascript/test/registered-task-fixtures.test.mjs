import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { prepareSemanticCatalog } from "../dist/index.js";

const fixture = (name) => readFile(new URL(`../../../fixtures/host-semantics/${name}`, import.meta.url), "utf8");

test("expanded registered tasks use the same programs as the independent Rust catalog agent", async () => {
  const catalog = prepareSemanticCatalog(await fixture("javascript-agent-semantics-v2.json"));
  assert.equal(catalog.profile, "javascript.registered");
  for (const [task, file, effects] of [
    ["read_draft", "read-draft", 2], ["publish_article", "publish-article", 9],
    ["read_publication", "read-publication", 2], ["check_publication", "check-publication", 1],
  ]) {
    const prepared = catalog.prepare(task, task === "publish_article" ? { text: "literal data, never source ✓" } : {});
    assert.equal(prepared.plan.source, await fixture(`javascript-${file}.lese`));
    assert.equal(prepared.expectedEffects, effects);
    assert.equal(prepared.plan.source.includes("literal data, never source ✓"), false);
  }
});
