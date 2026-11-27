// Hostile/slow child-process fixtures for the Node-owned carrier, never a VM.
const selected = process.argv[2];
const mode = selected.startsWith("failure_") ? selected.slice("failure_".length) : selected;
const command = process.argv.at(-1);
const report = selected.startsWith("failure_")
  ? '{ "schema_version":1,"profile":"javascript.registered","kind":"javascript_task_run","status":"failed","error":"fixture missing object","possible_partial_effects":true,"execution_failure":{"code":"execution_object_unavailable"} }'
  : '{"integer":18446744073709551615}';
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
  const profile = process.argv.at(-2) === "--profile-gui";
  const ready = line({ agent_worker_version: 3, kind: "profile_ready" });
  if (profile || mode === "ordinary_ready") {
    if (mode === "missing_ready") finish(line({ agent_worker_version: 3, kind: "report", report_json: report }));
    else {
      if (mode === "profile_handshake") {
        const premature = () => process.exit(17);
        process.stdin.once("readable", premature);
        await new Promise((resolve) => setTimeout(resolve, 30));
        process.stdin.off("readable", premature);
      }
      process.stdout.write(mode === "duplicate_ready" ? ready + ready : ready);
    }
  }
  const start = await readLine();
  version = mode === "downgrade" ? 1 : mode === "upgrade" || mode === "downgrade_two" ? 2
    : mode === "upgrade_three" ? 3 : start.agent_worker_version;
  if (![1, 2, 3].includes(start.agent_worker_version)) throw new Error("unsupported fixture start");
  const expectedFields = ["agent_worker_version", "kind", "scope", "source", "arguments"];
  if (start.agent_worker_version === 3) {
    expectedFields.push("input_mode");
    if (!["single_string", "named_strings"].includes(start.input_mode)) throw new Error("invalid fixture input mode");
  }
  if (Object.keys(start).length !== expectedFields.length || expectedFields.some((name) => !Object.hasOwn(start, name))) {
    throw new Error("invalid fixture start fields");
  }
  const request = { transport_version: 1, ...start.scope, request_id: 1, budget_ms: 1000,
    operation: "gui.list", arguments: {} };
  const challenge = "f".repeat(64);
  const requestFrame = envelope("request", { challenge, request });
  const terminal = envelope("report", { report_json: mode === "echo_arguments"
    ? JSON.stringify({ version, source: start.source, arguments: start.arguments,
      input_mode: start.input_mode, start_keys: Object.keys(start) }) : report });
  if ((profile && mode !== "missing_profile") || mode === "ordinary_profile") {
    terminal.worker_profile = { compile_ns: 101, vm_local_ns: 250, exchange_ns: mode === "invalid_duration" ? -1 : 400,
      exchange_count: mode === "wrong_exchange_count" || mode === "report_nonzero" ? 0 : 1 };
  }
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
