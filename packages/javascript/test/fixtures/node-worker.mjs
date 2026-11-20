// Hostile/slow child-process fixtures for the Node-owned carrier, never a VM.
const mode = process.argv[2];
const command = process.argv.at(-1);
const report = '{"integer":18446744073709551615}';
let version = 1;
const envelope = (kind, fields) => ({ agent_worker_version: version, kind, ...fields });
const line = (value) => `${JSON.stringify(value)}\n`;
function finish(bytes, code = 0) {
  process.stdout.write(bytes, () => process.exit(code));
}
function readLine() {
  return new Promise((resolve, reject) => {
    let pending = "";
    const data = (chunk) => {
      pending += chunk;
      if (pending.includes("\n")) {
        process.stdin.off("data", data);
        process.stdin.pause();
        try { resolve(JSON.parse(pending.trim())); } catch (error) { reject(error); }
      }
    };
    process.stdin.on("data", data);
    process.stdin.resume();
  });
}
if (command === "--describe") {
  if (mode === "description_duplicate") finish('{"profile":"a","profile":"b"}');
  else if (mode === "description_oversized") finish(" ".repeat(256 * 1024 + 1));
  else if (mode === "description_invalid_utf8") finish(Buffer.from([0xff]));
  else if (mode === "description_hang") setInterval(() => {}, 1000);
  else finish('{"schema_version":1,"profile":"javascript.registered"}');
} else {
  const start = await readLine();
  version = mode === "downgrade" ? 1 : mode === "upgrade" ? 2 : start.agent_worker_version;
  if (![1, 2].includes(start.agent_worker_version)) throw new Error("unsupported fixture start");
  const request = { transport_version: 1, ...start.scope, request_id: 1, budget_ms: 1000,
    operation: "gui.list", arguments: {} };
  const challenge = "f".repeat(64);
  const requestFrame = envelope("request", { challenge, request });
  const terminal = envelope("report", { report_json: mode === "echo_arguments"
    ? JSON.stringify({ version, source: start.source, arguments: start.arguments }) : report });
  if (mode === "ignore_term") {
    process.on("SIGTERM", () => {});
    process.stderr.write(`ready:${process.pid}\n`);
    setInterval(() => {}, 1000);
  } else if (mode === "report_held") {
    process.stdout.write(line(terminal), () => {
      process.stderr.write("report_sent\n");
      setTimeout(() => process.exit(0), 200);
    });
  } else if (mode === "report_nonzero") finish(line(terminal), 2);
  else if (mode === "report_hang") {
    process.stdout.write(line(terminal));
    setInterval(() => {}, 1000);
  } else if (mode === "duplicate_terminal") finish(line(terminal) + line(terminal));
  else if (mode === "truncated") finish(JSON.stringify(terminal));
  else if (mode === "invalid_utf8") finish(Buffer.from([0xff, 0x0a]));
  else if (mode === "duplicate_key") finish(line(terminal).replace('"kind":"report"', '"kind":"error","kind":"report"'));
  else if (mode === "nested_duplicate") finish(line(requestFrame).replace('"arguments":{}', '"arguments":{"x":1,"x":2}'));
  else if (mode === "extra_key") finish(line({ ...terminal, extra: true }));
  else if (mode === "unsafe_integer") finish(line(requestFrame).replace('"request_id":1', '"request_id":9007199254740993'));
  else if (mode === "invalid_surrogate") finish('{"agent_worker_version":1,"kind":"report","report_json":"\\ud800"}\n');
  else if (mode === "oversized") finish("x".repeat(512 * 1024 + 1));
  else if (mode === "report_oversized") finish(line(envelope("report", { report_json: "x".repeat(128 * 1024 + 1) })));
  else if (mode === "stderr_oversized") process.stderr.write("x".repeat(128 * 1024 + 1), () => setInterval(() => {}, 1000));
  else if (mode === "no_report") process.exit(0);
  else if (mode === "error") finish(line(envelope("error", { error: "fixture_failure" })), 1);
  else if (mode === "foreign_scope") finish(line({ ...requestFrame, request: { ...request, epoch: "foreign" } }));
  else if (mode === "serial_gap") finish(line({ ...requestFrame, request: { ...request, request_id: 2 } }));
  else if (mode === "request_double") finish(line(requestFrame) + line(requestFrame));
  else if (mode === "pending_report") finish(line(requestFrame) + line(terminal));
  else if (mode === "after_report") finish(line(terminal) + line(requestFrame));
  else if (mode === "bad_challenge") finish(line({ ...requestFrame, challenge: "not-a-challenge" }));
  else if (mode === "array_challenge") finish(line({ ...requestFrame, challenge: [challenge] }));
  else {
    const replyPromise = readLine();
    process.stdout.write(line(requestFrame));
    const reply = await replyPromise;
    if (reply.agent_worker_version !== version || reply.kind !== "reply" || reply.challenge !== challenge
      || reply.reply.window !== start.scope.window || reply.reply.epoch !== start.scope.epoch
      || reply.reply.session !== start.scope.session || reply.reply.request_id !== 1) {
      finish(line(envelope("error", { error: "fixture_bad_reply" })), 1);
    } else finish(line(terminal));
  }
}
