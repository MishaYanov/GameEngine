"""Stage a project twice, export it once, and record the exact package."""

import argparse
from hashlib import sha256
import json
import os
from pathlib import Path
import stat
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[1]


def digest(path):
    fingerprint = sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(64 * 1024), b""):
            fingerprint.update(block)
    return fingerprint.hexdigest()


def ordinary_mode(path):
    metadata = path.lstat()
    attributes = getattr(metadata, "st_file_attributes", 0)
    if stat.S_ISLNK(metadata.st_mode) or attributes & stat.FILE_ATTRIBUTE_REPARSE_POINT:
        raise ValueError(f"Unexpected link or reparse point: {path}")
    return metadata.st_mode


def inventory(package):
    files = {}
    pending = [package]
    while pending:
        folder = pending.pop()
        for path in folder.iterdir():
            mode = ordinary_mode(path)
            if stat.S_ISDIR(mode):
                pending.append(path)
            elif stat.S_ISREG(mode):
                files[path.relative_to(package).as_posix()] = digest(path)
            else:
                raise ValueError(f"Unexpected package entry: {path}")
    return dict(sorted(files.items()))


def compiler(run, label, operation, manifest, parent):
    command = [
        "cargo", "run", "-p", "compiler", "--offline", "--locked", "--",
        operation, str(manifest), str(parent),
    ]
    log = run / f"{label}.log"
    with log.open("xb") as output:
        result = subprocess.run(
            command, cwd=ROOT, stdout=output, stderr=subprocess.STDOUT,
            env={**os.environ, "CARGO_NET_OFFLINE": "true"},
            check=False,
        )
    return result.returncode, log


def require(condition, message):
    if not condition:
        raise ValueError(message)


def verify_copy(report_path, package):
    report = json.loads(report_path.read_text(encoding="utf-8"))
    evidence = report_path.parent.resolve(strict=True)
    require(
        stat.S_ISDIR(ordinary_mode(package)) and package.resolve(strict=True).is_relative_to(evidence),
        "Package must be an ordinary directory within this rehearsal",
    )
    require(inventory(package) == report["package_files"], "Package differs from the recorded inventory")
    print(f"Verified package copy {package}")


def rehearse(manifest, evidence):
    evidence.mkdir(parents=True, exist_ok=True)
    run = Path(tempfile.mkdtemp(prefix="courtyard-", dir=evidence))
    print(f"Evidence directory {run}", flush=True)
    original_manifest = digest(manifest)
    paths = {}
    for label, operation in (
        ("stage-a", "--stage"), ("stage-b", "--stage"), ("release", "--export")
    ):
        parent = run / label
        parent.mkdir()
        code, log = compiler(run, label, operation, manifest, parent)
        require(code == 0, f"{label} failed. See {log}")
        packages = list(parent.iterdir())
        require(len(packages) == 1, f"{label} did not produce one package under {parent}")
        package = packages[0]
        require(stat.S_ISDIR(ordinary_mode(package)), f"Expected a package directory: {package}")
        paths[label] = package

    player_name = "game.exe" if os.name == "nt" else "game"
    first = inventory(paths["stage-a"])
    second = inventory(paths["stage-b"])
    release = inventory(paths["release"])
    data_files = [name for name in first if name == "project.json" or name.startswith("assets/")]
    require(first == second, "Independent stages differ")
    require("project.json" in first and len(data_files) > 1, "Stage is missing project data")
    require(player_name not in first, "Data-only stage contains a player")
    require(player_name in release, "Release player was never published")
    require(
        first == {name: value for name, value in release.items() if name != player_name},
        "Release contents differ from the data-only stage",
    )

    code, log = compiler(run, "occupied-stage", "--stage", manifest, paths["stage-a"].parent)
    require(
        code != 0 and "already exists" in log.read_text(encoding="utf-8", errors="replace"),
        f"Expected occupied-destination rejection. See {log}",
    )
    require(inventory(paths["stage-a"]) == first, "Occupied stage was changed")
    require(digest(manifest) == original_manifest, "Authored manifest changed during rehearsal")

    report = {
        "authored_manifest": str(manifest),
        "authored_manifest_sha256": original_manifest,
        "export_directory": str(paths["release"]),
        "data_file_count": len(data_files),
        "occupied_stage_rejected": True,
        "package_files": release,
    }
    with (run / "export-report.json").open("x", encoding="utf-8") as stream:
        json.dump(report, stream, indent=2)
        stream.write("\n")
    print(f"Release ready for smoke at {paths['release']}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--verify-copy", action="store_true",
                        help="compare a copied package to a trusted local export report")
    parser.add_argument("input", type=Path, help="authored manifest, or export report with --verify-copy")
    parser.add_argument("output", type=Path, help="evidence root, or copied package with --verify-copy")
    options = parser.parse_args()
    try:
        if options.verify_copy:
            verify_copy(options.input, options.output)
            return
        manifest = options.input.resolve(strict=True)
        require(manifest.is_file(), f"Manifest is not a regular file: {manifest}")
        rehearse(manifest, options.output.resolve())
    except (OSError, ValueError, KeyError) as error:
        parser.exit(status=1, message=f"{error}\n")


if __name__ == "__main__":
    main()
