"""Verify local release consensus with disposable OpenSSH keys."""

import argparse
from contextlib import redirect_stdout
from datetime import datetime, timedelta, timezone
from io import StringIO
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from zipfile import ZipFile

import review_candidate as review
from sign_release_review import NAMESPACE, run_ssh, ssh_keygen
from test_release_qualification import FILES, fingerprint, receipt
from test_sign_release_review import has_system_ssh_keygen
from test_verify_host_gates import PROFILES
from verify_host_gates import gate_result
from verify_release_signoffs import build_ledger, main, role_policy


@unittest.skipUnless(has_system_ssh_keygen(), "requires system OpenSSH ssh-keygen")
class ReleaseSignoffTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = root = Path(temporary.name)
        self.args = args = argparse.Namespace(
            receipt=root / "receipt.json", archive=root / "candidate.zip",
            evidence_dir=root / "gates", profiles=root / "profiles.json",
            packet=root / "packet.json", playtest=None, role_policy=root / "roles.json",
            allowed_signers=root / "allowed", revoked_signers=None, ssh_keygen=None,
            signoff=[],
        )
        with ZipFile(args.archive, "x") as archive:
            for name, data in FILES.items():
                archive.writestr(name, data)
        self.receipt = receipt(args.archive)
        args.receipt.write_text(json.dumps(self.receipt), encoding="utf-8")
        args.profiles.write_text(json.dumps(PROFILES), encoding="utf-8")
        args.evidence_dir.mkdir()
        self.roles = {
            "schema_version": 1, "max_decision_age_seconds": 3600,
            "roles": {"release": ["r-owner"], "qa": ["qa-owner"]},
        }
        args.role_policy.write_text(json.dumps(self.roles), encoding="utf-8")
        self.executable = ssh_keygen()
        self.keys = {role: root / role for role in self.roles["roles"]}
        for key in self.keys.values():
            subprocess.run([self.executable, "-qt", "ed25519", "-N", "", "-f", str(key)],
                           capture_output=True, timeout=10, check=True)
        self.write_allowed(self.keys)

    def write_allowed(self, keys):
        policy = ""
        for role, key in keys.items():
            principal = self.roles["roles"][role][0]
            public = key.with_suffix(".pub").read_text(encoding="utf-8").strip()
            policy += f'{principal} namespaces="{NAMESPACE}" {public}\n'
        self.args.allowed_signers.write_text(policy, encoding="utf-8")

    def prepare(self, passed=True):
        args = self.args
        if passed:
            for gate, profile in PROFILES.items():
                data = b"Observed gate outcome"
                log = args.evidence_dir / f"{gate}.log"
                log.write_bytes(data)
                status = gate_result(self.receipt, gate, profile, "passed",
                                     {log.name: fingerprint(data)})
                (args.evidence_dir / f"{gate}.gate.json").write_text(json.dumps(status), encoding="utf-8")
        self.packet = review.build_packet(args.receipt, args.archive, args.evidence_dir, args.profiles)
        review._write_new(args.packet, review._json_bytes(self.packet), review.MAX_PACKET_BYTES)

    def decision(self, role, status, *, key=None, mutate=None):
        decision = self.root / f"{role}.json"
        signature = self.root / f"{role}.sig"
        value = review.decide(self.packet, self.args.packet, decision, scope="windows-player",
                              status=status, reviewer="Authorized test reviewer", reason="Test decision")
        if mutate is not None:
            mutate(value)
            decision.write_bytes(review._json_bytes(value))
        result = run_ssh(self.executable, ["-Y", "sign", "-f", str(key or self.keys[role]),
                                           "-n", NAMESPACE], decision.read_bytes())
        self.assertEqual(result.returncode, 0, result.stderr)
        signature.write_bytes(result.stdout)
        args = role, self.roles["roles"][role][0], decision, signature
        self.args.signoff.append(args)
        return value

    def run_cli(self, command, ledger):
        args = self.args
        arguments = [
            command, str(args.receipt), str(args.archive), str(args.evidence_dir),
            str(args.packet), str(ledger), "--profiles", str(args.profiles),
            "--role-policy", str(args.role_policy), "--allowed-signers", str(args.allowed_signers),
        ]
        for row in args.signoff:
            arguments.extend(("--signoff", *map(str, row)))
        with redirect_stdout(StringIO()):
            return main(arguments)

    def test_acceptance_needs_two_current_roles_and_publishes_once(self):
        self.prepare()
        self.decision("qa", "accepted")
        partial = build_ledger(self.args)
        self.assertEqual(partial["status"], "held")
        self.assertEqual(partial["roles"][0], {"role": "release", "status": "missing"})
        self.decision("release", "accepted")
        ledger = self.root / "ledger.json"
        self.assertEqual(self.run_cli("publish", ledger), 0)
        self.assertEqual(self.run_cli("verify", ledger), 0)
        original = ledger.read_bytes()
        with self.assertRaises(SystemExit) as conflict:
            self.run_cli("publish", ledger)
        self.assertEqual(conflict.exception.code, 3)
        self.assertEqual(original, ledger.read_bytes())
        self.args.allowed_signers.write_bytes(self.args.allowed_signers.read_bytes() + b"\n")
        with self.assertRaises(SystemExit) as changed:
            self.run_cli("verify", ledger)
        self.assertEqual(changed.exception.code, 3)

    def test_expired_approval_holds_and_current_rejection_dominates(self):
        self.prepare()
        self.decision("release", "accepted")
        expired = datetime.now(timezone.utc) + timedelta(hours=2)
        ledger = build_ledger(self.args, now=expired)
        self.assertEqual(ledger["status"], "held")
        self.assertEqual(ledger["roles"][0]["status"], "expired")
        self.decision("qa", "rejected")
        self.assertEqual(build_ledger(self.args)["status"], "rejected")

    def test_pending_gate_and_held_decisions_stay_held(self):
        self.prepare(passed=False)
        self.decision("release", "held")
        self.decision("qa", "held")
        ledger = build_ledger(self.args)
        self.assertEqual(ledger["gate_verdict"], "pending")
        self.assertEqual(ledger["status"], "held")
        self.args.receipt.write_bytes(self.args.receipt.read_bytes() + b"\n")
        with self.assertRaisesRegex(ValueError, "packet no longer matches"):
            build_ledger(self.args)

    def test_wrong_candidate_and_role_substitution_are_denied(self):
        self.prepare()
        self.decision("qa", "accepted", mutate=lambda value: value.update(archive_sha256="f" * 64))
        with self.assertRaisesRegex(ValueError, "packet or archive"):
            build_ledger(self.args)
        role, _, decision, signature = self.args.signoff[0]
        self.args.signoff[0] = role, "r-owner", decision, signature
        with self.assertRaisesRegex(ValueError, "outside the policy"):
            build_ledger(self.args)

    def test_one_signing_key_cannot_count_for_two_principals(self):
        self.prepare()
        self.write_allowed(dict.fromkeys(self.keys, self.keys["qa"]))
        self.decision("qa", "accepted")
        self.decision("release", "accepted", key=self.keys["qa"])
        with self.assertRaisesRegex(ValueError, "different signing keys"):
            build_ledger(self.args)

    def test_policy_requires_unique_principals_and_roles(self):
        path = self.args.role_policy
        self.roles["roles"]["qa"] = ["r-owner"]
        path.write_text(json.dumps(self.roles), encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "only one role"):
            role_policy(path)
        path.write_text('{"roles":{},"roles":{}}', encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "duplicate field"):
            role_policy(path)
        self.roles["roles"] = {"release": ["r-owner"], "human": ["qa-owner"]}
        path.write_text(json.dumps(self.roles), encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "release and qa"):
            role_policy(path)


if __name__ == "__main__":
    unittest.main()
