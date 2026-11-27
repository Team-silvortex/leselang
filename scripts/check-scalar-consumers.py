"""Run the shared meter fixtures with three application-owned scalar hosts."""

import json
import os
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile
import uuid


ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT / "fixtures/application-operations"


def build_native(output):
    source = ROOT / "consumer/native"
    cc = shlex.split(os.environ.get("CC", "cc"))
    cxx = shlex.split(os.environ.get("CXX", "c++"))
    cflags = shlex.split(subprocess.check_output(["pkg-config", "--cflags", "libcjson"], text=True))
    libs = shlex.split(subprocess.check_output(["pkg-config", "--libs", "libcjson"], text=True))
    warnings = ["-Wall", "-Wextra", "-Werror", "-pedantic"]
    objects = []
    for module in ("meter_json", "meter_wire"):
        obj = output / (module + ".o")
        subprocess.run([*cc, "-std=c11", *warnings, *cflags, "-c", source / (module + ".c"), "-o", obj], check=True)
        objects.append(obj)
    for name, compiler, standard, app in [
        ("c", cc, "c11", "meter_c.c"), ("cpp", cxx, "c++17", "meter_cpp.cpp")
    ]:
        subprocess.run([*compiler, "-std=" + standard, *warnings, source / app, *objects,
                        *libs, "-o", output / name], check=True)


def reap(process):
    if process.poll() is None:
        process.kill()
    process.wait()


def run_native_case(host, worker, case_id, outcome):
    # The host and the evaluator own both ends of the protocol. The runner
    # observes process status and the host's separate diagnostic stream.
    with tempfile.TemporaryFile() as diagnostics:
        evaluator = subprocess.Popen([worker, "--serve"], stdin=subprocess.PIPE,
                                     stdout=subprocess.PIPE, stderr=diagnostics)
        try:
            try:
                peer = subprocess.Popen([host, FIXTURES, case_id, uuid.uuid4().hex], stdin=evaluator.stdout,
                                        stdout=evaluator.stdin, stderr=subprocess.PIPE)
            finally:
                evaluator.stdout.close()
                evaluator.stdin.close()
            try:
                _, evidence = peer.communicate(timeout=15)
                status = evaluator.wait(timeout=5)
            finally:
                reap(peer)
        finally:
            reap(evaluator)
        expected_exit = status == 0 if outcome == "reported" else status > 0
        if peer.returncode != 0 or not expected_exit:
            diagnostics.seek(0)
            raise RuntimeError(f"{host.name}/{case_id}: host exit {peer.returncode}, worker exit {status}\n"
                               + evidence.decode(errors="replace") + diagnostics.read().decode(errors="replace"))
    return evidence.decode("utf-8")


def expected_cases():
    success = json.loads((FIXTURES / "two-effects.json").read_text())["expected"]
    failures = json.loads((FIXTURES / "failure-vectors.json").read_text())["cases"]
    expected = {"two-effects": dict(success, outcome="reported", add_calls=1,
                                    read_calls=1, committed_writes=1, balance=success["result"]["value"])}
    for case in failures:
        if case["id"] in expected:
            raise ValueError(f"Repeated fixture ID {case['id']}")
        expected[case["id"]] = case["expected"]
    if set(expected) != {"two-effects", "missing-grant", "post-write-invalid-result"}:
        raise ValueError("Expected the shared two-Effect run and both failure vectors")
    return expected


def read_observations(text):
    rows = {}
    for line in text.splitlines():
        if line.startswith("LESE_METER "):
            row = json.loads(line[len("LESE_METER "):])
            if set(row) != {"id", "observed"} or row["id"] in rows:
                raise ValueError(f"Unexpected or duplicate observation: {line}")
            rows[row["id"]] = row["observed"]
    return rows


def main(worker):
    expected = expected_cases()
    evidence_root = ROOT / "target/verification"
    evidence_root.mkdir(exist_ok=True, parents=True)
    output = Path(tempfile.mkdtemp(prefix="scalar-consumers.", dir=evidence_root))
    build_native(output)
    for name in ("python", "c", "cpp"):
        if name == "python":
            result = subprocess.run([sys.executable, ROOT / "consumer/python/meter.py", worker, FIXTURES],
                                    capture_output=True, text=True, timeout=60)
            if result.returncode:
                raise RuntimeError(f"Python meter exited {result.returncode}\n{result.stderr}")
            rows = read_observations(result.stdout)
        else:
            rows = {}
            for case_id, case in expected.items():
                observed = read_observations(run_native_case(output / name, worker, case_id, case["outcome"]))
                if set(observed) != {case_id}:
                    raise ValueError(f"{name}/{case_id}: expected exactly one host observation, got {observed}")
                rows.update(observed)
        if json.dumps(rows, sort_keys=True) != json.dumps(expected, sort_keys=True):
            raise ValueError(f"{name}: observed application state differs from the shared fixtures: {rows}")
        (output / f"{name}.json").write_text(json.dumps(rows, indent=2) + "\n")
    for case_id, case in expected.items():
        print(f"{case_id}: {case['outcome']}, adds={case['add_calls']}, reads={case['read_calls']}, "
              f"commits={case['committed_writes']}, balance={case['balance']}")
    print(f"Python, C and C++ matched all three shared cases. Observations: {output}")


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit("Usage: python3 scripts/check-scalar-consumers.py WORKER")
    main(Path(sys.argv[1]).resolve(strict=True))
