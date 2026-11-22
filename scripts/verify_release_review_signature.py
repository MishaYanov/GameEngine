"""Verify a reviewed release decision against an explicit OpenSSH signer policy."""

import argparse
from datetime import datetime, timezone
from hashlib import sha256
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import tempfile
from zipfile import BadZipFile

from compare_playtrace import read_local_json
from rehearse_release import ordinary_mode
from review_candidate import MAX_DECISION_BYTES, build_packet, validate_reviewed_decision
from sign_release_review import (
    MAX_SIGNATURE_BYTES, NAMESPACE, add_inputs, require_signature,
    run_ssh, ssh_keygen,
)

MAX_POLICY_BYTES = 256 * 1024
MAX_REVOCATIONS_BYTES = 1024 * 1024
PRINCIPAL = re.compile(r"[A-Za-z0-9][A-Za-z0-9_.@-]{0,95}")


def read_ordinary(path, limit):
    if not stat.S_ISREG(ordinary_mode(path)):
        raise ValueError("Signature inputs must be ordinary files")
    flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0) | getattr(os, "O_NONBLOCK", 0)
    flags |= getattr(os, "O_BINARY", 0)
    with os.fdopen(os.open(path, flags), "rb") as source:
        metadata = os.fstat(source.fileno())
        if not stat.S_ISREG(metadata.st_mode) or not 0 < metadata.st_size <= limit:
            raise ValueError("Signature input is outside its byte limit")
        data = source.read(limit + 1)
    if len(data) != metadata.st_size:
        raise ValueError("Signature input changed size while reading")
    return data


def verify(args):
    if PRINCIPAL.fullmatch(args.principal) is None:
        raise ValueError("Expected one explicit signer principal")
    signature = require_signature(read_ordinary(args.signature, MAX_SIGNATURE_BYTES))
    policy = read_ordinary(args.allowed_signers, MAX_POLICY_BYTES)
    revoked = (read_ordinary(args.revoked_signers, MAX_REVOCATIONS_BYTES)
               if args.revoked_signers is not None else None)
    record, payload = read_local_json(args.decision, MAX_DECISION_BYTES)
    # OpenSSH reads an immutable snapshot of the public policy and signature. The
    # checked decision bytes go directly to stdin without reopening the document.
    with tempfile.TemporaryDirectory(prefix="engine-review-") as directory:
        root = Path(directory)
        policy_file = root / "allowed-signers"
        signature_file = root / "decision.sig"
        policy_file.write_bytes(policy)
        signature_file.write_bytes(signature)
        command = ["-Y", "verify", "-f", str(policy_file), "-I", args.principal,
                   "-n", NAMESPACE, "-s", str(signature_file)]
        if revoked is not None:
            revocation_file = root / "revoked-signers"
            revocation_file.write_bytes(revoked)
            command.extend(("-r", str(revocation_file)))
        verified = run_ssh(ssh_keygen(args.ssh_keygen), command, payload)
    if verified.returncode != 0:
        raise ValueError("Signature was denied by the configured signer policy")
    packet = build_packet(args.receipt, args.archive, args.evidence_dir, args.profiles,
                          playtest=args.playtest)
    validate_reviewed_decision(packet, args.packet, record)
    return {
        "schema_version": 1,
        "scope": record["scope"],
        "status": record["status"],
        "archive_sha256": record["archive_sha256"],
        "principal": args.principal,
        "policy_sha256": sha256(policy).hexdigest(),
        "revocations_sha256": sha256(revoked).hexdigest() if revoked is not None else None,
        "verified_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    }


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    add_inputs(parser)
    parser.add_argument("--allowed-signers", type=Path, required=True)
    parser.add_argument("--principal", required=True)
    parser.add_argument("--revoked-signers", type=Path)
    args = parser.parse_args(argv)
    try:
        print(json.dumps(verify(args), sort_keys=True))
    except (OSError, ValueError, BadZipFile, subprocess.SubprocessError) as error:
        parser.exit(1, f"Release signature verification failed. {error}\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
