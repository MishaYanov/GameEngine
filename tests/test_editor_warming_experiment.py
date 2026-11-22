"""Exercise the local experiment matrix using native-shaped disposable evidence."""

from contextlib import redirect_stderr
from hashlib import sha256
from io import StringIO
import json
import tempfile
import unittest
from pathlib import Path
from uuid import UUID

from editor_warming_experiment import main
import test_measure_editor_warming as native
from test_measure_scene_load import PROFILE


class WarmingExperimentTests(unittest.TestCase):
    def setUp(self):
        scratch = tempfile.TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        self.root = Path(scratch.name)
        self.profile = self.write("profile.json", PROFILE)
        self.plan_value = {
            "schema_version": 1, "name": "chapter-switch", "fixture_sha256": "b" * 64,
            "profile_sha256": self.digest(self.profile),
            "scene_pairs": [{"source": native.ACTIVE, "target": native.TARGET}],
            "modes": ["off", "models"], "cache_states": ["retained"],
            "repeats": 3, "presentations_per_repeat": 10,
        }
        self.plan = self.write("plan.json", self.plan_value)
        self.budget_value = {
            "schema_version": 1, "plan_name": "chapter-switch", "plan_sha256": self.digest(self.plan),
            "max_first_present_p95_increase_us": 0, "max_active_frame_p95_us": 100,
        }
        self.budget = self.write("budget.json", self.budget_value)
        self.output = self.root / "report.json"
        self.run_count = 0
        self.native = native.EditorWarmingBenchmarkTests()

    def write(self, name, value):
        path = self.root / name
        path.write_text(json.dumps(value), encoding="utf-8")
        return path

    def digest(self, path):
        return sha256(path.read_bytes()).hexdigest()

    def cli(self, command, *runs):
        arguments = [command, "--plan", str(self.plan), "--profile", str(self.profile),
                     "--output", str(self.output)]
        if command == "budget-review":
            arguments += ["--budget", str(self.budget)]
        for files in runs:
            arguments.extend(("--run", *map(str, files)))
        errors = StringIO()
        with redirect_stderr(errors):
            code = main(arguments)
        return code, errors.getvalue()

    def report(self):
        value = json.loads(self.output.read_bytes())
        self.output.unlink()
        return value

    def capture(self, mode, repeat, *, count=10, other_view=False):
        self.run_count += 1
        log = self.root / f"run-{self.run_count}.jsonl"
        report = self.native.capture(log, mode, count=count)
        new_session = f"native-run-{self.run_count}"

        def update(record):
            if "request_id" in record:
                record["request_id"] = str(UUID(int=UUID(record["request_id"]).int + self.run_count * 10000))
            if other_view and "viewport" in record:
                record["viewport"]["grid"] = False

        records = [json.loads(line) for line in log.read_bytes().splitlines()]
        for event in records:
            body = json.loads(event["message"])
            update(body)
            if "identity" in body:
                update(body["identity"])
            event["session_id"] = new_session
            event["message"] = json.dumps(body)
        self.native.write(log, records)
        capture = json.loads(report.read_bytes())
        capture["session_id"] = new_session
        for row in capture["rows"]:
            update(row)
        self.write(report.name, capture)
        metadata = self.write(f"metadata-{self.run_count}.json", {
            "schema_version": 1, "plan_sha256": self.digest(self.plan),
            "fixture_sha256": self.plan_value["fixture_sha256"],
            "profile_sha256": self.plan_value["profile_sha256"],
            "slot": {"source": native.ACTIVE, "target": native.TARGET, "mode": mode,
                     "cache_state": "retained", "repeat": repeat},
        })
        return metadata, report, log

    def all_runs(self, **options):
        return [self.capture(mode, repeat, **(options if mode == "models" else {}))
                for mode in ("off", "models") for repeat in range(1, 4)]

    def test_plan_uses_exact_profile_and_bounded_unique_slots(self):
        self.assertEqual(self.cli("plan"), (0, ""))
        self.assertEqual(len(self.report()["slots"]), 6)
        for update in (
            {"scene_pairs": self.plan_value["scene_pairs"] * 2},
            {"scene_pairs": [{"source": "arena", "target": "arena"}]},
            {"modes": ["off", "third-mode"]},
            {"repeats": 11},
            {"presentations_per_repeat": 16},
            {"profile_sha256": "c" * 64},
        ):
            with self.subTest(update=update):
                self.write("plan.json", self.plan_value | update)
                self.assertEqual(self.cli("plan")[0], 3)
                self.assertFalse(self.output.exists())

    def test_coverage_and_budget_bind_real_matches_include_cancellations_and_zero_adoption(self):
        runs = self.all_runs()
        self.assertEqual(self.cli("plan-coverage", *runs[:-1])[0], 2)
        partial = self.report()
        self.assertEqual(partial["coverage"]["missing"][0]["repeat"], 3)
        self.assertEqual(partial["comparisons"][0]["reason"], "incomplete_coverage")
        self.assertEqual(self.cli("plan-coverage", *runs), (0, ""))
        coverage = self.report()["coverage"]
        self.assertEqual(self.cli("budget-review", *runs), (0, ""))
        report = self.report()
        self.assertEqual(report["coverage"], coverage)
        measured, = report["comparisons"]
        self.assertEqual(measured["status"], "within_budget")
        self.assertEqual(measured["warmed"]["active_frame_us"]["p95"], 100)
        self.assertEqual(measured["warmed"]["active_frame_us"]["sample_stddev"], 0.0)
        self.assertEqual(measured["warmed"]["model_warming"]["zero_adoption_samples"], 30)
        self.assertEqual(measured["warmed"]["presentation_outcomes"]["cancelled"], 3)
        self.write("budget.json", self.budget_value | {"max_active_frame_p95_us": 99})
        self.assertEqual(self.cli("budget-review", *runs)[0], 1)
        self.assertEqual(self.report()["comparisons"][0]["status"], "over_budget")

    def test_cross_viewport_comparison_and_short_capture_have_no_timing_verdict(self):
        runs = self.all_runs(other_view=True)
        self.assertEqual(self.cli("budget-review", *runs)[0], 2)
        results = self.report()["comparisons"]
        self.assertEqual(len(results), 2)
        self.assertTrue(all(row["status"] == "insufficient_measurements" for row in results))
        runs[-1] = self.capture("models", 3, count=9)
        self.assertEqual(self.cli("plan-coverage", *runs)[0], 2)
        coverage = self.report()["coverage"]
        short, = [row for row in coverage["runs"] if row["status"] == "short"]
        self.assertEqual(short["qualifying_presentations"], 9)
        self.assertEqual(short["observed"][0]["presentation_outcomes"]["cancelled"], 1)

    def test_invalid_budget_duplicate_capture_and_occupied_output_do_not_write(self):
        run = self.capture("off", 1)
        duplicate = self.capture("off", 2)
        reused = (duplicate[0], run[1], run[2])
        self.assertIn("separate native session", self.cli("plan-coverage", run, reused)[1])
        self.assertIn("Duplicate warming run slot", self.cli("plan-coverage", run, run)[1])
        for patch in ({"plan_sha256": "d" * 64}, {"unknown": 2}, {"max_active_frame_p95_us": True}):
            with self.subTest(patch=patch):
                self.write("budget.json", self.budget_value | patch)
                self.assertEqual(self.cli("budget-review")[0], 3)
                self.assertFalse(self.output.exists())
        self.output.write_bytes(b"occupied")
        self.assertEqual(self.cli("plan")[0], 3)
        self.assertEqual(self.output.read_bytes(), b"occupied")


if __name__ == "__main__":
    unittest.main()
