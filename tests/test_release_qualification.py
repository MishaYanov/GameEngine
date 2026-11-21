"""Exercise release qualification and downloaded package verification."""

from hashlib import sha256
import json
from pathlib import Path
import stat
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
import warnings
from zipfile import ZipFile, ZipInfo


sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
import qualify_release
from player_smoke import FRAME_MARKER
from rehearse_release import digest, inventory
from verify_candidate import read_receipt, verify_candidate


FILES = {"game.exe": b"copied player", "project.json": b"{}", "assets/scene.json": b"scene"}


def fingerprint(data):
    return sha256(data).hexdigest()


def receipt(archive, files=FILES):
    return {
        "schema_version": 1,
        "git_revision": "a" * 40,
        "source_dirty": False,
        "authored_manifest_sha256": fingerprint(b"{}"),
        "archive_sha256": digest(archive),
        "package_files": {name: fingerprint(data) for name, data in files.items()},
        "smoke": {"player-default": "passed", "player-arrows": "passed"},
    }


class ReleaseQualificationTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        self.manifest = self.root / "authored.json"
        self.manifest.write_bytes(b"{}")
        self.run_dir = self.root / "run"
        self.process_root = self.run_dir / "staging" / "courtyard-test"

    def fake_process(self, command, **options):
        output = options["stdout"]
        if command[1] == "-B":
            package = self.process_root / "release" / "Courtyard"
            for name, data in FILES.items():
                path = package / name
                path.parent.mkdir(exist_ok=True, parents=True)
                path.write_bytes(data)
            report = {
                "export_directory": str(package),
                "occupied_stage_rejected": True,
                "authored_manifest_sha256": digest(self.manifest),
                "package_files": inventory(package),
            }
            (self.process_root / "export-report.json").write_text(json.dumps(report), encoding="utf-8")
            output.write(b"Exported the fixture\n")
        else:
            self.assertEqual(options["cwd"], self.run_dir / "independent-working-directory")
            self.assertEqual(
                Path(options["env"]["ENGINE_LOG_DIR"]), self.run_dir / "native-logs"
            )
            self.assertEqual(Path(command[0]), self.process_root / "copied-player/game.exe")
            output.write(FRAME_MARKER + b"\n")
        return subprocess.CompletedProcess(command, 0)

    def run_candidate(self, fake_process, *, require_clean=False, dirty_before=b"", dirty_after=b""):
        with (
            patch.object(
                qualify_release, "_git",
                side_effect=[b"a" * 40, dirty_before, b"a" * 40, dirty_after],
            ),
            patch.object(qualify_release.subprocess, "run", side_effect=fake_process),
        ):
            return qualify_release.qualify(self.manifest, self.run_dir, require_clean=require_clean)

    def test_clean_mode_rejects_uncommitted_changes_before_running_children(self):
        with self.assertRaisesRegex(ValueError, "requires a clean checkout"):
            self.run_candidate(
                lambda *args, **kwargs: self.fail("unexpected child"),
                require_clean=True, dirty_before=b" M Cargo.toml",
            )
        self.assertFalse(self.run_dir.exists())

    def test_clean_mode_does_not_publish_if_tree_changes_during_export(self):
        with self.assertRaisesRegex(ValueError, "changed during Windows qualification"):
            self.run_candidate(self.fake_process, require_clean=True, dirty_after=b" M Cargo.toml")
        self.assertTrue((self.run_dir / "export.log").is_file())
        self.assertFalse((self.run_dir / "qualification.json").exists())

    def test_local_candidate_reports_an_edit_detected_after_export(self):
        result = self.run_candidate(self.fake_process, dirty_after=b" M Cargo.toml")
        self.assertTrue(read_receipt(result / "qualification.json")["source_dirty"])

    def test_archives_exact_copy_and_runs_both_options(self):
        self.commands = []

        def process(command, **options):
            self.commands.append(command)
            return self.fake_process(command, **options)

        result = self.run_candidate(process)
        self.assertEqual(result, self.run_dir)
        self.assertEqual(self.commands[1][1:], ["--smoke-test"])
        self.assertEqual(
            self.commands[2][1:], ["--smoke-test", "--input-profile", str(qualify_release.PROFILE)]
        )
        result_receipt = read_receipt(result / "qualification.json")
        verify_candidate(result / "candidate.zip", result_receipt)
        self.assertEqual(result_receipt["package_files"], receipt(result / "candidate.zip")["package_files"])

    def test_refuses_an_existing_run_without_launching_any_child(self):
        self.run_dir.mkdir()
        sentinel = self.run_dir / "keep.txt"
        sentinel.write_text("previous run", encoding="utf-8")
        with self.assertRaises(FileExistsError):
            self.run_candidate(lambda *args, **kwargs: self.fail("unexpected child"))
        self.assertEqual(sentinel.read_text(encoding="utf-8"), "previous run")

    def test_missing_smoke_marker_retains_log_without_receipt(self):
        def process(command, **options):
            if command[1] == "--smoke-test":
                options["stdout"].write(b"Exited without a frame\n")
                return subprocess.CompletedProcess(command, 0)
            return self.fake_process(command, **options)

        with self.assertRaisesRegex(RuntimeError, "frame marker absent"):
            self.run_candidate(process)
        self.assertTrue((self.run_dir / "player-default.log").is_file())
        self.assertFalse((self.run_dir / "candidate.zip").exists())
        self.assertFalse((self.run_dir / "qualification.json").exists())

    def test_nonzero_smoke_exit_retains_log_without_receipt(self):
        def process(command, **options):
            if "--input-profile" in command:
                options["stdout"].write(b"Native initialization failed\n")
                raise subprocess.CalledProcessError(9, command)
            return self.fake_process(command, **options)

        with self.assertRaisesRegex(RuntimeError, "player-arrows exited with code 9"):
            self.run_candidate(process)
        self.assertTrue((self.run_dir / "player-arrows.log").is_file())
        self.assertFalse((self.run_dir / "qualification.json").exists())

    def test_missing_package_file_fails_before_smoke(self):
        def process(command, **options):
            self.assertEqual(command[1], "-B")
            completed = self.fake_process(command, **options)
            (self.process_root / "release/Courtyard/assets/scene.json").unlink()
            return completed

        with self.assertRaisesRegex(ValueError, "differs from the recorded inventory"):
            self.run_candidate(process)
        self.assertFalse((self.run_dir / "player-default.log").exists())


class DownloadVerificationTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.archive = Path(directory.name) / "candidate.zip"

    def make_zip(self, members):
        with ZipFile(self.archive, "x") as output:
            for name, data in members:
                output.writestr(name, data)

    def test_changed_archive_digest_is_rejected(self):
        self.make_zip(FILES.items())
        expected = receipt(self.archive)
        expected["archive_sha256"] = "0" * 64
        with self.assertRaisesRegex(ValueError, "Unexpected archive hash"):
            verify_candidate(self.archive, expected)

    def test_missing_archive_member_is_rejected(self):
        self.make_zip([("game.exe", FILES["game.exe"]), ("project.json", FILES["project.json"])])
        with self.assertRaisesRegex(ValueError, "Unexpected ZIP inventory"):
            verify_candidate(self.archive, receipt(self.archive))

    def test_duplicate_archive_member_is_rejected(self):
        with warnings.catch_warnings():
            warnings.filterwarnings("ignore", message="Duplicate name", category=UserWarning)
            self.make_zip([*FILES.items(), ("game.exe", FILES["game.exe"])])
        with self.assertRaisesRegex(ValueError, "Unexpected ZIP inventory"):
            verify_candidate(self.archive, receipt(self.archive))

    def test_changed_member_digest_is_rejected(self):
        self.make_zip({**FILES, "assets/scene.json": b"changed"}.items())
        with self.assertRaisesRegex(ValueError, "Unexpected member hash"):
            verify_candidate(self.archive, receipt(self.archive))

    def test_unconfined_or_ambiguous_windows_paths_are_rejected(self):
        self.make_zip(FILES.items())
        for name in ("../outside.txt", "C:/absolute.txt", "assets/another\\file", "assets/NUL.txt"):
            with self.subTest(name=name), self.assertRaisesRegex(ValueError, "Unsafe package path"):
                verify_candidate(self.archive, receipt(self.archive, {**FILES, name: b"unused"}))
        with self.assertRaisesRegex(ValueError, "Duplicate Windows package path"):
            verify_candidate(self.archive, receipt(self.archive, {**FILES, "GAME.EXE": b"other"}))

    def test_zip_link_metadata_is_rejected(self):
        other = ZipInfo("assets/scene.json")
        other.create_system = 3
        other.external_attr = stat.S_IFLNK << 16
        self.make_zip([
            ("game.exe", FILES["game.exe"]),
            ("project.json", FILES["project.json"]),
            (other, FILES["assets/scene.json"]),
        ])
        with self.assertRaisesRegex(ValueError, "Unsupported package member"):
            verify_candidate(self.archive, receipt(self.archive))


if __name__ == "__main__":
    unittest.main()
