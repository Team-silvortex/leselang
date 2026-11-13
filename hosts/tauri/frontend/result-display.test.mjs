import assert from "node:assert/strict";
import test from "node:test";
import { describeVerifiedResult } from "./dist/result-display.js";

test("verified result display preserves scalar meaning without rounding exact integer evidence", () => {
  for (const [type, value, expected] of [
    ["boolean", true, "boolean：true"],
    ["boolean", false, "boolean：false"],
    ["string", 'saved "✓"\nnext', 'string："saved \\"✓\\"\\nnext"'],
    ["integer", 3, "integer：3"],
    ["none", null, "none"],
    ["optional_string", null, "optional_string：null"],
    ["optional_string", "saved", 'optional_string："saved"'],
    ["string_list", ["input", "save"], 'string_list：["input","save"]'],
  ]) {
    assert.equal(describeVerifiedResult(JSON.stringify({ result: { type, value } })), expected);
  }
  const raw = '{"result":{"type":"integer","value":18446744073709551615}}';
  assert.equal(describeVerifiedResult(raw), "integer：精确整数，见原始报告");
  assert.match(raw, /18446744073709551615/u, "the separately displayed raw report stays unchanged");
  for (const raw of [null, {}, "invalid", "null", '{}', '{"result":{"type":"boolean","value":1}}']) {
    assert.equal(describeVerifiedResult(raw), "结果见原始报告");
  }
});
