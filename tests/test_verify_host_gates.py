"""Check provenance, evidence, and verdicts for the Windows host gates."""

from contextlib import redirect_stdout
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from zipfile import ZipFile

from test_release_qualification import FILES, fingerprint, receipt
from verify_host_gates import gate_result, main, verify_gates


PROFILES = {
    "headless": "workspace-runner",
    "editor": "windows-editor",
    "clean_player": "separate-windows-player",
    "manual_controller": "separate-windows-player",
}


class HostGateTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        self.archive = self.root / "candidate.zip"
        with ZipFile(self.archive, "x") as archive:
            for name, data in FILES.items():
                archive.writestr(name, data)
        self.expected = receipt(self.archive)
        self.receipt = self.root / "qualification.json"
        self.write_json(self.receipt, self.expected)
        self.profiles = self.root / "profiles.json"
        self.write_json(self.profiles, PROFILES)
        self.evidence = self.root / "evidence"
        self.evidence.mkdir()

    @staticmethod
    def write_json(path, record):
        path.write_text(json.dumps(record), encoding="utf-8")

    def write_gate(self, gate, status="passed", **overrides):
        logs = {}
        if status != "not_run":
            log = self.evidence / f"{gate}.log"
            observed = b"Gate observation\n"
            log.write_bytes(observed)
            logs[log.name] = fingerprint(observed)
        record = gate_result(self.expected, gate, PROFILES[gate], status, logs)
        self.write_json(self.evidence / f"{gate}.gate.json", {**record, **overrides})
        return record

    def verify(self):
        return verify_gates(self.receipt, self.archive, self.evidence, self.profiles)

    def verify_cli(self):
        with redirect_stdout(io.StringIO()) as output:
            exit_code = main([
                "verify", str(self.receipt), str(self.archive), str(self.evidence),
                "--profiles", str(self.profiles),
            ])
        return exit_code, json.loads(output.getvalue())

    def test_all_four_passes_on_one_clean_candidate_are_accepted(self):
        expected = {gate: self.write_gate(gate) for gate in PROFILES}
        exit_code, result = self.verify_cli()
        self.assertEqual(exit_code, 0)
        self.assertEqual(result["verdict"], "accepted")
        self.assertEqual(result["gates"], expected)
        self.assertEqual(result["archive_sha256"], self.expected["archive_sha256"])

    def test_missing_and_not_run_results_are_pending(self):
        self.write_gate("headless")
        self.write_gate("manual_controller", "not_run")
        exit_code, result = self.verify_cli()
        self.assertEqual(exit_code, 2)
        self.assertEqual(result["verdict"], "pending")
        self.assertEqual(result["pending_gates"], ["clean_player", "editor", "manual_controller"])

    def test_dirty_candidate_stays_pending_even_when_every_gate_passes(self):
        self.expected["source_dirty"] = True
        self.write_json(self.receipt, self.expected)
        for gate in PROFILES:
            self.write_gate(gate)
        result = self.verify()
        self.assertEqual(result["verdict"], "pending")
        self.assertTrue(result["source_dirty"])
        self.assertEqual(result["pending_gates"], [])

    def test_failed_gate_rejects_and_keeps_diagnostics_and_pending_names(self):
        failed = self.write_gate("clean_player", "failed")
        exit_code, result = self.verify_cli()
        self.assertEqual(exit_code, 1)
        self.assertEqual(result["verdict"], "rejected")
        self.assertEqual(result["failed_gates"], ["clean_player"])
        self.assertEqual(result["gates"]["clean_player"], failed)
        self.assertEqual(result["pending_gates"], ["editor", "headless", "manual_controller"])

    def test_candidate_and_source_provenance_must_match(self):
        for overrides in ({"archive_sha256": "f" * 64}, {"git_revision": "f" * 40}):
            with self.subTest(overrides=overrides):
                self.write_gate("editor", **overrides)
                with self.assertRaisesRegex(ValueError, "another revision or candidate"):
                    self.verify()

    def test_duplicate_gate_and_unknown_status_are_rejected(self):
        record = self.write_gate("clean_player")
        duplicate = self.evidence / "dupe.gate.json"
        self.write_json(duplicate, record)
        with self.assertRaisesRegex(ValueError, "Duplicate result"):
            self.verify()
        duplicate.unlink()
        self.write_json(self.evidence / "clean_player.gate.json", {**record, "status": "complete"})
        with self.assertRaisesRegex(ValueError, "Unsupported gate or status"):
            self.verify()

    def test_profiles_must_name_the_assigned_and_separate_hosts(self):
        self.write_gate("clean_player", host_profile="local-rehearsal")
        with self.assertRaisesRegex(ValueError, "Unexpected host profile"):
            self.verify()
        self.write_json(self.profiles, {**PROFILES, "clean_player": PROFILES["headless"]})
        with self.assertRaisesRegex(ValueError, "needs a separate host profile"):
            self.verify()

    def test_gate_evidence_must_match_current_nonempty_bytes(self):
        record = self.write_gate("headless")
        self.assertEqual(self.verify()["gates"]["headless"], record)
        log = self.evidence / "headless.log"
        log.write_bytes(b"Different observation\n")
        with self.assertRaisesRegex(ValueError, "hash did not match"):
            self.verify()
        log.write_bytes(b"")
        with self.assertRaisesRegex(ValueError, "outside its size limit"):
            self.verify()
        log.write_bytes(b"too large")
        with patch("verify_host_gates.MAX_EVIDENCE_BYTES", 4):
            with self.assertRaisesRegex(ValueError, "outside its size limit"):
                self.verify()

    def test_changed_zip_is_rejected_even_with_no_gate_files(self):
        self.archive.write_bytes(self.archive.read_bytes() + b"changed")
        with self.assertRaisesRegex(ValueError, "Unexpected archive hash"):
            self.verify()

    def test_record_uses_relative_evidence_and_does_not_replace_an_existing_result(self):
        notes = self.evidence / "manual"
        notes.mkdir()
        observation = notes / "controller.txt"
        observation.write_bytes(b"Controller disconnected on confirmation screen\n")
        command = [
            "record", str(self.receipt), str(self.archive), str(self.evidence),
            "--gate", "manual_controller", "--host-profile", PROFILES["manual_controller"],
            "--status", "passed", "--evidence", "manual/controller.txt",
        ]
        self.assertEqual(main(command), 0)
        before = self.evidence / "manual_controller.gate.json"
        saved = before.read_bytes()
        with self.assertRaises(SystemExit) as failure:
            main(command)
        self.assertEqual(failure.exception.code, 3)
        self.assertEqual(before.read_bytes(), saved)
        result = self.verify()
        self.assertEqual(result["gates"]["manual_controller"]["evidence"], {
            "manual/controller.txt": fingerprint(observation.read_bytes()),
        })


if __name__ == "__main__":
    unittest.main()
