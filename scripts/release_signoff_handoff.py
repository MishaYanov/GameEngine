"""Publish, verify, compare or summarize relocatable signed Windows release reviews."""

import argparse
from copy import copy
from dataclasses import dataclass
from datetime import datetime, timezone
from hashlib import sha256
from itertools import islice
import os
from pathlib import Path
import stat
import subprocess
from zipfile import BadZipFile

from collect_playtest_evidence import EVIDENCE, MAX_EVIDENCE_BYTES as PLAYTEST_BYTES
from collect_playtest_evidence import MAX_TRACE_BYTES, REPORT, TRACE
from compare_playtrace import MAX_REPORT_BYTES, read_local_json
from rehearse_release import ordinary_mode
from review_candidate import MAX_DECISION_BYTES, MAX_PACKET_BYTES, _json_bytes, _write_new
from sign_release_review import MAX_SIGNATURE_BYTES
from verify_candidate import MAX_PACKAGE_BYTES, MAX_RECEIPT_BYTES, _path_parts, digest_exact
from verify_host_gates import GATES, MAX_EVIDENCE_BYTES, MAX_RESULT_BYTES
from verify_release_review_signature import PRINCIPAL
from verify_release_signoffs import EXIT, MAX_LEDGER_BYTES, ROLES, require_independent_trust, review_ledger

MAX_ARTIFACTS = 64
MAX_INDEX_BYTES = 96 * 1024
MAX_COMPARISON_BYTES = 320 * 1024
MAX_SUMMARY_BYTES = 96 * 1024
INDEX = "handoff.json"


def identity(info):
    # Windows path and descriptor stats can expose different ctime fields
    # for the same file. Use the stable file ID, length and modification time.
    return info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns, info.st_nlink


def handoff_root(root):
    if not stat.S_ISDIR(ordinary_mode(root)):
        raise ValueError("Handoff root must be an ordinary directory")
    return root.resolve(strict=True)


class Inventory:
    def __init__(self, root):
        self.root = handoff_root(root)
        self.rows = {}
        self.identities = {}
        self.spelling = {}

    def path(self, name):
        parts = _path_parts(name)
        if len(parts) > 9 or len(name.encode("utf-8")) > 1024:
            raise ValueError("Handoff path exceeds its depth or byte limit")
        parent = self.root
        for index, part in enumerate(parts, 1):
            prefix = "/".join(parts[:index])
            if self.spelling.setdefault(prefix.casefold(), prefix) != prefix:
                raise ValueError("Ambiguous Windows path in handoff")
            parent /= part
            if index < len(parts) and not stat.S_ISDIR(ordinary_mode(parent)):
                raise ValueError("Handoff parent must be an ordinary directory")
        return parent

    def add(self, name, limit):
        path = self.path(name)
        if name in self.rows:
            if self.rows[name]["size"] > limit:
                raise ValueError(f"Handoff file violates its byte limit: {name}")
            return path
        if len(self.rows) == MAX_ARTIFACTS:
            raise ValueError("Handoff has more than 64 artifacts")
        initial = self.file_info(path, limit)
        flags = os.O_RDONLY | getattr(os, "O_BINARY", 0)
        flags |= getattr(os, "O_NOFOLLOW", 0) | getattr(os, "O_NONBLOCK", 0)
        with os.fdopen(os.open(path, flags), "rb") as stream:
            opened = os.fstat(stream.fileno())
            if not stat.S_ISREG(opened.st_mode) or identity(opened) != identity(initial):
                raise ValueError(f"Handoff file changed before hashing: {name}")
            digest = digest_exact(stream, opened.st_size)
            if identity(os.fstat(stream.fileno())) != identity(opened):
                raise ValueError(f"Handoff file changed while hashing: {name}")
        self.remember(name, opened, digest)
        return path

    def remember(self, name, info, digest):
        if len(self.rows) == MAX_ARTIFACTS:
            raise ValueError("Handoff has more than 64 artifacts")
        self.rows[name] = {"path": name, "size": info.st_size, "sha256": digest}
        self.identities[name] = identity(info)

    @staticmethod
    def file_info(path, limit):
        info = path.lstat()
        attributes = getattr(info, "st_file_attributes", 0)
        if (not stat.S_ISREG(info.st_mode) or info.st_nlink != 1
                or attributes & stat.FILE_ATTRIBUTE_REPARSE_POINT):
            raise ValueError(f"Handoff file must be ordinary and unlinked: {path.name}")
        if not 0 < info.st_size <= limit:
            raise ValueError(f"Handoff file violates its byte limit: {path.name}")
        return info

    def json(self, name, limit):
        path = self.add(name, limit)
        value, payload = read_local_json(path, limit)
        self.require_digest(name, sha256(payload).hexdigest())
        return value, payload

    def require_digest(self, name, digest):
        if name not in self.rows or self.rows[name]["sha256"] != digest:
            raise ValueError(f"Verified evidence changed or is absent: {name}")

    def finish(self):
        for name, row in self.rows.items():
            if identity(self.file_info(self.path(name), row["size"])) != self.identities[name]:
                raise ValueError(f"Handoff file changed during verification: {name}")
        return sorted(self.rows.values(), key=lambda row: row["path"].encode("utf-8"))


def gate_files(inventory, evidence):
    if not stat.S_ISDIR(ordinary_mode(evidence)):
        raise ValueError("Gate evidence must be an ordinary directory")
    with os.scandir(evidence) as items:
        names = [item.name for item in islice(items, MAX_ARTIFACTS)]
    if len(names) == MAX_ARTIFACTS:
        raise ValueError("Gate evidence directory exceeds its entry limit")
    result = {}
    for name in sorted(names):
        if not name.endswith(".gate.json"):
            continue
        review = "evidence/" + name
        record, _ = inventory.json(review, MAX_RESULT_BYTES)
        if not isinstance(record, dict) or not isinstance(record.get("gate"), str):
            raise ValueError("Gate record needs a known gate ID")
        gate = record["gate"]
        files = record.get("evidence")
        if gate not in GATES or gate in result or not isinstance(files, dict) or len(files) > 8:
            raise ValueError("Invalid gate inventory in handoff")
        for relative in files:
            inventory.add("evidence/" + relative, MAX_EVIDENCE_BYTES)
        result[gate] = review
    return result


def role_inputs(inventory, ledger):
    rows = ledger.get("roles") if isinstance(ledger, dict) else None
    if not isinstance(rows, list) or len(rows) != len(ROLES):
        raise ValueError("Ledger needs release and QA rows")
    inputs = []
    signoffs = inventory.path("signoffs")
    has_signoffs = signoffs.exists() or signoffs.is_symlink()
    if has_signoffs and not stat.S_ISDIR(ordinary_mode(signoffs)):
        raise ValueError("Signoff evidence needs an ordinary directory")
    for role, row in zip(ROLES, rows):
        if not isinstance(row, dict) or row.get("role") != role:
            raise ValueError("Ledger role order does not match its schema")
        if row.get("status") == "missing":
            if has_signoffs:
                for suffix in ("json", "sig"):
                    path = inventory.path(f"signoffs/{role}-decision.{suffix}")
                    if path.exists() or path.is_symlink():
                        raise ValueError(f"Ledger omits retained {role} evidence")
            continue
        principal = row.get("principal")
        if not isinstance(principal, str) or PRINCIPAL.fullmatch(principal) is None:
            raise ValueError("Ledger row needs an explicit principal")
        decision = inventory.add(f"signoffs/{role}-decision.json", MAX_DECISION_BYTES)
        signature = inventory.add(f"signoffs/{role}-decision.sig", MAX_SIGNATURE_BYTES)
        inputs.append((role, principal, decision, signature))
    return inputs


@dataclass(frozen=True)
class _HandoffReview:
    index: dict
    packet: dict
    ledger: dict


def _review_handoff(args, *, now=None):
    args = copy(args)
    inventory = Inventory(args.root)
    require_independent_trust(args, (inventory.root,))
    args.receipt = inventory.add("qualification.json", MAX_RECEIPT_BYTES)
    args.archive = inventory.path("candidate.zip")
    archive_info = inventory.file_info(args.archive, MAX_PACKAGE_BYTES)
    args.packet = inventory.add("packet.json", MAX_PACKET_BYTES)
    stored, stored_bytes = inventory.json("ledger.json", MAX_LEDGER_BYTES)
    args.evidence_dir = inventory.path("evidence")
    gate_records = gate_files(inventory, args.evidence_dir)
    args.signoff = role_inputs(inventory, stored)
    playtest = inventory.path("playtest")
    if playtest.exists() or playtest.is_symlink():
        attachment, _ = inventory.json("playtest/" + EVIDENCE, PLAYTEST_BYTES)
        inventory.add("playtest/" + TRACE, MAX_TRACE_BYTES)
        inventory.add("playtest/" + REPORT, MAX_REPORT_BYTES)
        args.playtest = playtest
    else:
        attachment = None
        args.playtest = None
    ledger, packet, verified_archive_info = review_ledger(args, now=now)
    if _json_bytes(ledger) != stored_bytes:
        if any(row["status"] == "expired" for row in ledger["roles"]):
            raise ValueError("Handoff ledger differs from the current verified evidence. "
                             "Use the renewal checklist and publish a renewed root")
        raise ValueError("Handoff ledger differs from the current verified evidence")
    if identity(archive_info) != identity(verified_archive_info):
        raise ValueError("Handoff archive changed while checking the ledger")
    inventory.remember("candidate.zip", verified_archive_info, ledger["archive_sha256"])
    inventory.require_digest("qualification.json", packet["receipt_sha256"])
    inventory.require_digest("packet.json", ledger["packet_sha256"])
    for gate in packet["gates"]:
        record = gate_records.get(gate["id"])
        if record is not None or gate["record_sha256"] is not None:
            inventory.require_digest(record, gate["record_sha256"])
        for relative, digest in gate["evidence"].items():
            inventory.require_digest("evidence/" + relative, digest)
    for role in ledger["roles"]:
        if role["status"] != "missing":
            name = f"signoffs/{role['role']}-decision"
            inventory.require_digest(name + ".json", role["decision_sha256"])
            inventory.require_digest(name + ".sig", role["signature_sha256"])
    if attachment is not None:
        inventory.require_digest("playtest/" + EVIDENCE, packet["playtest_evidence_sha256"])
        for name in (TRACE, REPORT):
            inventory.require_digest("playtest/" + name, attachment["files"][name])
    index = {
        "schema_version": 1, "scope": ledger["scope"], "status": ledger["status"],
        "archive_sha256": ledger["archive_sha256"], "packet_sha256": ledger["packet_sha256"],
        "files": inventory.finish(),
    }
    return _HandoffReview(index, packet, ledger)


def build_index(args, *, now=None):
    return _review_handoff(args, now=now).index


def _verify_handoff(args, *, now=None):
    target = handoff_root(args.root) / INDEX
    Inventory.file_info(target, MAX_INDEX_BYTES)
    _, stored = read_local_json(target, MAX_INDEX_BYTES)
    review = _review_handoff(args, now=now)
    if _json_bytes(review.index) != stored:
        raise ValueError("Handoff index differs from the current verified artifacts")
    return review


def verify_index(args, *, now=None):
    return _verify_handoff(args, now=now).index


def summarize_handoff(args):
    output = args.output
    if not stat.S_ISDIR(ordinary_mode(output.parent)):
        raise ValueError("Summary output parent must be an ordinary directory")
    output = output.parent.resolve(strict=True) / output.name
    if output.is_relative_to(handoff_root(args.root)):
        raise ValueError("Write the summary outside the handoff")
    if output.exists() or output.is_symlink():
        raise FileExistsError("Summary output already exists")
    review = _verify_handoff(args)
    files = {row["path"]: row for row in review.index["files"]}
    gates = [
        {**gate, "evidence": [files["evidence/" + path] for path in sorted(gate["evidence"])]}
        for gate in review.packet["gates"]
    ]
    report = {
        "schema_version": 1,
        "scope": review.ledger["scope"],
        "status": review.ledger["status"],
        "gate_verdict": review.ledger["gate_verdict"],
        "archive_sha256": review.ledger["archive_sha256"],
        "packet_sha256": review.ledger["packet_sha256"],
        "receipt_sha256": review.packet["receipt_sha256"],
        "git_revision": review.packet["git_revision"],
        "source_dirty": review.packet["source_dirty"],
        "gates": gates,
        "roles": review.ledger["roles"],
    }
    _write_new(output, _json_bytes(report), MAX_SUMMARY_BYTES)
    return report


def compare_indexes(prior, current):
    old = {row["path"]: row for row in prior["files"]}
    new = {row["path"]: row for row in current["files"]}
    paths = sorted(old.keys() | new.keys(), key=lambda path: path.encode("utf-8"))
    keys = ("scope", "status", "archive_sha256", "packet_sha256")
    return {
        "schema_version": 1,
        "prior": {key: prior[key] for key in keys},
        "current": {key: current[key] for key in keys},
        "added": [new[path] for path in paths if path not in old],
        "removed": [old[path] for path in paths if path not in new],
        "changed": [{"path": path, "prior": old[path], "current": new[path]}
                    for path in paths if path in old and path in new and old[path] != new[path]],
    }


def compare_handoffs(args):
    output = args.output
    if not stat.S_ISDIR(ordinary_mode(output.parent)):
        raise ValueError("Comparison output parent must be an ordinary directory")
    output = output.parent.resolve(strict=True) / output.name
    roots = (handoff_root(args.prior_root), handoff_root(args.current_root))
    require_independent_trust(args, roots)
    if any(output.is_relative_to(root) for root in roots):
        raise ValueError("Write the comparison outside both handoffs")
    if output.exists() or output.is_symlink():
        raise FileExistsError("Comparison output already exists")
    now = datetime.now(timezone.utc)
    indexes = []
    for root in roots:
        verified = copy(args)
        verified.root = root
        indexes.append(verify_index(verified, now=now))
    report = compare_indexes(*indexes)
    _write_new(output, _json_bytes(report), MAX_COMPARISON_BYTES)
    return report


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    publish = commands.add_parser("publish")
    verify = commands.add_parser("verify")
    compare = commands.add_parser("compare")
    summarize = commands.add_parser("summarize")
    for command in (publish, verify, summarize):
        command.add_argument("root", type=Path)
    summarize.add_argument("output", type=Path)
    for name in ("prior_root", "current_root", "output"):
        compare.add_argument(name, type=Path)
    for command in (publish, verify, compare, summarize):
        command.add_argument("--profiles", type=Path, required=True)
        command.add_argument("--role-policy", type=Path, required=True)
        command.add_argument("--allowed-signers", type=Path, required=True)
        command.add_argument("--revoked-signers", type=Path)
        command.add_argument("--ssh-keygen", type=Path)
    args = parser.parse_args(argv)
    try:
        if args.command == "compare":
            report = compare_handoffs(args)
            print(f"Compared verified release handoffs ({report['prior']['status']} to {report['current']['status']})")
            return 0
        if args.command == "summarize":
            report = summarize_handoff(args)
            print(f"Summarized verified release handoff {report['status']}")
            return EXIT[report["status"]]
        if args.command == "publish":
            target = handoff_root(args.root) / INDEX
            if target.exists() or target.is_symlink():
                raise FileExistsError("Handoff index already exists")
            index = build_index(args)
            _write_new(target, _json_bytes(index), MAX_INDEX_BYTES)
        else:
            index = verify_index(args)
    except (OSError, ValueError, BadZipFile, subprocess.SubprocessError) as error:
        parser.exit(3, f"Release handoff could not be verified. {error}\n")
    print(f"Verified release handoff {index['status']}")
    return EXIT[index["status"]]


if __name__ == "__main__":
    raise SystemExit(main())
