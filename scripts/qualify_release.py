"""Build and verify a Windows game release candidate."""

import argparse
import json
import os
from pathlib import Path
import shutil
import stat
import subprocess
import sys
from zipfile import BadZipFile, ZIP_DEFLATED, ZipFile

from player_smoke import run_logged, smoke
from rehearse_release import ROOT, digest, ordinary_mode, verify_copy
from verify_candidate import SMOKE_LABELS, read_receipt, verify_candidate


BUILD_TIMEOUT_SECONDS = 45 * 60
PROFILE = ROOT / "apps/game/tests/fixtures/rehearsal-arrows.json"


def _git(*arguments):
    return subprocess.check_output(
        ["git", "--no-optional-locks", *arguments], cwd=ROOT, timeout=30
    ).strip()


def _rehearse(manifest, run_dir):
    staging = run_dir / "staging"
    run_logged(
        [sys.executable, "-B", str(ROOT / "scripts/rehearse_release.py"), str(manifest), str(staging)],
        ROOT,
        run_dir / "export.log",
        BUILD_TIMEOUT_SECONDS,
        env={**os.environ, "ENGINE_LOG_DIR": str(run_dir / "export-engine-logs")},
    )
    if not stat.S_ISDIR(ordinary_mode(staging)):
        raise ValueError("Rehearsal staging must be an ordinary directory")
    reports = list(staging.glob("*/export-report.json"))
    if len(reports) != 1:
        raise ValueError("Expected exactly one rehearsal report. See export.log")
    report_path = reports[0]
    rehearsal = report_path.parent
    if not stat.S_ISDIR(ordinary_mode(rehearsal)) or not stat.S_ISREG(ordinary_mode(report_path)):
        raise ValueError("Rehearsal report must be an ordinary file under its staging directory")
    report = read_receipt(report_path)
    if not isinstance(report, dict) or report.get("occupied_stage_rejected") is not True:
        raise ValueError("Rehearsal report lacks occupied-stage verification")
    export = Path(report["export_directory"])
    if not export.is_absolute() or export.resolve(strict=True) == rehearsal.resolve(strict=True):
        raise ValueError("Rehearsal report has an invalid export directory")
    verify_copy(report_path, export)
    return report_path, export, report


def qualify(manifest, run_dir, *, require_clean=False):
    manifest = manifest.resolve()
    run_dir = run_dir.resolve()
    revision = _git("rev-parse", "--verify", "HEAD").decode("ascii")
    source_dirty = bool(_git("status", "--porcelain", "--untracked-files=normal"))
    if require_clean and source_dirty:
        raise ValueError("Windows qualification requires a clean checkout")
    run_dir.mkdir(parents=True, exist_ok=False)
    report_path, export, rehearsal = _rehearse(manifest, run_dir)
    copied = report_path.parent / "copied-player"
    shutil.copytree(export, copied, symlinks=True)
    verify_copy(report_path, copied)

    workdir = run_dir / "independent-working-directory"
    workdir.mkdir()
    player = copied / "game.exe"
    if not stat.S_ISREG(ordinary_mode(player)):
        raise ValueError("Release package is missing its Windows Player")
    default, arrows = SMOKE_LABELS
    smoke(player, workdir, run_dir / f"{default}.log")
    smoke(player, workdir, run_dir / f"{arrows}.log", "--input-profile", str(PROFILE))
    verify_copy(report_path, copied)
    if digest(manifest) != rehearsal["authored_manifest_sha256"]:
        raise ValueError("Authored manifest changed during qualification")
    if _git("rev-parse", "--verify", "HEAD").decode("ascii") != revision:
        raise ValueError("Git revision changed during qualification")
    source_dirty |= bool(_git("status", "--porcelain", "--untracked-files=normal"))
    if require_clean and source_dirty:
        raise ValueError("The checkout changed during Windows qualification")

    files = rehearsal["package_files"]
    partial = run_dir / "candidate.partial.zip"
    with ZipFile(partial, "x", compression=ZIP_DEFLATED) as archive:
        for name in files:
            archive.write(copied / name, arcname=name)
    receipt = {
        "schema_version": 1,
        "git_revision": revision,
        "source_dirty": source_dirty,
        "authored_manifest_sha256": rehearsal["authored_manifest_sha256"],
        "archive_sha256": digest(partial),
        "package_files": files,
        "smoke": {label: "passed" for label in SMOKE_LABELS},
    }
    verify_candidate(partial, receipt)
    partial_receipt = run_dir / "qualification.partial.json"
    with partial_receipt.open("x", encoding="utf-8") as output:
        json.dump(receipt, output, indent=2)
        output.write("\n")
    partial.rename(run_dir / "candidate.zip")
    partial_receipt.rename(run_dir / "qualification.json")
    return run_dir


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path)
    parser.add_argument("run_dir", type=Path, help="a fresh directory for this candidate")
    parser.add_argument("--require-clean", action="store_true",
                        help="reject a dirty source tree before or after export")
    args = parser.parse_args(argv)
    if os.name != "nt":
        parser.exit(1, "Windows release qualification requires a native Windows session.\n")
    try:
        result = qualify(args.manifest, args.run_dir, require_clean=args.require_clean)
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError, KeyError, BadZipFile) as error:
        parser.exit(1, f"Qualification failed. {error}\n")
    print(f"Qualified candidate and receipt at {result}")


if __name__ == "__main__":
    main()
