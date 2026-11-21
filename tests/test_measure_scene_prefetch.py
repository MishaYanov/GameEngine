"""Join native prefetch and presentation evidence without mixing experiment modes."""

from contextlib import redirect_stderr, redirect_stdout
from io import StringIO
import json
from pathlib import Path
import tempfile
import unittest

from test_measure_scene_load import PROFILE, load
from measure_scene_prefetch import main


def event(component, message, session):
    return json.dumps({
        "component": component, "message": json.dumps(message), "session_id": session,
    })


class PrefetchBenchmarkTests(unittest.TestCase):
    def setUp(self):
        scratch = tempfile.TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        self.root = Path(scratch.name)
        self.profile = self.root / "profile.json"
        self.profile.write_text(json.dumps(PROFILE), encoding="utf-8")
        self.off = self.root / "off.jsonl"
        self.on = self.root / "on.jsonl"
        self.output = self.root / "report.json"

    def evidence(self, path, enabled, elapsed=100, count=30):
        session = "enabled-run" if enabled else "disabled-run"
        ids = 1000 if enabled else 0
        with path.open("w", encoding="utf-8") as destination:
            for sample in range(count):
                terminal = load(ids + sample + 1, cpu=15 if enabled else 30)
                disposition = event("scene_prefetch", {
                    "schema_version": 1,
                    "kind": "load",
                    "request_id": terminal["request_id"],
                    "scene_id": "garden",
                    "outcome": "stale" if enabled and sample == 0 else "hit" if enabled else "disabled",
                    "retained_estimate_bytes": 256 if enabled else 0,
                }, session)
                destination.write(disposition + "\n")
                destination.write(event("scene_load", terminal, session) + "\n")
                for frame in range(3):
                    destination.write(event("scene_prefetch", {
                        "schema_version": 1, "kind": "active_frame", "sample": sample,
                        "frame": frame, "enabled": enabled, "elapsed_us": elapsed,
                    }, session) + "\n")

    def run_comparison(self, margin=5):
        errors = StringIO()
        with redirect_stdout(StringIO()), redirect_stderr(errors):
            code = main([
                "--disabled", str(self.off), "--enabled", str(self.on),
                "--fixture-sha256", "b" * 64, "--profile", str(self.profile),
                "--active-headroom-us", str(margin), "--output", str(self.output),
            ])
        return code, errors.getvalue()

    def test_matching_native_requests_and_active_frames(self):
        self.evidence(self.off, False)
        self.evidence(self.on, True, elapsed=105)
        self.assertEqual(self.run_comparison(), (0, ""))
        report = json.loads(self.output.read_bytes())
        self.assertTrue(report["active_frame_passed"])
        self.assertEqual(report["enabled"]["prefetch_outcomes"], {"hit": 29, "stale": 1})
        self.assertEqual(report["enabled"]["max_retained_estimate_bytes"], 256)
        self.assertEqual(report["enabled"]["transition_first_present_us"]["p95"], 30)
        self.assertEqual(report["enabled"]["active_frame_us"]["samples"], 90)
        self.assertEqual(report["active_frame_p95_ceiling_us"], 105)

    def test_regression_writes_comparison(self):
        self.evidence(self.off, False)
        self.evidence(self.on, True, elapsed=111)
        self.assertEqual(self.run_comparison(), (1, ""))
        self.assertFalse(json.loads(self.output.read_bytes())["active_frame_passed"])

    def test_duplicate_or_unmatched_input_fails(self):
        self.evidence(self.off, False)
        self.evidence(self.on, True, count=29)
        self.assertIn("Expected 30", self.run_comparison()[1])
        self.assertFalse(self.output.exists())
        self.evidence(self.on, True)
        lines = self.on.read_text(encoding="utf-8").splitlines()
        self.on.write_text("\n".join(lines + [lines[2]]) + "\n", encoding="utf-8")
        self.assertIn("Duplicate active frame", self.run_comparison()[1])
        self.on.write_text("\n".join(lines[0:1] + lines[2:]) + "\n", encoding="utf-8")
        self.assertIn("has no terminal timing", self.run_comparison()[1])
        self.assertFalse(self.output.exists())


if __name__ == "__main__":
    unittest.main()
