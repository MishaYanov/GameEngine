"""Exercise a fixed, local playtest handoff against a trusted candidate."""

from hashlib import sha256
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from zipfile import ZipFile


ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
from collect_playtest_evidence import EVIDENCE, REPORT, TRACE, collect, verify


def fingerprint(data):
    return sha256(data).hexdigest()


class CollectPlaytestEvidenceTests(unittest.TestCase):
    def setUp(self):
        work = tempfile.TemporaryDirectory()
        self.addCleanup(work.cleanup)
        self.root = Path(work.name)
        self.archive = self.root / "candidate.zip"
        self.receipt = self.root / "qualification.json"
        self.trace = self.root / "input.json"
        self.report = self.root / "report.json"
        self.output = self.root / "handoff"
        project = b'{"project":"courtyard"}\n'
        with ZipFile(self.archive, "w") as candidate:
            candidate.writestr("game.exe", b"fixture")
            candidate.writestr("project.json", project)
        self.receipt.write_text(json.dumps({
            "schema_version": 1,
            "git_revision": "a" * 40,
            "source_dirty": False,
            "authored_manifest_sha256": fingerprint(project),
            "archive_sha256": fingerprint(self.archive.read_bytes()),
            "package_files": {
                "game.exe": fingerprint(b"fixture"),
                "project.json": fingerprint(project),
            },
            "smoke": {"player-default": "passed", "player-arrows": "passed"},
        }), encoding="utf-8")
        self.data = {
            "schema_version": 5,
            "actions": [
                {"type": "command", "command": {"type": "start_play"}},
                {"type": "keyboard_input", "keys": ["KeyE"]},
                {"type": "tick", "count": 1},
                {"type": "mark", "label": "gate"},
            ],
        }
        self.summary = {
            "schema_version": 1,
            "replay_identity": {"project_manifest_sha256": fingerprint(project)},
            "active_scene_id": "courtyard",
            "play_scene_id": "courtyard",
            "simulation_tick": 1,
            "play_status": {"gameplay": {"completed": False, "score": 1}},
            "markers": [{
                "label": "gate", "scene_id": "courtyard", "simulation_tick": 1,
                "completed": False, "score": 1,
                "has_courtyard_key": True, "route_gate_open": True,
            }],
            "diagnostics": "Local source details must stay out of the handoff",
        }
        self.write_inputs()

    def write_inputs(self):
        self.trace.write_text(json.dumps(self.data), encoding="utf-8")
        self.summary["replay_identity"]["replay_sha256"] = fingerprint(self.trace.read_bytes())
        self.report.write_text(json.dumps(self.summary), encoding="utf-8")

    def collect(self, **options):
        collect(self.receipt, self.archive, self.trace, self.report, self.output, **options)

    def test_handoff_only_copies_three_files_and_verifies_from_another_directory(self):
        sentinel = self.root / "unaltered.txt"
        sentinel.write_text("existing")
        self.collect(metadata={"platform": "Windows", "input_method": "Controller"})
        self.assertEqual({item.name for item in self.output.iterdir()}, {TRACE, REPORT, EVIDENCE})
        self.assertEqual((self.output / TRACE).read_bytes(), self.trace.read_bytes())
        self.assertNotIn("diagnostics", json.loads((self.output / REPORT).read_text()))
        self.assertEqual(json.loads((self.output / EVIDENCE).read_text())["test"]["platform"], "Windows")
        workdir = self.root / "independent"
        workdir.mkdir()
        result = subprocess.run([
            sys.executable, "-B", str(ROOT / "scripts/collect_playtest_evidence.py"),
            "verify", str(self.receipt), str(self.archive), str(self.output),
        ], cwd=workdir, capture_output=True, check=False)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(sentinel.read_text(), "existing")
        with self.assertRaises(FileExistsError):
            self.collect()

    def test_changed_trace_and_receipt_are_rejected(self):
        self.collect()
        trace = self.output / TRACE
        original = trace.read_bytes()
        trace.write_bytes(original + b" ")
        with self.assertRaisesRegex(ValueError, "file hash"):
            verify(self.receipt, self.archive, self.output)
        trace.write_bytes(original)
        self.receipt.write_bytes(self.receipt.read_bytes() + b" ")
        with self.assertRaisesRegex(ValueError, "different candidate receipt"):
            verify(self.receipt, self.archive, self.output)

    def test_rejects_mismatched_report_identity_and_marker_labels(self):
        identity = self.summary["replay_identity"]
        previous = identity["project_manifest_sha256"]
        identity["project_manifest_sha256"] = "0" * 64
        self.write_inputs()
        with self.assertRaisesRegex(ValueError, "candidate manifest"):
            self.collect()
        self.assertFalse(self.output.exists())
        identity["project_manifest_sha256"] = previous
        self.summary["markers"][0]["label"] = "wrong-mark"
        self.write_inputs()
        with self.assertRaisesRegex(ValueError, "marker labels"):
            self.collect()
        self.assertFalse(self.output.exists())

    def test_recorded_v4_trace_can_be_shared_without_marks(self):
        self.data["schema_version"] = 4
        self.data["actions"].pop()
        del self.summary["markers"]
        self.write_inputs()
        self.collect()
        verify(self.receipt, self.archive, self.output)
        self.assertEqual(json.loads((self.output / REPORT).read_text())["markers"], [])

    def test_unsupported_commands_fail_before_creating_the_handoff(self):
        self.data["actions"].insert(1, {
            "type": "command", "command": {"type": "spawn", "name": "local authored text"},
        })
        self.write_inputs()
        with self.assertRaisesRegex(ValueError, "unsupported playtest action"):
            self.collect()
        self.assertFalse(self.output.exists())
