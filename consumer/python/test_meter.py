"""Protocol boundaries that a conforming real worker does not cross."""

import asyncio
import json
from pathlib import Path
from types import SimpleNamespace
import unittest
from unittest.mock import AsyncMock, Mock, patch

import meter


fixture = json.loads((Path(__file__).resolve().parents[2] /
                      "fixtures/application-operations/two-effects.json").read_text(encoding="utf-8"))


def request(serial, operation, arguments):
    return dict(agent_worker_version=4, kind="request", challenge=f"{serial:064x}",
                request=dict(transport_version=1, window="meter", epoch="python", session="test",
                             request_id=serial, budget_ms=5000, operation=operation, arguments=arguments))


def worker_with(frames, code=1):
    stdout = asyncio.StreamReader()
    for frame in frames:
        stdout.feed_data(meter.encode(frame) + b"\n")
    stdout.feed_eof()
    return SimpleNamespace(stdout=stdout, stdin=SimpleNamespace(write=Mock(), drain=AsyncMock()),
                           wait=AsyncMock(return_value=code))


async def run_exchange(worker, app):
    with patch.object(meter.uuid, "uuid4", return_value=SimpleNamespace(hex="test")):
        return await meter.exchange(worker, fixture["operations"], fixture["source"], fixture["arguments"],
                                    fixture["grants"], app)


class TerminalChecks(unittest.IsolatedAsyncioTestCase):
    async def test_uncertain_write_disallows_a_followup_read_or_success_report(self):
        for followup in (request(2, "meter.read", {}), dict(agent_worker_version=4, kind="report")):
            with self.subTest(kind=followup["kind"]):
                app = meter.Meter("10", "string")
                worker = worker_with([request(1, "meter.add", {"amount": "1"}), followup])
                with self.assertRaisesRegex(ValueError, "expected error terminal"):
                    await run_exchange(worker, app)
                self.assertEqual(app.observations(),
                                 dict(add_calls=1, read_calls=0, committed_writes=1, balance="11"))
                self.assertEqual(worker.stdin.write.call_count, 2)  # start and uncertain reply
                reply = json.loads(worker.stdin.write.call_args.args[0])["reply"]
                self.assertEqual(reply["status"], "uncertain")

    async def test_signal_death_cannot_validate_an_error_terminal(self):
        worker = worker_with([dict(agent_worker_version=4, kind="error", error="denied")], code=-9)
        with self.assertRaisesRegex(ValueError, "invalid worker error"):
            await run_exchange(worker, meter.Meter("10", "balance"))

    async def test_oversized_inner_request_stops_before_dispatch(self):
        app = meter.Meter("10", "balance")
        worker = worker_with([request(1, "meter.add", {"amount": "1" * (32 * 1024)})])
        with self.assertRaisesRegex(ValueError, "oversized inner request"):
            await run_exchange(worker, app)
        self.assertEqual(app.add_calls, 0)


if __name__ == "__main__":
    unittest.main()
