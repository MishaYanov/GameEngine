"""Prepare a local review packet, then bind a scoped decision to its verified inputs."""

import argparse
from datetime import datetime, timezone
from hashlib import sha256
import json
import os
from pathlib import Path
from zipfile import BadZipFile

from collect_playtest_evidence import verify_handoff
from compare_playtrace import read_local_json
from verify_candidate import MAX_RECEIPT_BYTES, _package_files, verify_candidate
from verify_host_gates import GATES, MAX_RESULT_BYTES, gate_verdict, inspect_gate_evidence


TARGET = "windows-player"
MAX_PACKET_BYTES = 64 * 1024
MAX_DECISION_BYTES = 4096
DECISIONS = {"accepted", "held", "rejected"}
DECISION_KEYS = {
    "schema_version", "packet_sha256", "archive_sha256", "scope", "status",
    "reviewer", "reason", "reviewed_at",
}


def _digest(data):
    return sha256(data).hexdigest()


def _json_bytes(value):
    return (json.dumps(value, ensure_ascii=False, sort_keys=True, indent=2) + "\n").encode("utf-8")


def _write_new(path, payload, limit):
    if not 0 < len(payload) <= limit:
        raise ValueError("Generated review file is outside its size limit")
    with path.open("xb") as output:
        output.write(payload)
        output.flush()
        os.fsync(output.fileno())


def build_packet(receipt, archive, evidence_dir, profiles, *, playtest=None):
    expected, receipt_data = read_local_json(receipt, MAX_RECEIPT_BYTES)
    workers, profile_data = read_local_json(profiles, MAX_RESULT_BYTES)
    package_files = _package_files(expected)
    records, gate_hashes = inspect_gate_evidence(expected, evidence_dir, workers)
    verify_candidate(archive, expected)
    verdict = gate_verdict(expected, records)
    receipt_sha = _digest(receipt_data)
    return {
        "schema_version": 1,
        "target": TARGET,
        "archive_sha256": expected["archive_sha256"],
        "git_revision": expected["git_revision"],
        "source_dirty": expected["source_dirty"],
        "receipt_sha256": receipt_sha,
        "host_profiles_sha256": _digest(profile_data),
        "gate_inventory_sha256": _digest(_json_bytes(gate_hashes)),
        "verdict": verdict["verdict"],
        "gates": [
            {
                "id": gate,
                "status": records.get(gate, {}).get("status", "not_run").replace("not_run", "pending"),
                "record_sha256": gate_hashes.get(gate),
                "profile": workers[gate],
                "evidence": records.get(gate, {}).get("evidence", {}),
            }
            for gate in sorted(GATES)
        ],
        "playtest_evidence_sha256": (
            verify_handoff(playtest, receipt_sha, package_files["project.json"])
            if playtest is not None else None
        ),
    }


def _verified_packet(path, current):
    _, payload = read_local_json(path, MAX_PACKET_BYTES)
    if payload != _json_bytes(current):
        raise ValueError("Review packet no longer matches the verified candidate and evidence")
    return _digest(payload)


def _text(value, max_bytes, field):
    if not isinstance(value, str) or not value.strip() or not value.isprintable():
        raise ValueError(f"{field} must be nonempty printable text")
    if len(value.encode("utf-8")) > max_bytes:
        raise ValueError(f"{field} exceeds {max_bytes} UTF-8 bytes")
    return value


def _validate_decision(record, packet, packet_hash):
    if (not isinstance(record, dict) or record.keys() != DECISION_KEYS
            or type(record["schema_version"]) is not int or record["schema_version"] != 1):
        raise ValueError("Unsupported decision schema")
    if record["scope"] != packet["target"] or record["scope"] != TARGET:
        raise ValueError("Decision scope does not match the reviewed package")
    if record["packet_sha256"] != packet_hash or record["archive_sha256"] != packet["archive_sha256"]:
        raise ValueError("Decision no longer matches its review packet or archive")
    status = record["status"]
    if not isinstance(status, str) or status not in DECISIONS:
        raise ValueError("Unknown decision status")
    if status == "accepted" and packet["verdict"] != "accepted":
        raise ValueError("Acceptance requires all mandatory passes on a clean candidate")
    _text(record["reviewer"], 96, "Reviewer display name")
    _text(record["reason"], 512, "Reason")
    timestamp = record["reviewed_at"]
    if not isinstance(timestamp, str) or len(timestamp) != 20:
        raise ValueError("Decision timestamp must use UTC to the second")
    when = datetime.strptime(timestamp, "%Y-%m-%dT%H:%M:%SZ")
    if when.strftime("%Y-%m-%dT%H:%M:%SZ") != timestamp:
        raise ValueError("Decision timestamp must use canonical UTC")


def decide(packet, packet_path, decision_path, *, scope, status, reviewer, reason):
    identity = _verified_packet(packet_path, packet)
    record = {
        "schema_version": 1,
        "packet_sha256": identity,
        "archive_sha256": packet["archive_sha256"],
        "scope": scope,
        "status": status,
        "reviewer": reviewer,
        "reason": reason,
        "reviewed_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    }
    _validate_decision(record, packet, identity)
    _write_new(decision_path, _json_bytes(record), MAX_DECISION_BYTES)
    return record


def verify_decision(packet, packet_path, decision_path):
    identity = _verified_packet(packet_path, packet)
    record, _ = read_local_json(decision_path, MAX_DECISION_BYTES)
    _validate_decision(record, packet, identity)
    return record


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    prepare = commands.add_parser("prepare")
    decision = commands.add_parser("decide")
    check = commands.add_parser("verify-decision")
    for command in (prepare, decision, check):
        command.add_argument("receipt", type=Path)
        command.add_argument("archive", type=Path)
        command.add_argument("evidence_dir", type=Path)
        command.add_argument("packet", type=Path)
        command.add_argument("--profiles", required=True, type=Path)
        command.add_argument("--playtest", type=Path)
    for command in (decision, check):
        command.add_argument("decision", type=Path)
    decision.add_argument("--scope", choices=[TARGET], required=True)
    decision.add_argument("--status", choices=sorted(DECISIONS), required=True)
    decision.add_argument("--reviewer", required=True)
    decision.add_argument("--reason", required=True)
    args = parser.parse_args(argv)
    try:
        packet = build_packet(args.receipt, args.archive, args.evidence_dir, args.profiles,
                              playtest=args.playtest)
        if args.command == "prepare":
            _write_new(args.packet, _json_bytes(packet), MAX_PACKET_BYTES)
            print(f"Verified packet: {packet['verdict']}")
        elif args.command == "decide":
            record = decide(packet, args.packet, args.decision, scope=args.scope,
                            status=args.status, reviewer=args.reviewer, reason=args.reason)
            print(f"Recorded local decision: {record['status']}")
        else:
            record = verify_decision(packet, args.packet, args.decision)
            print(f"Verified local decision: {record['status']}")
        return 0
    except (OSError, ValueError, BadZipFile) as error:
        parser.exit(1, f"Candidate review failed. {error}\n")


if __name__ == "__main__":
    raise SystemExit(main())
