"""Exercise the native editor warming benchmark input and report contract."""

from contextlib import redirect_stderr, redirect_stdout
from io import StringIO
import json
from pathlib import Path
import tempfile
import unittest

from test_measure_scene_load import PROFILE, load
from measure_editor_warming import main


TARGET = "garden"
ACTIVE = "courtyard"


def row(component, message, session):
    return {"component": component, "message": json.dumps(message), "session_id": session}


def active_frame(scene, warming, elapsed):
    return {
        "schema_version": 1,
        "kind": "active_frame",
        "scene_id": scene,
        "warming_requested": warming,
        "elapsed_us": elapsed,
    }


class EditorWarmingBenchmarkTests(unittest.TestCase):
    def setUp(self):
        scratch = tempfile.TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        self.directory = Path(scratch.name)
        self.profile = self.directory / "profile.json"
        self.profile.write_text(json.dumps(PROFILE), encoding="utf-8")
        self.baseline = self.directory / "baseline.jsonl"
        self.warmed = self.directory / "warmed.jsonl"
        self.output = self.directory / "report.json"

    def evidence(self, path, enabled, count=30, frame_count=30, elapsed=100):
        session = "warming-native" if enabled else "baseline-native"
        base = 1000 if enabled else 0
        records = []
        for index in range(count + 2):
            terminal = load(base + index + 1, scope="authored", host="editor",
                            cpu=(15 if enabled else 40) + index)
            disposition = "unrequested"
            if enabled and index < count:
                disposition = ("hit", "miss", "stale")[index % 3]
            if index == count:
                terminal.update(outcome="cancelled", first_present_us=None, presentation_us=None)
            records.extend([
                row("editor_scene_warming", {
                    "schema_version": 1,
                    "kind": "load",
                    "request_id": terminal["request_id"],
                    "scene_id": TARGET if index <= count else "other",
                    "outcome": disposition,
                }, session),
                row("scene_load", terminal, session),
            ])
        for _ in range(frame_count):
            records.append(row("editor_scene_warming", active_frame(ACTIVE, enabled, elapsed), session))
        records.append(row("editor_scene_warming", active_frame("other", True, 1_000_000), session))
        if enabled:
            records.append(row("editor_scene_warming", active_frame(ACTIVE, False, 1_000_000), session))
        records.extend([
            row("scene_load", load(base + 500, scope="play", host="editor"), session),
            row("scene_load", load(base + 501, scope="authored", host="player"), session),
        ])
        self.write(path, records)
        return records

    def write(self, path, records):
        path.write_text("".join(json.dumps(event) + "\n" for event in records), encoding="utf-8")

    def with_model_records(self, path, enabled):
        records = self.evidence(path, enabled)
        for record in list(records):
            if record["component"] != "editor_scene_warming":
                continue
            event = json.loads(record["message"])
            if event["kind"] != "load":
                continue
            matched = enabled and event["outcome"] == "hit"
            records.append(row("editor_model_warming", {
                "schema_version": 1,
                "kind": "model_load",
                "request_id": event["request_id"],
                "scene_id": event["scene_id"],
                "mode": "decode" if event["outcome"] != "unrequested" else "off",
                "batch_matched": matched,
                "prepared_models": 2 if matched else 0,
                "consumed_models": 1 if matched else 0,
                "retained_bytes": 8192 if matched else 0,
            }, record["session_id"]))
        self.write(path, records)
        return records

    def compare(self, ceiling=105, decode=False):
        errors = StringIO()
        with redirect_stdout(StringIO()), redirect_stderr(errors):
            code = main([
                "--baseline", str(self.baseline), "--warmed", str(self.warmed),
                "--scene", TARGET, "--active-scene", ACTIVE, "--fixture-sha256", "b" * 64,
                "--profile", str(self.profile), "--active-ceiling-us", str(ceiling),
                "--output", str(self.output),
            ] + (["--decode-evidence"] if decode else []))
        return code, errors.getvalue()

    def test_joins_same_session_and_reports_selected_native_results(self):
        self.evidence(self.baseline, False)
        self.evidence(self.warmed, True, elapsed=105)
        self.assertEqual(self.compare(), (0, ""))
        report = json.loads(self.output.read_bytes())
        self.assertEqual(report["profile"], PROFILE)
        self.assertTrue(report["active_frame_passed"])
        self.assertEqual(report["active_scene_id"], ACTIVE)
        self.assertEqual(report["baseline"]["first_present_us"], {"samples": 30, "median": 69.5, "p95": 83})
        self.assertEqual(report["warmed"]["first_present_us"], {"samples": 30, "median": 44.5, "p95": 58})
        self.assertEqual(report["warmed"]["active_frame_us"], {"samples": 30, "median": 105.0, "p95": 105})
        self.assertEqual(report["warmed"]["warming_outcomes"], {
            "unrequested": 1, "hit": 10, "miss": 10, "stale": 10,
        })
        self.assertEqual(report["warmed"]["presentation_outcomes"], {
            "presented": 30, "cancelled": 1, "failed": 0,
        })

    def test_ceiling_regression_preserves_report_and_existing_file(self):
        self.evidence(self.baseline, False)
        self.evidence(self.warmed, True, elapsed=106)
        self.assertEqual(self.compare(), (1, ""))
        original = self.output.read_bytes()
        self.assertFalse(json.loads(original)["active_frame_passed"])
        self.assertEqual(self.compare()[0], 2)
        self.assertEqual(self.output.read_bytes(), original)

    def test_requires_enough_target_presentations_and_matching_active_frames(self):
        self.evidence(self.baseline, False)
        self.evidence(self.warmed, True, count=29)
        self.assertIn("Expected 30 to 200 presented garden loads", self.compare()[1])
        self.evidence(self.warmed, True, frame_count=29)
        self.assertIn("Expected at least 30 eligible courtyard active frames", self.compare()[1])
        self.evidence(self.warmed, True, count=31)
        self.assertIn("need equal presented target load counts", self.compare()[1])
        self.assertFalse(self.output.exists())

    def test_rejects_missing_cross_session_and_duplicate_request_records(self):
        self.evidence(self.baseline, False)
        records = self.evidence(self.warmed, True)
        self.write(self.warmed, records[1:])
        self.assertIn("needs one authored terminal in the same session", self.compare()[1])
        records[0]["session_id"] = "some-other-session"
        self.write(self.warmed, records)
        self.assertIn("needs one authored terminal in the same session", self.compare()[1])
        self.write(self.warmed, records + [records[1]])
        self.assertIn("Duplicate terminal record", self.compare()[1])
        self.assertFalse(self.output.exists())

    def test_rejects_mixed_baseline_and_malformed_warming_rows(self):
        baseline = self.evidence(self.baseline, False)
        self.evidence(self.warmed, True)
        self.write(self.baseline, baseline + [
            row("editor_scene_warming", active_frame(ACTIVE, True, 100), "baseline-native"),
        ])
        self.assertIn("Baseline active frames include a warming request", self.compare()[1])
        self.write(self.baseline, baseline + [
            row("editor_scene_warming", active_frame(ACTIVE, 1, 100), "baseline-native"),
        ])
        self.assertIn("Invalid active-frame measurement", self.compare()[1])
        self.assertFalse(self.output.exists())

    def test_rejects_reused_native_session_across_modes(self):
        self.evidence(self.baseline, False)
        records = self.evidence(self.warmed, True)
        for record in records:
            record["session_id"] = "baseline-native"
        self.write(self.warmed, records)
        self.assertIn("must use independent native runs", self.compare()[1])
        self.assertFalse(self.output.exists())


    def test_reports_only_presented_target_adoptions_with_matching_session(self):
        self.with_model_records(self.baseline, False)
        self.with_model_records(self.warmed, True)
        self.assertEqual(self.compare(decode=True), (0, ""))
        report = json.loads(self.output.read_bytes())
        self.assertEqual(report["warmed"]["model_warming"], {
            "matched_batches": 10, "prepared_models": 20, "consumed_models": 10,
            "max_retained_bytes": 8192,
        })
        self.assertEqual(report["baseline"]["model_warming"]["consumed_models"], 0)

    def test_rejects_required_model_event_missing_from_session_or_repeated(self):
        self.with_model_records(self.baseline, False)
        records = self.with_model_records(self.warmed, True)
        self.write(self.warmed, records[:-1])
        self.assertIn("needs one model record in the same session", self.compare(decode=True)[1])
        record = {**records[-1], "session_id": "wrong-native-session"}
        self.write(self.warmed, [*records[:-1], record])
        self.assertIn("needs one model record in the same session", self.compare(decode=True)[1])
        self.write(self.warmed, [*records, records[-1]])
        self.assertIn("Duplicate model_load record", self.compare(decode=True)[1])

    def test_rejects_oversized_batch_and_references_only_decode_evidence(self):
        self.with_model_records(self.baseline, False)
        records = self.with_model_records(self.warmed, True)
        index = next(index for index, record in enumerate(records)
                     if record["component"] == "editor_model_warming"
                     and json.loads(record["message"])["batch_matched"])
        row = records[index]
        event = json.loads(row["message"])
        self.write(self.warmed, [*records[:index], {
            **row, "message": json.dumps({**event, "retained_bytes": 16 * 1024 * 1024 + 1})
        }, *records[index + 1:]])
        self.assertIn("retained-byte budget", self.compare(decode=True)[1])
        self.write(self.warmed, [*records[:index], {
            **row, "message": json.dumps({**event, "mode": "references", "prepared_models": 0,
                                         "batch_matched": False, "consumed_models": 0,
                                         "retained_bytes": 0})
        }, *records[index + 1:]])
        self.assertIn("does not match its scene or capture mode", self.compare(decode=True)[1])


if __name__ == "__main__":
    unittest.main()
