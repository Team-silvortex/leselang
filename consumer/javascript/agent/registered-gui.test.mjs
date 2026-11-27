import test from "node:test";
import { fileURLToPath } from "node:url";
import { checkRegisteredGui } from "./registered-gui.mjs";

test("the Node GUI Consumer verifies the shared publication cases and reads both controls in fresh workers", async (t) => {
  const suffix = process.platform === "win32" ? ".exe" : "";
  const worker = fileURLToPath(new URL(`../../../target/lean/leselang-agent-worker${suffix}`, import.meta.url));
  const fixture = fileURLToPath(new URL("../../../fixtures/registered-gui/publication.json", import.meta.url));
  await checkRegisteredGui(worker, fixture, ({ id }) => t.diagnostic(id));
});
