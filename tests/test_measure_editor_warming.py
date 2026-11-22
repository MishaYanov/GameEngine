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

    def with_model_records(self, path, enabled, configured=None, **sample_options):
        records = self.evidence(path, enabled, **sample_options)
        for record in list(records):
            if record["component"] != "editor_scene_warming":
                continue
            event = json.loads(record["message"])
            if event["kind"] != "load":
                continue
            matched = enabled and configured != "references" and event["outcome"] == "hit"
            attempt = "references" if configured == "references" else "decode"
            records.append(row("editor_model_warming", {
                "schema_version": 1 if configured is None else 2,
                "kind": "model_load",
                "request_id": event["request_id"],
                "scene_id": event["scene_id"],
                "mode": attempt if event["outcome"] != "unrequested" else "off",
                "batch_matched": matched,
                "prepared_models": 2 if matched else 0,
                "consumed_models": 1 if matched else 0,
                "retained_bytes": 8192 if matched else 0,
                **({"configured_mode": configured} if configured is not None else {}),
            }, record["session_id"]))
        self.write(path, records)
        return records

    def capture(self, path, mode, count=30, cache_bytes=8192):
        cache = {
            "state": "retained", "asset_generation": 0,
            "loaded_models": 2, "gpu_resources": 10, "gpu_bytes": cache_bytes,
        }
        viewport = {"width": PROFILE["width"], "height": PROFILE["height"],
                    "projection": "perspective", "grid": True, "colliders": False}
        record = {"workspace_generation": 1, "pre_switch_cache": cache, "viewport": viewport}
        records = self.with_model_records(path, mode != "off", mode, count=count)
        session = records[0]["session_id"]
        models = {}
        for item in records:
            event = json.loads(item["message"])
            if item["component"] == "editor_model_warming":
                # A retained reference hit and a decoded batch can still yield no adoption.
                event["consumed_models"] = 0
                models[event["request_id"]] = event
                item["message"] = json.dumps(event)
            elif (item["component"] == "editor_scene_warming"
                  and event["kind"] == "active_frame" and (mode != "off" or not event["warming_requested"])):
                item["message"] = json.dumps(event | record | {"schema_version": 2, "mode": mode})
        rows = []
        for item in list(records):
            event = json.loads(item["message"])
            if item["component"] != "editor_scene_warming" or event["kind"] != "load":
                continue
            request = event["request_id"]
            scene = event["scene_id"]
            identity = record | {"source_scene_id": ACTIVE if scene == TARGET else None}
            fields = {"request_id": request, "scene_id": scene, "scene_revision": len(rows) + 1,
                      "mode": mode, "mode_revision": 1}
            records.append(row("editor_warming_session",
                               fields | {"schema_version": 1, "identity": identity}, session))
            terminal = next(json.loads(candidate["message"]) for candidate in records
                            if candidate["component"] == "scene_load"
                            and json.loads(candidate["message"])["request_id"] == request)
            if terminal["outcome"] == "presented":
                model = models[request]
                rows.append(fields | identity | {
                    "outcome": event["outcome"],
                    "references_hit": event["outcome"] == "hit",
                    "prepared_models": model["prepared_models"],
                    "adopted_models": model["consumed_models"],
                    "retained_bytes": model["retained_bytes"],
                })
        report = path.with_suffix(".json")
        report.write_text(json.dumps({"schema_version": 1, "session_id": session,
                                      "workspace_generation": 1, "rows": rows}), encoding="utf-8")
        self.write(path, records)
        return report

    def compare(self, ceiling=105, decode=False, warmed_mode=None, captures=()):
        errors = StringIO()
        with redirect_stdout(StringIO()), redirect_stderr(errors):
            code = main([
                "--baseline", str(self.baseline), "--warmed", str(self.warmed),
                "--scene", TARGET, "--active-scene", ACTIVE, "--fixture-sha256", "b" * 64,
                "--profile", str(self.profile), "--active-ceiling-us", str(ceiling),
                "--output", str(self.output),
            ] + (["--decode-evidence"] if decode else [])
              + (["--warmed-mode", warmed_mode] if warmed_mode else []) + list(captures))
        return code, errors.getvalue()

    def matched(self, baseline, warmed, mode="models", digest="b" * 64, warmed_profile=None):
        return self.compare(warmed_mode=mode, captures=(
            "--baseline-capture", str(baseline), "b" * 64, str(self.profile),
            "--warmed-capture", str(warmed), digest, str(warmed_profile or self.profile),
        ))

    def test_matches_off_and_each_explicit_warmed_mode_with_zero_adoptions(self):
        off = self.capture(self.baseline, "off")
        for mode in ("models", "references"):
            with self.subTest(mode=mode):
                warmed = self.capture(self.warmed, mode)
                self.assertEqual((0, ""), self.matched(off, warmed, mode))
                report = json.loads(self.output.read_text(encoding="utf-8"))
                self.assertEqual(2, report["schema_version"])
                group, = report["comparison_groups"]
                self.assertEqual("measured", group["status"])
                self.assertEqual("retained", group["pre_switch_cache"]["state"])
                self.assertEqual("off", group["baseline"]["mode"])
                self.assertEqual(mode, group["warmed"]["mode"])
                self.assertEqual(1, group["warmed"]["presentation_outcomes"]["cancelled"])
                self.assertEqual(10, group["warmed"]["warming_outcomes"]["hit"])
                self.assertEqual(30, group["warmed"]["first_present_us"]["samples"])
                self.assertEqual(20 if mode == "models" else 0,
                                 group["warmed"]["model_warming"]["prepared_models"])
                self.assertEqual(0, group["warmed"]["model_warming"]["consumed_models"])
                self.output.unlink()

    def test_rejects_mismatched_fixture_profile_and_cache(self):
        off = self.capture(self.baseline, "off")
        warmed = self.capture(self.warmed, "models")
        self.assertIn("same fixture SHA-256", self.matched(off, warmed, digest="c" * 64)[1])
        profile = self.directory / "different-profile.json"
        profile.write_text(json.dumps(PROFILE | {"driver": "another driver"}), encoding="utf-8")
        self.assertIn("same viewport profile", self.matched(off, warmed, warmed_profile=profile)[1])
        warmed = self.capture(self.warmed, "models", cache_bytes=10000)
        self.assertIn("matching native cache conditions", self.matched(off, warmed)[1])
        self.assertFalse(self.output.exists())

    def test_rejects_unpaired_and_mislabeled_capture_rows(self):
        off = self.capture(self.baseline, "off")
        warmed = self.capture(self.warmed, "models")
        report = json.loads(warmed.read_text(encoding="utf-8"))
        original = report["rows"][0]
        report["rows"][0] = original | {"mode_revision": original["mode_revision"] + 1}
        warmed.write_text(json.dumps(report), encoding="utf-8")
        self.assertIn("disagree with the native log", self.matched(off, warmed)[1])
        report["rows"].pop(0)
        warmed.write_text(json.dumps(report), encoding="utf-8")
        self.assertIn("missing its report row", self.matched(off, warmed)[1])
        self.assertFalse(self.output.exists())

    def test_labels_insufficient_matched_samples_without_percentiles(self):
        off = self.capture(self.baseline, "off", count=29)
        warmed = self.capture(self.warmed, "models", count=29)
        self.assertEqual((2, ""), self.matched(off, warmed))
        report = json.loads(self.output.read_text(encoding="utf-8"))
        group, = report["comparison_groups"]
        self.assertEqual("insufficient_matched_samples", group["status"])
        self.assertEqual(29, group["warmed"]["presented_samples"])
        self.assertNotIn("first_present_us", group["warmed"])
        self.assertIsNone(report["active_frame_passed"])

    def test_rejects_non_string_capture_modes_as_input_errors(self):
        off = self.capture(self.baseline, "off")
        warmed = self.capture(self.warmed, "models")
        report = json.loads(warmed.read_bytes())
        report["rows"][0]["mode"] = ["models"]
        warmed.write_text(json.dumps(report), encoding="utf-8")
        code, error = self.matched(off, warmed)
        self.assertEqual(2, code)
        self.assertIn("Invalid native warming mode", error)

        warmed = self.capture(self.warmed, "models")
        records = [json.loads(line) for line in self.warmed.read_bytes().splitlines()]
        identity = next(item for item in records if item["component"] == "editor_warming_session")
        event = json.loads(identity["message"])
        identity["message"] = json.dumps(event | {"mode": {"value": "models"}})
        self.write(self.warmed, records)
        code, error = self.matched(off, warmed)
        self.assertEqual(2, code)
        self.assertIn("Invalid editor warming session record", error)
        self.assertFalse(self.output.exists())

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

    def test_reports_references_mode_with_no_model_adoptions(self):
        self.with_model_records(self.baseline, False, configured="off")
        self.with_model_records(self.warmed, True, configured="references")
        self.assertEqual(self.compare(warmed_mode="references"), (0, ""))
        report = json.loads(self.output.read_bytes())
        self.assertEqual(report["warmed_mode"], "references")
        self.assertEqual(report["warmed"]["model_warming"]["consumed_models"], 0)

    def test_rejects_configured_baseline_mode_and_mislabeled_model_records(self):
        self.with_model_records(self.baseline, False, configured="references")
        self.with_model_records(self.warmed, True, configured="models")
        self.assertIn("Baseline capture needs scene warming configured Off", self.compare(warmed_mode="models")[1])
        self.with_model_records(self.baseline, False, configured="off")
        self.assertIn("does not match its scene or capture mode", self.compare(warmed_mode="references")[1])
        records = self.with_model_records(self.warmed, True, configured="models")
        index = next(index for index, record in enumerate(records)
                     if record["component"] == "editor_model_warming" and
                     json.loads(record["message"])["mode"] == "decode")
        event = json.loads(records[index]["message"])
        records[index]["message"] = json.dumps({**event, "configured_mode": "references"})
        self.write(self.warmed, records)
        self.assertIn("does not match its configured mode", self.compare(warmed_mode="models")[1])

    def test_rejects_off_mode_target_presentation_from_a_model_capture(self):
        self.with_model_records(self.baseline, False, configured="off")
        records = self.with_model_records(self.warmed, True, configured="models")
        index = next(index for index, record in enumerate(records)
                     if record["component"] == "editor_model_warming"
                     and json.loads(record["message"])["scene_id"] == TARGET
                     and json.loads(record["message"])["mode"] == "off")
        event = json.loads(records[index]["message"])
        records[index]["message"] = json.dumps({**event, "configured_mode": "off"})
        self.write(self.warmed, records)
        self.assertIn("Warmed target loads must use the configured capture mode",
                      self.compare(warmed_mode="models")[1])


if __name__ == "__main__":
    unittest.main()
