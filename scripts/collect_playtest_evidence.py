"""Create or verify a local replay handoff for a trusted release candidate."""

import argparse
from hashlib import sha256
import json
from itertools import islice
from os import scandir
from pathlib import Path
import re
from zipfile import BadZipFile

from compare_playtrace import (
    MARKER_LABEL, MAX_MARKERS, MAX_REPORT_BYTES, marker_rows, read_local_json,
    valid_flag, valid_scene, valid_score, valid_tick,
)
from verify_candidate import MAX_RECEIPT_BYTES, SHA256, verify_candidate


TRACE = "trace.replay.json"
REPORT = "trace-report.json"
EVIDENCE = "evidence.json"
MAX_TRACE_BYTES = 8 * 1024 * 1024
MAX_EVIDENCE_BYTES = 4096
KEY_CODE = re.compile(r"[A-Za-z][A-Za-z0-9]{0,31}")


def _hash(data):
    return sha256(data).hexdigest()


def _json_bytes(value):
    return json.dumps(value, sort_keys=True, indent=2).encode("utf-8") + b"\n"


def _valid_sha(value):
    return isinstance(value, str) and SHA256.fullmatch(value) is not None


def _valid_description(value):
    return (
        isinstance(value, str) and 1 <= len(value) <= 120
        and value.isprintable() and value.strip() == value
    )


def _check_test_metadata(metadata):
    if not isinstance(metadata, dict) or set(metadata) - {"platform", "input_method"}:
        raise ValueError("Invalid playtest metadata fields")
    if not all(_valid_description(value) for value in metadata.values()):
        raise ValueError("Playtest metadata must be 1 through 120 printable characters")


def _trace_labels(trace):
    if not isinstance(trace, dict) or set(trace) != {"schema_version", "actions"}:
        raise ValueError("Expected a replay object containing only a version and actions")
    version, actions = trace["schema_version"], trace["actions"]
    if type(version) is not int or version not in (4, 5):
        raise ValueError("Playtest handoff requires replay version 4 or 5")
    if not isinstance(actions, list) or not 1 <= len(actions) <= 10_000:
        raise ValueError("Playtest trace must have 1 through 10000 actions")
    if actions[0] != {"type": "command", "command": {"type": "start_play"}}:
        raise ValueError("Playtest trace must start a fresh Play run")
    labels, seen = [], set()
    total_ticks = 0
    for action in actions:
        if not isinstance(action, dict):
            raise ValueError("Invalid replay action")
        kind = action.get("type")
        keys = set(action) - {"type"}
        if kind == "command":
            command = action.get("command")
            valid = (
                keys == {"command"} and isinstance(command, dict)
                and (
                    command in ({"type": "start_play"}, {"type": "stop_play"})
                    or set(command) == {"type", "paused"}
                    and command["type"] == "set_paused" and type(command["paused"]) is bool
                )
            )
        elif kind == "restart_run":
            valid = not keys
        elif kind == "keyboard_input":
            values = action.get("keys")
            valid = (
                keys == {"keys"} and isinstance(values, list) and len(values) <= 16
                and all(isinstance(key, str) and KEY_CODE.fullmatch(key) for key in values)
                and len(set(values)) == len(values)
            )
        elif kind == "controller_input":
            valid = keys == {"left_x", "left_y", "jump", "interact"} and all(
                type(action[key]) is int and -32768 <= action[key] <= 32767
                for key in ("left_x", "left_y")
            ) and all(type(action[key]) is bool for key in ("jump", "interact"))
        elif kind == "tick":
            count = action.get("count", 1)
            valid = (
                keys <= {"count"} and type(count) is int and 0 <= count <= 100_000
                and total_ticks + count <= 100_000
            )
            if valid:
                total_ticks += count
        elif kind == "mark":
            label = action.get("label")
            valid = (
                version == 5 and keys == {"label"} and isinstance(label, str)
                and MARKER_LABEL.fullmatch(label) and label not in seen
                and len(labels) < MAX_MARKERS
            )
            if valid:
                labels.append(label)
                seen.add(label)
        else:
            valid = False
        if not valid:
            raise ValueError("Trace includes an invalid or unsupported playtest action")
    return labels


def _normalize_report(report):
    if not isinstance(report, dict):
        raise ValueError("Expected a headless replay report")
    status = report.get("play_status")
    status = {} if status is None else status
    if not isinstance(status, dict):
        raise ValueError("Invalid headless play status")
    gameplay = status.get("gameplay")
    gameplay = {} if gameplay is None else gameplay
    if not isinstance(gameplay, dict):
        raise ValueError("Invalid headless gameplay status")
    return {
        "schema_version": report.get("schema_version"),
        "replay_identity": report.get("replay_identity"),
        "markers": marker_rows(report),
        "final": {
            "scene_id": report.get("play_scene_id", report.get("active_scene_id")),
            "simulation_tick": report.get("simulation_tick"),
            "completed": gameplay.get("completed"),
            "score": gameplay.get("score"),
        },
    }


def _check_report(report, labels, trace_digest, project_digest):
    if (
        not isinstance(report, dict)
        or set(report) != {"schema_version", "replay_identity", "markers", "final"}
        or type(report["schema_version"]) is not int or report["schema_version"] != 1
    ):
        raise ValueError("Unsupported headless report schema")
    if report["replay_identity"] != {
        "replay_sha256": trace_digest,
        "project_manifest_sha256": project_digest,
    }:
        raise ValueError("Replay report does not match the trace and candidate manifest")
    if [row["label"] for row in marker_rows(report)] != labels:
        raise ValueError("Replay report marker labels do not match the trace")
    final = report["final"]
    if (
        not isinstance(final, dict)
        or set(final) != {"scene_id", "simulation_tick", "completed", "score"}
        or not valid_scene(final["scene_id"])
        or not valid_tick(final["simulation_tick"])
        or not valid_flag(final["completed"])
        or not valid_score(final["score"])
    ):
        raise ValueError("Invalid final replay facts")


def _candidate_receipt(receipt_path):
    receipt, data = read_local_json(receipt_path, MAX_RECEIPT_BYTES)
    if not isinstance(receipt, dict) or not isinstance(receipt.get("package_files"), dict):
        raise ValueError("Invalid candidate receipt")
    project_digest = receipt["package_files"].get("project.json")
    if not _valid_sha(project_digest):
        raise ValueError("Candidate receipt lacks a valid project manifest digest")
    return receipt, _hash(data), project_digest


def collect(receipt, archive, trace, report, folder, *, metadata=None):
    metadata = {} if metadata is None else metadata
    _check_test_metadata(metadata)
    candidate, receipt_digest, project_digest = _candidate_receipt(receipt)
    replay, replay_bytes = read_local_json(trace, MAX_TRACE_BYTES)
    trace_digest = _hash(replay_bytes)
    normalized = _normalize_report(read_local_json(report, MAX_REPORT_BYTES)[0])
    _check_report(normalized, _trace_labels(replay), trace_digest, project_digest)
    verify_candidate(archive, candidate)
    report_bytes = _json_bytes(normalized)
    evidence = {
        "schema_version": 1,
        "candidate_receipt_sha256": receipt_digest,
        "files": {TRACE: trace_digest, REPORT: _hash(report_bytes)},
    }
    if metadata:
        evidence["test"] = metadata

    folder.mkdir()
    for filename, data in (
        (TRACE, replay_bytes), (REPORT, report_bytes), (EVIDENCE, _json_bytes(evidence)),
    ):
        with (folder / filename).open("xb") as output:
            output.write(data)


def verify(receipt, archive, folder):
    with scandir(folder) as entries:
        names = {entry.name for entry in islice(entries, 4)}
    if names != {TRACE, REPORT, EVIDENCE}:
        raise ValueError("Evidence folder must contain exactly the three handoff files")
    candidate, receipt_digest, project_digest = _candidate_receipt(receipt)
    evidence = read_local_json(folder / EVIDENCE, MAX_EVIDENCE_BYTES)[0]
    if (
        not isinstance(evidence, dict)
        or set(evidence) - {"schema_version", "candidate_receipt_sha256", "files", "test"}
        or type(evidence.get("schema_version")) is not int or evidence["schema_version"] != 1
    ):
        raise ValueError("Unsupported playtest evidence schema")
    if evidence.get("candidate_receipt_sha256") != receipt_digest:
        raise ValueError("Playtest evidence references a different candidate receipt")
    _check_test_metadata(evidence.get("test", {}))
    inventory = evidence.get("files")
    if (
        not isinstance(inventory, dict) or set(inventory) != {TRACE, REPORT}
        or not all(_valid_sha(value) for value in inventory.values())
    ):
        raise ValueError("Invalid playtest evidence inventory")
    replay, replay_bytes = read_local_json(folder / TRACE, MAX_TRACE_BYTES)
    report, report_bytes = read_local_json(folder / REPORT, MAX_REPORT_BYTES)
    trace_digest = _hash(replay_bytes)
    if trace_digest != inventory[TRACE] or _hash(report_bytes) != inventory[REPORT]:
        raise ValueError("Playtest evidence file hash does not match the inventory")
    _check_report(report, _trace_labels(replay), trace_digest, project_digest)
    verify_candidate(archive, candidate)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    subcommands = parser.add_subparsers(dest="command", required=True)
    collect_args = subcommands.add_parser("collect", help="create a fresh local handoff")
    check_args = subcommands.add_parser("verify", help="check a handoff and its candidate")
    for subcommand in (collect_args, check_args):
        subcommand.add_argument("receipt", type=Path)
        subcommand.add_argument("archive", type=Path)
    collect_args.add_argument("trace", type=Path)
    collect_args.add_argument("report", type=Path)
    collect_args.add_argument("folder", type=Path)
    collect_args.add_argument("--platform")
    collect_args.add_argument("--input-method")
    check_args.add_argument("folder", type=Path)
    args = parser.parse_args(argv)
    try:
        if args.command == "collect":
            metadata = {
                key: value for key, value in (
                    ("platform", args.platform), ("input_method", args.input_method)
                ) if value is not None
            }
            collect(args.receipt, args.archive, args.trace, args.report, args.folder, metadata=metadata)
        else:
            verify(args.receipt, args.archive, args.folder)
    except (OSError, ValueError, BadZipFile) as error:
        parser.exit(1, f"Playtest handoff failed. {error}\n")
    print(f"Playtest evidence {args.command} succeeded: {args.folder}")


if __name__ == "__main__":
    main()
