"""Probe a verified packaged Windows Player from an independent directory."""

import argparse
from hashlib import file_digest
import json
import os
from pathlib import Path
import platform
import shutil
import stat
import subprocess

from zipfile import BadZipFile

from player_smoke import smoke
from rehearse_release import digest, ordinary_mode
from verify_candidate import MAX_PACKAGE_BYTES, read_receipt, verified_package
from verify_host_gates import gate_result, validate_profile


def _host_details():
    command = (
        "[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false); "
        "Get-CimInstance Win32_VideoController -ErrorAction Stop | "
        "Select-Object -Property Name, DriverVersion | ConvertTo-Json -Compress"
    )
    output = subprocess.run(
        ["powershell.exe", "-NoProfile", "-NonInteractive", "-Command", command],
        capture_output=True, check=True, timeout=20,
    ).stdout
    if len(output) > 16_384:
        raise ValueError("Windows GPU metadata exceeds its size limit")
    devices = json.loads(output.decode("utf-8"))
    if isinstance(devices, dict):
        devices = [devices]
    if not devices or not isinstance(devices, list) or len(devices) > 16:
        raise ValueError("No bounded GPU inventory was reported by Windows")
    if any(
        not isinstance(device, dict)
        or not isinstance(device.get("Name"), str)
        or not isinstance(device.get("DriverVersion"), (str, type(None)))
        for device in devices
    ):
        raise ValueError("Invalid GPU inventory reported by Windows")
    return {"windows": platform.version(), "graphics": devices}


def _extract_verified(archive, receipt, destination):
    with verified_package(archive, receipt) as package:
        destination.mkdir()
        for member in package.infolist():
            path = destination.joinpath(*member.filename.split("/"))
            path.parent.mkdir(parents=True, exist_ok=True)
            with package.open(member) as source, path.open("xb") as output:
                shutil.copyfileobj(source, output, 64 * 1024)


def _verify_tree(root, files):
    if not stat.S_ISDIR(ordinary_mode(root)):
        raise ValueError("Copied package root must be an ordinary directory")
    expected_folders = {
        parent.as_posix()
        for name in files
        for parent in Path(name).parents
        if parent != Path(".")
    }
    seen = set()
    folders = [root]
    total = 0
    while folders:
        folder = folders.pop()
        for item in folder.iterdir():
            name = item.relative_to(root).as_posix()
            mode = ordinary_mode(item)
            if stat.S_ISDIR(mode) and name in expected_folders:
                folders.append(item)
                continue
            if not stat.S_ISREG(mode) or name not in files:
                raise ValueError(f"Unexpected package entry {name}")
            with item.open("rb") as payload:
                total += os.fstat(payload.fileno()).st_size
                if total > MAX_PACKAGE_BYTES or file_digest(payload, "sha256").hexdigest() != files[name]:
                    raise ValueError(f"Copied package differs at {name}")
            seen.add(name)
    if seen != files.keys():
        raise ValueError("Copied package is missing expected files")


def probe(receipt_path, archive_path, output, host_profile, *, environment=_host_details, runner=smoke):
    validate_profile(host_profile)
    for source in (receipt_path, archive_path):
        if not stat.S_ISREG(ordinary_mode(source)):
            raise ValueError("Candidate input must be an ordinary file")
    receipt = read_receipt(receipt_path)
    output = output.absolute()
    output.parent.mkdir(parents=True, exist_ok=True)
    output.mkdir()
    package = output / "delivered-player"
    _extract_verified(archive_path, receipt, package)
    _verify_tree(package, receipt["package_files"])
    workdir = output / "independent-working-directory"
    profile = output / "disposable-profile"
    workdir.mkdir()
    profile.mkdir()
    log = output / "clean-player.log"
    details = output / "windows-host.json"
    status = "passed"
    try:
        host = environment()
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        status = "failed"
        host = {"windows": platform.version(), "graphics": [], "error": str(error)}
        log.write_text(f"Could not collect Windows host metadata. {error}\n", encoding="utf-8")
    with details.open("x", encoding="utf-8") as stream:
        json.dump(host, stream, indent=2)
        stream.write("\n")
    if status == "passed":
        try:
            runner(package / "game.exe", workdir, log, profile_home=profile)
        except (OSError, RuntimeError, subprocess.SubprocessError) as error:
            status = "failed"
            with log.open("a", encoding="utf-8") as stream:
                stream.write(f"\nPackaged Player smoke failed. {error}\n")
    try:
        _verify_tree(package, receipt["package_files"])
    except (OSError, ValueError) as error:
        status = "failed"
        with log.open("a", encoding="utf-8") as stream:
            stream.write(f"\nPackage verification after shutdown failed. {error}\n")
    evidence = {file.name: digest(file) for file in (log, details)}
    result = gate_result(receipt, "clean_player", host_profile, status, evidence)
    with (output / "clean-player.gate.json").open("x", encoding="utf-8") as stream:
        json.dump(result, stream, indent=2)
        stream.write("\n")
    return result


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("receipt", type=Path)
    parser.add_argument("archive", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--host-profile", required=True)
    args = parser.parse_args(argv)
    if os.name != "nt":
        parser.exit(1, "The installed Player probe requires a native Windows session.\n")
    try:
        result = probe(args.receipt, args.archive, args.output, args.host_profile)
    except (OSError, ValueError, BadZipFile, subprocess.SubprocessError) as error:
        parser.exit(1, f"Installed candidate probe failed. {error}\n")
    print(f"Probe {result['status']}. Evidence at {args.output}")
    return 0 if result["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
