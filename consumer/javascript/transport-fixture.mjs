// Deliberately invalid transport peers for the explicit Rust example tests.
// This fixture does not import the SDK and is never used by node_host runs.
import { createInterface } from "node:readline";

const mode = process.argv[2];
const scope = JSON.parse(process.env.LESELANG_BRIDGE_SCOPE);
const reply = (request_id) => ({ transport_version: 1, ...scope, request_id,
  status: "ok", value: [], error: null });
const envelope = (challenge, value) => JSON.stringify({ stdio_version: 1, challenge, reply: value }) + "\n";
if (mode === "no-read") {
  process.stdin.pause();
  setInterval(() => {}, 1000);
} else if (mode === "preplay") {
  process.stdout.write(envelope("0".repeat(64), reply(1)));
  setInterval(() => {}, 1000);
} else if (mode === "future") {
  const lines = createInterface({ input: process.stdin });
  lines.once("line", (line) => {
    const request = JSON.parse(line);
    // Both records are emitted together, before the parent has sent request 2.
    // A parser-thread scheduling race must never turn the second into a reply
    // to a future request: its challenge belongs to request 1.
    process.stdout.write(envelope(request.challenge, reply(1)) + envelope(request.challenge, reply(2)));
  });
} else {
  throw new Error("unknown transport fixture mode");
}
