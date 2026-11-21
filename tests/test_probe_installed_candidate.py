"""Exercise the detached Windows package probe with isolated directories."""

import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import Mock, patch
from zipfile import ZipFile

from test_release_qualification import FILES, fingerprint, receipt
from player_smoke import FRAME_MARKER, smoke
from probe_installed_candidate import probe


class InstalledCandidateProbeTests(unittest.TestCase):
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
        self.receipt.write_text(json.dumps(self.expected), encoding="utf-8")
        self.output = self.root / "probe"

    def run_probe(self, *, runner=smoke, environment=None):
        return probe(
            self.receipt, self.archive, self.output, "detached-windows",
            environment=environment or (lambda: {"windows": "10.0", "graphics": [{"Name": "Probe GPU"}]}),
            runner=runner,
        )

    def test_launch_uses_verified_bytes_and_separate_working_directory_and_profile(self):
        def process(command, **options):
            self.assertEqual(command[1:], ["--smoke-test"])
            player = Path(command[0])
            self.assertEqual(player, self.output / "delivered-player/game.exe")
            self.assertEqual(player.read_bytes(), FILES["game.exe"])
            self.assertEqual(options["cwd"], self.output / "independent-working-directory")
            self.assertEqual(Path(options["env"]["LOCALAPPDATA"]), self.output / "disposable-profile")
            self.assertEqual(Path(options["env"]["ENGINE_LOG_DIR"]), self.output / "native-logs")
            self.assertNotIn("GAME_ENGINE_NATIVE_EDITOR_DEMO", options["env"])
            options["stdout"].write(FRAME_MARKER + b"\n")
            return subprocess.CompletedProcess(command, 0)

        with patch("player_smoke.subprocess.run", side_effect=process):
            result = self.run_probe()
        self.assertEqual(result["status"], "passed")
        self.assertEqual(result["archive_sha256"], self.expected["archive_sha256"])
        self.assertEqual(result["git_revision"], self.expected["git_revision"])
        self.assertEqual(result["host_profile"], "detached-windows")
        self.assertEqual(
            result["evidence"],
            {name: fingerprint((self.output / name).read_bytes())
             for name in ("clean-player.log", "windows-host.json")},
        )
        self.assertEqual(json.loads((self.output / "clean-player.gate.json").read_bytes()), result)

    def test_mismatched_archive_is_rejected_before_unpacking_or_launching(self):
        wrong = {**self.expected, "archive_sha256": "0" * 64}
        self.receipt.write_text(json.dumps(wrong), encoding="utf-8")
        runner = Mock()
        with self.assertRaisesRegex(ValueError, "Unexpected archive hash"):
            self.run_probe(runner=runner)
        runner.assert_not_called()
        self.assertFalse((self.output / "delivered-player").exists())

    def test_occupied_output_is_not_changed(self):
        self.output.mkdir()
        sentinel = self.output / "keep.txt"
        sentinel.write_bytes(b"previous run")
        runner = Mock()
        with self.assertRaises(FileExistsError):
            self.run_probe(runner=runner)
        runner.assert_not_called()
        self.assertEqual(sentinel.read_bytes(), b"previous run")

    def test_zero_exit_with_missing_exact_first_frame_marker_records_failure(self):
        def process(command, **options):
            options["stdout"].write(FRAME_MARKER + b" unexpected trailing data\n")
            return subprocess.CompletedProcess(command, 0)

        with patch("player_smoke.subprocess.run", side_effect=process):
            result = self.run_probe()
        self.assertEqual(result["status"], "failed")
        self.assertIn("frame marker absent", (self.output / "clean-player.log").read_text(encoding="utf-8"))

    def test_modified_extracted_file_fails_after_the_player_shuts_down(self):
        def change_project(player, _cwd, log, **_options):
            player.with_name("project.json").write_bytes(b"different content")
            log.write_bytes(FRAME_MARKER + b"\n")

        result = self.run_probe(runner=change_project)
        self.assertEqual(result["status"], "failed")
        self.assertIn("differs at project.json", (self.output / "clean-player.log").read_text(encoding="utf-8"))

    def test_missing_host_metadata_keeps_evidence_and_never_starts_the_player(self):
        runner = Mock()
        result = self.run_probe(runner=runner, environment=Mock(side_effect=PermissionError("GPU metadata denied")))
        runner.assert_not_called()
        self.assertEqual(result["status"], "failed")
        metadata = json.loads((self.output / "windows-host.json").read_bytes())
        self.assertEqual(metadata["error"], "GPU metadata denied")
        self.assertIn("clean-player.log", result["evidence"])


if __name__ == "__main__":
    unittest.main()
