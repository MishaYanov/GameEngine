"""Validate and combine trusted per-host Windows release gate evidence."""

import argparse
from hashlib import sha256
import json
import os
from pathlib import Path
import re
import stat
from zipfile import BadZipFile

from compare_playtrace import read_local_json
from rehearse_release import ordinary_mode
from verify_candidate import _is_hash, _path_parts, digest_exact, read_receipt, verify_candidate


GATES = {"headless", "editor", "clean_player", "manual_controller"}
STATUSES = {"passed", "failed", "not_run"}
PROFILE = re.compile(r"[a-z0-9][a-z0-9_-]{0,63}")
MAX_RESULT_BYTES = 16_384
MAX_EVIDENCE_BYTES = 64 * 1024 * 1024


def validate_profile(profile):
    if not isinstance(profile, str) or PROFILE.fullmatch(profile) is None:
        raise ValueError("Invalid host profile")
    return profile


def _ordinary_file(path):
    if not stat.S_ISREG(ordinary_mode(path)):
        raise ValueError(f"Expected an ordinary file at {path}")


def _bounded_json(path):
    _ordinary_file(path)
    if path.stat().st_size > MAX_RESULT_BYTES:
        raise ValueError(f"Gate JSON exceeds its size limit at {path}")
    return read_receipt(path)


def _evidence_digest(root, name):
    parts = _path_parts(name)
    if len(parts) > 8 or name.endswith(".gate.json"):
        raise ValueError("Invalid gate evidence path")
    path = root
    for directory in parts[:-1]:
        path /= directory
        if not stat.S_ISDIR(ordinary_mode(path)):
            raise ValueError(f"Invalid gate evidence directory {path}")
    path /= parts[-1]
    _ordinary_file(path)
    with path.open("rb") as source:
        metadata = os.fstat(source.fileno())
        if not stat.S_ISREG(metadata.st_mode) or not 0 < metadata.st_size <= MAX_EVIDENCE_BYTES:
            raise ValueError(f"Gate evidence is outside its size limit at {name}")
        return digest_exact(source, metadata.st_size)


def gate_result(receipt, gate, host_profile, status, evidence):
    validate_profile(host_profile)
    if gate not in GATES or status not in STATUSES:
        raise ValueError("Unsupported gate or status")
    if not isinstance(evidence, dict) or len(evidence) > 8 or (status != "not_run" and not evidence):
        raise ValueError("Gate result needs a bounded inventory of evidence")
    return {
        "schema_version": 1,
        "gate": gate,
        "host_profile": host_profile,
        "archive_sha256": receipt["archive_sha256"],
        "git_revision": receipt["git_revision"],
        "status": status,
        "evidence": evidence,
    }


def _validate_result(record, root, receipt, profiles):
    keys = {"schema_version", "gate", "host_profile", "archive_sha256", "git_revision", "status", "evidence"}
    if not isinstance(record, dict) or record.keys() != keys or type(record["schema_version"]) is not int:
        raise ValueError("Unsupported gate result schema")
    gate, status, profile = (record[name] for name in ("gate", "status", "host_profile"))
    if not isinstance(gate, str) or not isinstance(status, str):
        raise ValueError("Gate and status must be strings")
    expected = gate_result(receipt, gate, profile, status, record["evidence"])
    if record != expected:
        raise ValueError("Gate result belongs to another revision or candidate")
    if profile != profiles[gate]:
        raise ValueError(f"Unexpected host profile for {gate}")
    for name, digest in record["evidence"].items():
        if not _is_hash(digest) or _evidence_digest(root, name) != digest:
            raise ValueError(f"Gate evidence hash did not match at {name}")
    return gate


def _validate_profiles(profiles):
    if not isinstance(profiles, dict) or profiles.keys() != GATES:
        raise ValueError("Each required gate must have one assigned host profile")
    for profile in profiles.values():
        validate_profile(profile)
    if profiles["clean_player"] in {profiles["headless"], profiles["editor"]}:
        raise ValueError("The clean Player gate needs a separate host profile")


def inspect_gate_evidence(receipt, root, profiles):
    """Validate gate evidence and bind each gate to the exact verified record bytes."""
    _validate_profiles(profiles)
    records = {}
    digests = {}
    if not stat.S_ISDIR(ordinary_mode(root)):
        raise ValueError("Gate evidence root must be an ordinary directory")
    paths = []
    with os.scandir(root) as entries:
        for index, entry in enumerate(entries):
            if index == 63:
                raise ValueError("Too many entries in the release-gate evidence directory")
            if entry.name.endswith(".gate.json"):
                paths.append(Path(entry.path))
    for path in sorted(paths):
        record, source = read_local_json(path, MAX_RESULT_BYTES)
        gate = _validate_result(record, root, receipt, profiles)
        if gate in records:
            raise ValueError(f"Duplicate result for {gate}")
        records[gate] = record
        digests[gate] = sha256(source).hexdigest()
    return records, digests


def gate_verdict(receipt, records):
    failed = sorted(gate for gate, row in records.items() if row["status"] == "failed")
    pending = sorted(gate for gate in GATES if gate not in records or records[gate]["status"] == "not_run")
    verdict = "rejected" if failed else "pending" if pending or receipt["source_dirty"] else "accepted"
    return {
        "schema_version": 1, "verdict": verdict, "archive_sha256": receipt["archive_sha256"],
        "git_revision": receipt["git_revision"], "source_dirty": receipt["source_dirty"],
        "pending_gates": pending, "failed_gates": failed, "gates": records,
    }


def verify_gates(receipt_path, archive, root, profile_path):
    for path in (receipt_path, archive):
        _ordinary_file(path)
    receipt = read_receipt(receipt_path)
    profiles = _bounded_json(profile_path)
    records, _ = inspect_gate_evidence(receipt, root, profiles)
    verify_candidate(archive, receipt)
    return gate_verdict(receipt, records)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    verify = commands.add_parser("verify")
    record = commands.add_parser("record")
    for command in (verify, record):
        command.add_argument("receipt", type=Path)
        command.add_argument("archive", type=Path)
        command.add_argument("evidence_dir", type=Path)
    verify.add_argument("--profiles", required=True, type=Path)
    record.add_argument("--gate", required=True, choices=sorted(GATES))
    record.add_argument("--host-profile", required=True)
    record.add_argument("--status", required=True, choices=sorted(STATUSES))
    record.add_argument("--evidence", action="append", default=[])
    args = parser.parse_args(argv)
    try:
        if args.command == "verify":
            result = verify_gates(args.receipt, args.archive, args.evidence_dir, args.profiles)
            print(json.dumps(result, indent=2))
            return {"accepted": 0, "rejected": 1, "pending": 2}[result["verdict"]]
        for path in (args.receipt, args.archive):
            _ordinary_file(path)
        if not stat.S_ISDIR(ordinary_mode(args.evidence_dir)):
            raise ValueError("Gate evidence root must be an ordinary directory")
        receipt = read_receipt(args.receipt)
        verify_candidate(args.archive, receipt)
        if len(args.evidence) > 8 or len(set(args.evidence)) != len(args.evidence):
            raise ValueError("Provide at most eight unique evidence files")
        evidence = {name: _evidence_digest(args.evidence_dir, name) for name in args.evidence}
        result = gate_result(receipt, args.gate, args.host_profile, args.status, evidence)
        with (args.evidence_dir / f"{args.gate}.gate.json").open("x", encoding="utf-8") as output:
            json.dump(result, output, indent=2)
            output.write("\n")
        return 0
    except (OSError, ValueError, BadZipFile, KeyError) as error:
        parser.exit(3, f"Gate evidence rejected. {error}\n")


if __name__ == "__main__":
    raise SystemExit(main())
