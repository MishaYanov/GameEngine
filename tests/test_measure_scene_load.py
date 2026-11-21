"""Calibrate and compare bounded, request-keyed scene-load logs."""

from contextlib import redirect_stderr, redirect_stdout
from io import StringIO
import json
from pathlib import Path
import sys
import tempfile
import unittest
from uuid import UUID

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from measure_scene_load import MAX_LINE_BYTES, main


PROFILE = {
    "os": "Windows 11", "cpu": "Test CPU", "gpu": "Test GPU", "driver": "1.0",
    "build": "test-release", "width": 800, "height": 600,
}


def load(number, scope="play", cpu=None, host="player"):
    cpu = (number % 20) + 30 if cpu is None else cpu
    total = cpu + 15
    return {
        "schema_version": 1,
        "request_id": str(UUID(int=number)),
        "host": host,
        "scope": scope,
        "outcome": "presented",
        "elapsed_us": total,
        "cpu_us": cpu,
        "queue_us": 1,
        "gpu_us": 10,
        "presentation_us": 4,
        "first_present_us": total,
    }


class SceneLoadTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.profile = self.root / "profile.json"
        self.profile.write_text(json.dumps(PROFILE), encoding="utf-8")
        self.log = self.root / "engine.jsonl"
        self.budget = self.root / "budget.json"
        self.report = self.root / "report.json"

    def write(self, events, log=None):
        log = self.log if log is None else log
        with log.open("w", encoding="utf-8") as sink:
            sink.write('{"component":"player","message":"starting"}\n')
            for event in events:
                sink.write(json.dumps({"component": "scene_load", "message": json.dumps(event)}))
                sink.write("\n")

    def invoke(self, command, *, extra=(), fixture="d" * 64, logs=None):
        output = self.budget if command == "calibrate" else self.report
        command_line = [
            command, "--fixture-sha256", fixture, "--profile", str(self.profile),
            "--output", str(output),
        ]
        for log in (self.log,) if logs is None else logs:
            command_line.extend(("--log", str(log)))
        if command == "calibrate":
            command_line.extend(("--host", "player", "--headroom-percent", "5"))
        else:
            command_line.extend(("--budget", str(self.budget)))
        errors = StringIO()
        with redirect_stdout(StringIO()), redirect_stderr(errors):
            code = main(command_line + list(extra))
        return code, errors.getvalue()

    def test_calibration_holds_cold_warm_profiles_apart(self):
        self.write([load(i) for i in range(1, 21)] + [load(i, "startup", cpu=90) for i in range(21, 41)]
                   + [load(41, "warmup", cpu=1000), load(42, "play", cpu=1000, host="editor")])
        self.assertEqual(self.invoke("calibrate", extra=("--scope", "both", "--jitter-us", "2")), (0, ""))
        budget = json.loads(self.budget.read_bytes())
        self.assertEqual(budget["baseline"]["warm"]["first_present_us"], {"median": 54.5, "p95": 63})
        self.assertEqual(budget["baseline"]["cold"]["first_present_us"], {"median": 105, "p95": 105})
        self.assertEqual(budget["ceilings_us"]["warm"]["queue_us"], 3)
        self.assertEqual(self.invoke("check"), (0, ""))
        self.assertTrue(json.loads(self.report.read_bytes())["passed"])

    def test_regression_has_a_report_and_exit_status_one(self):
        self.write(load(i, cpu=10) for i in range(1, 21))
        self.assertEqual(self.invoke("calibrate", extra=("--scope", "warm")), (0, ""))
        self.write(load(i, cpu=99) for i in range(1, 21))
        self.assertEqual(self.invoke("check"), (1, ""))
        report = json.loads(self.report.read_bytes())
        self.assertFalse(report["passed"])
        self.assertIn({"scope": "warm", "metric": "first_present_us", "p95_us": 114, "ceiling_us": 27},
                      report["regressions"])

    def test_duplicate_requests_across_files_fail(self):
        second = self.root / "more.jsonl"
        self.write(load(i) for i in range(1, 21))
        self.write([load(18)], log=second)
        code, error = self.invoke("calibrate", extra=("--scope", "warm"), logs=(self.log, second))
        self.assertEqual(code, 2)
        self.assertIn("Duplicate", error)
        self.assertFalse(self.budget.exists())

    def test_short_and_malformed_inputs_are_rejected(self):
        self.write([])
        self.assertIn("found 0", self.invoke("calibrate", extra=("--scope", "warm"))[1])
        for field, value in [
            ("first_present_us", None), ("cpu_us", -1), ("gpu_us", True),
            ("queue_us", float("nan")), ("presentation_us", 999), ("request_id", "not-a-uuid"),
        ]:
            with self.subTest(field=field):
                self.write([dict(load(1), **{field: value})])
                self.assertEqual(self.invoke("calibrate", extra=("--scope", "warm"))[0], 2)
        self.log.write_bytes(b" " * (MAX_LINE_BYTES + 1))
        self.assertIn("size limit", self.invoke("calibrate", extra=("--scope", "warm"))[1])
        self.assertFalse(self.budget.exists())

    def test_unexpected_cancellation_fails_selected_scope(self):
        cancelled = dict(load(21), outcome="cancelled", presentation_us=None, first_present_us=None)
        self.write([load(i) for i in range(1, 21)] + [cancelled])
        code, error = self.invoke("calibrate", extra=("--scope", "warm"))
        self.assertEqual(code, 2)
        self.assertIn("Unexpected cancelled", error)
        self.assertFalse(self.budget.exists())

    def test_profile_fixture_and_budget_fields_are_checked(self):
        self.write(load(i) for i in range(1, 21))
        self.assertEqual(self.invoke("calibrate", extra=("--scope", "warm")), (0, ""))
        self.assertIn("does not match", self.invoke("check", fixture="2" * 64)[1])
        self.profile.write_text(json.dumps(dict(PROFILE, driver="2.0")), encoding="utf-8")
        self.assertIn("does not match", self.invoke("check")[1])
        self.profile.write_text(json.dumps(PROFILE), encoding="utf-8")
        budget = json.loads(self.budget.read_bytes())
        budget["ceilings_us"]["warm"]["cpu_us"] = 0
        self.budget.write_text(json.dumps(budget), encoding="utf-8")
        self.assertIn("do not match", self.invoke("check")[1])
        self.assertFalse(self.report.exists())
