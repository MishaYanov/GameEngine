"""Check direction grouping, completed and failed samples, and request ownership."""

from contextlib import redirect_stderr, redirect_stdout
from io import StringIO
import json
from pathlib import Path
import sys
import tempfile
import unittest
from uuid import UUID

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from measure_scene_switch import main


PROFILE = {
    "os": "Windows 11", "cpu": "Test CPU", "gpu": "Test GPU", "driver": "1.0",
    "build": "test-release", "width": 800, "height": 600,
}


def sample(index, session="run", *, failed=False):
    request = str(UUID(int=index + 1))
    timing = {
        "schema_version": 1, "request_id": request, "host": "player",
        "scope": "warmup" if index < 2 else "play",
        "outcome": "failed" if failed else "presented",
        "elapsed_us": 60, "cpu_us": 15, "queue_us": 0, "gpu_us": 25,
        "presentation_us": None if failed else 20,
        "first_present_us": None if failed else 60,
    }
    counters = {
        "models": 2, "cpu_retained_source_bytes": 256, "logical_gpu_bytes": 100,
        "logical_gpu_resources": 3, "tracked_gpu": {"allocations": 2, "bytes": 128, "peak_bytes": 160},
    }
    direction = ("forest", "village") if index % 2 == 0 else ("village", "forest")
    switch = {
        "schema_version": 1, "request_id": request, "index": index, "reason": "smoke_cycle",
        "source": direction[0], "destination": direction[1], "active_frame_us": [13, 15, 20],
        "frame_gap_us": None if failed else 64, "before": counters, "settled": None if failed else counters,
        "preparation": {
            "read_us": 4, "simulation_us": 6, "native_assembly_us": 15, "drain_us": 4, "install_us": 3,
            "assets": {"cache_hits": 1, "cache_misses": 1, "static_import_us": 4, "static_upload_us": 5},
            "overlap": dict(counters, logical_gpu_bytes=140),
        },
    }
    return [
        {"component": component, "session_id": session, "message": json.dumps(event)}
        for component, event in (("scene_load", timing), ("scene_switch", switch))
    ]


class SceneSwitchTests(unittest.TestCase):
    def run_report(self, rows):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            profile = root / "profile.json"
            log = root / "engine.jsonl"
            output = root / "report.json"
            profile.write_text(json.dumps(PROFILE), encoding="utf-8")
            log.write_text("\n".join(json.dumps(row) for row in rows), encoding="utf-8")
            errors = StringIO()
            with redirect_stdout(StringIO()), redirect_stderr(errors):
                code = main([
                    "--log", str(log), "--profile", str(profile), "--fixture-sha256", "a" * 64,
                    "--minimum-samples", "1", "--frame-budget-us", "16", "--output", str(output),
                ])
            report = json.loads(output.read_bytes()) if output.exists() else None
            return code, report, errors.getvalue()

    def test_separates_directions_and_excludes_warmups(self):
        rows = [row for index in range(6) for row in sample(index)]
        code, report, errors = self.run_report(reversed(rows))
        self.assertEqual((code, errors), (0, ""))
        self.assertTrue(report["passed"])
        forward, back = report["runs"][0]["directions"]
        self.assertEqual((forward["source"], forward["destination"]), ("forest", "village"))
        self.assertEqual((back["source"], back["destination"]), ("village", "forest"))
        self.assertEqual(forward["attempts"], 2)
        self.assertEqual(forward["source_frames_over_budget"], 2)
        self.assertEqual(forward["switch_gaps_over_budget"], 2)
        self.assertEqual(forward["resources"]["overlap"]["logical_gpu_bytes"]["max"], 140)
        self.assertEqual(forward["resources"]["settled"]["logical_gpu_bytes"]["max"], 100)

    def test_failed_switch_is_reported_and_fails_run(self):
        rows = [row for index in range(4) for row in sample(index, failed=index == 3)]
        code, report, errors = self.run_report(rows)
        self.assertEqual((code, errors), (1, ""))
        direction = report["runs"][0]["directions"][1]
        self.assertEqual(direction["outcomes"], {"failed": 1})
        self.assertIsNone(direction["timing"]["first_present_us"])

    def test_rejects_mismatched_and_duplicate_request(self):
        rows = [row for index in range(4) for row in sample(index)]
        rows[-1]["session_id"] = "different-run"
        code, report, errors = self.run_report(rows)
        self.assertEqual((code, report), (2, None))
        self.assertIn("no matching terminal", errors)
        rows[-1]["session_id"] = "run"
        code, report, errors = self.run_report(rows + [rows[-1]])
        self.assertEqual((code, report), (2, None))
        self.assertIn("Duplicate scene_switch", errors)


if __name__ == "__main__":
    unittest.main()
