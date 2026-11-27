"""Independent Python meter application using the scalar worker v4 protocol."""

import argparse
import asyncio
import json
from pathlib import Path
import re
import uuid


PROFILE = "lese.operations"
MAX_U64 = 2**64 - 1
KIB = 1024


def require(condition, message):
    if not condition:
        raise ValueError(message)


def object_fields(pairs):
    obj = dict(pairs)
    require(len(obj) == len(pairs), "duplicate JSON field")
    return obj


def metadata_number(token):
    require(re.fullmatch(r"0|[1-9][0-9]*", token), "invalid metadata number")
    return int(token)


def decode(text):
    return json.loads(text, object_pairs_hook=object_fields, parse_int=metadata_number,
                      parse_float=metadata_number, parse_constant=metadata_number)


def fields(obj, *names):
    require(isinstance(obj, dict) and obj.keys() == set(names), "unexpected object fields")


def wire_integer(text):
    require(isinstance(text, str) and re.fullmatch(r"0|[1-9][0-9]{0,19}", text),
            "expected a canonical decimal integer")
    number = int(text)
    require(number <= MAX_U64, "integer exceeds u64")
    return number


class Meter:
    def __init__(self, initial_balance, write_reply):
        require(write_reply in ("balance", "string"), "unknown write reply mode")
        self.balance = wire_integer(initial_balance)
        self.write_reply = write_reply
        self.add_calls = self.read_calls = self.committed_writes = 0

    def dispatch(self, operation, arguments):
        if operation == "meter.read":
            fields(arguments)
            self.read_calls += 1
            return self.balance
        require(operation == "meter.add", "unknown operation")
        fields(arguments, "amount")
        amount = wire_integer(arguments["amount"])
        require(self.balance + amount <= MAX_U64, "balance exceeds u64")
        self.add_calls += 1
        self.balance += amount
        self.committed_writes += 1
        return "wrong type" if self.write_reply == "string" else self.balance

    def observations(self):
        return dict(add_calls=self.add_calls, read_calls=self.read_calls,
                    committed_writes=self.committed_writes, balance=str(self.balance))


def encode(value):
    return json.dumps(value, ensure_ascii=False, separators=(",", ":")).encode("utf-8")


async def send(stream, frame, limit):
    line = encode(frame) + b"\n"
    require(len(line) <= limit, "outbound frame too large")
    stream.write(line)
    await stream.drain()


async def exchange(worker, operations, source, arguments, grants, meter):
    scope = dict(window="meter", epoch="python", session=uuid.uuid4().hex)
    declarations = {op["name"]: op for op in operations}
    await send(worker.stdin, dict(agent_worker_version=4, kind="start", profile=PROFILE,
                                  scope=scope, source=source, arguments=arguments,
                                  operations=operations, grants=grants), 128 * KIB)
    challenges = set()
    expect_error = False
    while True:
        line = await worker.stdout.readuntil(b"\n")
        require(len(line) <= 512 * KIB, "worker frame too large")
        frame = decode(line.decode("utf-8"))
        require(frame["agent_worker_version"] == 4, "unexpected worker version")
        kind = frame["kind"]
        require(not expect_error or kind == "error", "expected error terminal after failed reply")
        if kind != "request":
            break
        fields(frame, "agent_worker_version", "kind", "challenge", "request")
        require(len(line) <= 64 * KIB, "oversized request")
        challenge, req = frame["challenge"], frame["request"]
        require(len(encode(req)) <= 32 * KIB, "oversized inner request")
        require(isinstance(challenge, str) and re.fullmatch(r"[a-f0-9]{64}", challenge)
                and challenge not in challenges, "invalid or repeated challenge")
        fields(req, "transport_version", *scope, "request_id", "budget_ms", "operation", "arguments")
        serial = len(challenges) + 1
        identity = dict(transport_version=1, **scope, request_id=serial)
        require(type(req["transport_version"]) is int and type(req["request_id"]) is int
                and all(req[k] == v for k, v in identity.items()), "foreign or out-of-order request")
        require(type(req["budget_ms"]) is int and 0 < req["budget_ms"] <= 5000, "invalid budget")
        name = req["operation"]
        require(serial <= 2 and name == ("meter.add", "meter.read")[serial - 1], "unexpected Effect")
        op = declarations[name]
        # Dispatch, commit and reply construction run together, before the next await.
        require(op["capability"] in grants, "ungranted request reached the application")
        require(op["result"] == "integer", "unsupported meter result declaration")
        challenges.add(challenge)
        value = meter.dispatch(name, req["arguments"])
        if type(value) is int:
            reply = dict(**identity, status="ok", value=str(value), error=None)
        else:
            reply = dict(**identity, status="uncertain", value=None, error="invalid_result")
        expect_error = reply["status"] != "ok"
        await send(worker.stdin, dict(agent_worker_version=4, kind="reply", challenge=challenge,
                                      reply=reply), 64 * KIB)

    require(await worker.stdout.read(1) == b"", "trailing data after terminal frame")
    code = await worker.wait()
    if kind == "error":
        fields(frame, "agent_worker_version", "kind", "error")
        require(code > 0 and isinstance(frame["error"], str)
                and 0 < len(frame["error"].encode("utf-8")) <= KIB, "invalid worker error")
        return dict(outcome="failed", **meter.observations())
    require(kind == "report" and code == 0, "expected successful worker report")
    fields(frame, "agent_worker_version", "kind", "report_json")
    raw = frame["report_json"]
    require(isinstance(raw, str) and len(raw.encode("utf-8")) <= 128 * KIB, "invalid report")
    report = decode(raw)
    fields(report, "schema_version", "kind", "profile", "result", "effects", "fuel_remaining")
    require(type(report["schema_version"]) is int and report["schema_version"] == 1
            and report["kind"] == "operation_task_run" and report["profile"] == PROFILE,
            "unrecognized report")
    require(type(report["effects"]) is int and report["effects"] == len(challenges)
            and type(report["fuel_remaining"]) is int and 0 <= report["fuel_remaining"] <= 10000,
            "invalid Effect or Fuel count")
    fields(report["result"], "type", "value")
    require(report["result"]["type"] == "integer" and wire_integer(report["result"]["value"]) == meter.balance,
            "reported result differs from the application's balance")
    return dict(outcome="reported", result=report["result"], effects=report["effects"],
                fuel_remaining=report["fuel_remaining"], **meter.observations())


async def run(worker_path, fixture_dir):
    success = decode((fixture_dir / "two-effects.json").read_text(encoding="utf-8"))
    failures = decode((fixture_dir / "failure-vectors.json").read_text(encoding="utf-8"))
    require(failures["entry"] == "main", "unexpected fixture entry")
    require(sorted(case["id"] for case in failures["cases"]) == ["missing-grant", "post-write-invalid-result"],
            "missing or duplicate failure scenario")
    expected = success["expected"]
    success_case = dict(id="two-effects", source=success["source"], arguments=success["arguments"],
                        grants=success["grants"], write_reply="balance",
                        expected=dict(outcome="reported", **expected, balance=expected["result"]["value"],
                                      add_calls=1, read_calls=1, committed_writes=1))
    for case in [success_case, *failures["cases"]]:
        source = case.get("source", failures["source"])
        meter = Meter(failures["initial_balance"], case["write_reply"])
        worker = await asyncio.create_subprocess_exec(
            str(worker_path), "--serve", stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE, limit=512 * KIB)
        try:
            session = exchange(worker, success["operations"], source, case.get("arguments", {}), case["grants"], meter)
            observed = await asyncio.wait_for(session, 10)
        finally:
            if worker.returncode is None:
                try:
                    worker.kill()
                except ProcessLookupError:
                    pass
            # Consume buffered stdout too: asyncio can pause a full pipe even
            # after the child dies, which can prevent wait() from completing.
            await asyncio.wait_for(worker.communicate(), 2)
        require(observed == case["expected"], f"{case['id']} observation differs: {observed!r}")
        print("LESE_METER " + json.dumps(dict(id=case["id"], observed=observed)))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("worker", type=Path)
    parser.add_argument("fixture_dir", type=Path)
    args = parser.parse_args()
    asyncio.run(run(args.worker.resolve(), args.fixture_dir))
