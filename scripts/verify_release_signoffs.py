"""Publish or revalidate a local ledger of independent release decisions."""

import argparse
from dataclasses import dataclass
from datetime import datetime, timezone
from hashlib import sha256
from pathlib import Path
import re
import stat
import subprocess
from zipfile import BadZipFile

from compare_playtrace import read_local_json
from rehearse_release import ordinary_mode
from review_candidate import (
    MAX_DECISION_BYTES, _json_bytes, _write_new, prepare_packet,
    validate_decision, verified_packet_digest,
)
from sign_release_review import MAX_SIGNATURE_BYTES, ssh_keygen
from verify_release_review_signature import (
    PRINCIPAL, SignerPolicy, read_ordinary, verify_detached_signature,
)

ROLES = ("release", "qa")
MAX_ROLE_POLICY_BYTES = 4096
MAX_LEDGER_BYTES = 8192
MAX_CHECKLIST_BYTES = 32 * 1024
MAX_AGE_SECONDS = 31 * 24 * 60 * 60
FINGERPRINT = re.compile(rb" SHA256:([A-Za-z0-9+/]{43})\s*\Z")
EXIT = {"accepted": 0, "rejected": 1, "held": 2}


@dataclass(frozen=True)
class VerifiedRole:
    principal: str
    decision: object
    decision_sha256: str
    signature_sha256: str
    signing_key_sha256: str


def role_policy(path):
    policy, payload = read_local_json(path, MAX_ROLE_POLICY_BYTES)
    if (not isinstance(policy, dict)
            or policy.keys() != {"schema_version", "max_decision_age_seconds", "roles"}
            or type(policy["schema_version"]) is not int or policy["schema_version"] != 1):
        raise ValueError("Expected release role policy schema 1")
    age = policy["max_decision_age_seconds"]
    if type(age) is not int or not 1 <= age <= MAX_AGE_SECONDS:
        raise ValueError("Decision age limit must be between one second and 31 days")
    roles = policy["roles"]
    if not isinstance(roles, dict) or roles.keys() != set(ROLES):
        raise ValueError("Policy must define the release and qa roles")
    all_principals = set()
    for role in ROLES:
        principals = roles[role]
        if not isinstance(principals, list) or not 1 <= len(principals) <= 8:
            raise ValueError(f"Role {role} needs one to eight principals")
        for principal in principals:
            if not isinstance(principal, str) or PRINCIPAL.fullmatch(principal) is None:
                raise ValueError(f"Invalid principal in role {role}")
            if principal in all_principals:
                raise ValueError("Each principal must be assigned to only one role")
            all_principals.add(principal)
    return roles, age, sha256(payload).hexdigest()


def signoff_inputs(entries, roles):
    selected = {}
    for role, principal, decision, signature in entries:
        if role not in roles or role in selected:
            raise ValueError("Unknown or repeated signoff role")
        if principal not in roles[role]:
            raise ValueError(f"Principal is outside the policy for role {role}")
        selected[role] = principal, Path(decision), Path(signature)
    return selected


def verify_roles(selected, policy, executable):
    verified_roles = {}
    fingerprints = set()
    for role, (principal, decision_path, signature_path) in selected.items():
        record, payload = read_local_json(decision_path, MAX_DECISION_BYTES)
        signature = read_ordinary(signature_path, MAX_SIGNATURE_BYTES)
        verified = verify_detached_signature(payload, signature, principal, policy, executable)
        key = FINGERPRINT.search(verified)
        if key is None:
            raise ValueError("OpenSSH did not report a verified signing-key fingerprint")
        fingerprint = key[1].decode("ascii")
        if fingerprint in fingerprints:
            raise ValueError("Required roles must use different signing keys")
        fingerprints.add(fingerprint)
        verified_roles[role] = VerifiedRole(
            principal, record, sha256(payload).hexdigest(),
            sha256(signature).hexdigest(), f"SHA256:{fingerprint}",
        )
    return verified_roles


def review_ledger(args, *, now=None):
    """Return the current ledger, its authenticated packet and verified ZIP metadata."""
    roles, max_age, role_digest = role_policy(args.role_policy)
    selected = signoff_inputs(args.signoff, roles)
    policy = SignerPolicy.load(args.allowed_signers, args.revoked_signers)
    verified_roles = verify_roles(selected, policy, ssh_keygen(args.ssh_keygen)) if selected else {}
    packet, archive_info = prepare_packet(
        args.receipt, args.archive, args.evidence_dir, args.profiles, playtest=args.playtest,
    )
    packet_digest = verified_packet_digest(args.packet, packet)
    now = now if now is not None else datetime.now(timezone.utc)
    rows = []
    for role in ROLES:
        verified = verified_roles.get(role)
        if verified is None:
            rows.append({"role": role, "status": "missing"})
            continue
        record = verified.decision
        validate_decision(record, packet, packet_digest)
        reviewed = datetime.strptime(record["reviewed_at"], "%Y-%m-%dT%H:%M:%SZ")
        age = (now - reviewed.replace(tzinfo=timezone.utc)).total_seconds()
        if age < 0:
            raise ValueError(f"Decision for role {role} is dated in the future")
        rows.append({
            "role": role,
            "status": "expired" if age > max_age else record["status"],
            "principal": verified.principal,
            "signing_key_sha256": verified.signing_key_sha256,
            "decision_status": record["status"],
            "reviewed_at": record["reviewed_at"],
            "decision_sha256": verified.decision_sha256,
            "signature_sha256": verified.signature_sha256,
        })

    gate = packet["verdict"]
    if gate == "rejected" or any(row["status"] == "rejected" for row in rows):
        status = "rejected"
    elif gate == "accepted" and all(row["status"] == "accepted" for row in rows):
        status = "accepted"
    else:
        status = "held"
    ledger = {
        "schema_version": 1,
        "scope": packet["target"],
        "status": status,
        "gate_verdict": gate,
        "archive_sha256": packet["archive_sha256"],
        "packet_sha256": packet_digest,
        "role_policy_sha256": role_digest,
        "allowed_signers_sha256": sha256(policy.allowed).hexdigest(),
        "revocations_sha256": sha256(policy.revoked).hexdigest() if policy.revoked is not None else None,
        "roles": rows,
    }
    return ledger, packet, archive_info


def build_ledger(args, *, now=None):
    return review_ledger(args, now=now)[0]


def require_independent_trust(args, roots):
    for trust in (args.profiles, args.role_policy, args.allowed_signers,
                  args.revoked_signers, args.ssh_keygen):
        if trust is not None and any(Path(trust).resolve().is_relative_to(root) for root in roots):
            raise ValueError("Use trusted policy files and verification tools outside the handoff and evidence")


def renewal_checklist(args, *, now=None):
    output = args.ledger
    if not stat.S_ISDIR(ordinary_mode(output.parent)):
        raise ValueError("Checklist output parent must be an ordinary directory")
    output = output.parent.resolve(strict=True) / output.name
    evidence_roots = tuple(root.resolve() for root in (args.evidence_dir, args.playtest) if root is not None)
    require_independent_trust(args, evidence_roots)
    if any(output.is_relative_to(root) for root in evidence_roots):
        raise ValueError("Write the checklist outside the evidence directories")
    if output.exists() or output.is_symlink():
        raise FileExistsError("Checklist output already exists")
    ledger, packet, _ = review_ledger(args, now=now)
    report = {
        "schema_version": 1,
        "scope": ledger["scope"],
        "status": ledger["status"],
        "gate_verdict": ledger["gate_verdict"],
        "archive_sha256": ledger["archive_sha256"],
        "packet_sha256": ledger["packet_sha256"],
        "receipt_sha256": packet["receipt_sha256"],
        "source_dirty": packet["source_dirty"],
        "outstanding_gates": [gate for gate in packet["gates"] if gate["status"] != "passed"],
        "roles": ledger["roles"],
        "roles_requiring_review": [
            row["role"] for row in ledger["roles"] if row["status"] in {"missing", "expired"}
        ],
    }
    _write_new(output, _json_bytes(report), MAX_CHECKLIST_BYTES)
    return report


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    for command in (commands.add_parser("publish"), commands.add_parser("verify"),
                    commands.add_parser("checklist")):
        for field in ("receipt", "archive", "evidence_dir", "packet", "ledger"):
            command.add_argument(field, type=Path)
        command.add_argument("--profiles", required=True, type=Path)
        command.add_argument("--role-policy", required=True, type=Path)
        command.add_argument("--allowed-signers", required=True, type=Path)
        command.add_argument("--revoked-signers", type=Path)
        command.add_argument("--ssh-keygen", type=Path)
        command.add_argument("--playtest", type=Path)
        command.add_argument("--signoff", action="append", nargs=4, default=[],
                             metavar=("ROLE", "PRINCIPAL", "DECISION", "SIGNATURE"))
    args = parser.parse_args(argv)
    try:
        if args.command == "checklist":
            report = renewal_checklist(args)
            print(f"Verified release renewal checklist {report['status']}")
            return EXIT[report["status"]]
        if args.command == "publish" and (args.ledger.exists() or args.ledger.is_symlink()):
            raise FileExistsError(f"Ledger output is already occupied {args.ledger.name}")
        stored = read_local_json(args.ledger, MAX_LEDGER_BYTES)[1] if args.command == "verify" else None
        ledger = build_ledger(args)
        payload = _json_bytes(ledger)
        if args.command == "publish":
            _write_new(args.ledger, payload, MAX_LEDGER_BYTES)
        elif stored != payload:
            raise ValueError("Ledger differs from the current verified evidence")
    except (OSError, ValueError, BadZipFile, subprocess.SubprocessError) as error:
        parser.exit(3, f"Release signoffs could not be verified. {error}\n")
    print(f"Verified release ledger {ledger['status']}")
    return EXIT[ledger["status"]]


if __name__ == "__main__":
    raise SystemExit(main())
