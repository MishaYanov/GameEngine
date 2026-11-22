"""Verify a movable release directory with disposable approvals and ordinary files."""

import argparse
from contextlib import redirect_stderr, redirect_stdout
from datetime import datetime, timedelta, timezone
from io import StringIO
import json
import os
from pathlib import Path
import shutil
import tempfile
import unittest
from unittest.mock import patch

import test_verify_release_signoffs as fixtures
from release_signoff_handoff import INDEX, Inventory, build_index, compare_handoffs, main, verify_index
from review_candidate import _json_bytes
from test_sign_release_review import has_system_ssh_keygen
from verify_release_signoffs import build_ledger


class InventoryTests(unittest.TestCase):
    def test_paths_use_one_case_and_bound_artifact_count(self):
        with tempfile.TemporaryDirectory() as folder:
            inventory = Inventory(Path(folder))
            self.assertEqual(inventory.path("Candidate.zip").name, "Candidate.zip")
            with self.assertRaisesRegex(ValueError, "Ambiguous"):
                inventory.path("candidate.zip")
            for index in range(64):
                name = f"file-{index}"
                (Path(folder) / name).write_bytes(b"record")
                inventory.add(name, 16)
            self.assertEqual(len(inventory.finish()), 64)
            (Path(folder) / "one-more").write_bytes(b"record")
            with self.assertRaisesRegex(ValueError, "more than 64"):
                inventory.add("one-more", 16)

    def test_hardlinked_file_is_outside_inventory_contract(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root / "original").write_bytes(b"record")
            try:
                os.link(root / "original", root / "copy")
            except (NotImplementedError, OSError) as error:
                self.skipTest(f"Hard links unavailable in fixture filesystem: {error}")
            with self.assertRaisesRegex(ValueError, "unlinked"):
                Inventory(root).add("copy", 16)


@unittest.skipUnless(has_system_ssh_keygen(), "requires system OpenSSH ssh-keygen")
class HandoffTests(unittest.TestCase):
    def setUp(self):
        self.fixture = fixtures.ReleaseSignoffTests()
        self.addCleanup(self.fixture.doCleanups)
        self.fixture.setUp()
        self.root = self.fixture.root / "handoff"
        self.root.mkdir()
        self.policy = self.fixture.args

    def prepare(self, roles, *, passed=True, now=None, reviewed_at=None):
        self.fixture.prepare(passed=passed)
        for role, status in roles:
            updates = {"reviewed_at": reviewed_at} if reviewed_at is not None else {}
            self.fixture.decision(role, status, mutate=lambda value: value.update(updates))
        for source, name in ((self.policy.receipt, "qualification.json"),
                             (self.policy.archive, "candidate.zip"),
                             (self.policy.packet, "packet.json")):
            shutil.copyfile(source, self.root / name)
        shutil.copytree(self.policy.evidence_dir, self.root / "evidence")
        if roles:
            signoffs = self.root / "signoffs"
            signoffs.mkdir()
            for role, _, decision, signature in self.policy.signoff:
                shutil.copyfile(decision, signoffs / f"{role}-decision.json")
                shutil.copyfile(signature, signoffs / f"{role}-decision.sig")
        (self.root / "ledger.json").write_bytes(_json_bytes(build_ledger(self.policy, now=now)))

    def fresh_root(self, name):
        self.policy.packet.unlink()
        for _, _, decision, signature in self.policy.signoff:
            decision.unlink()
            signature.unlink()
        self.policy.signoff.clear()
        self.root = self.fixture.root / name
        self.root.mkdir()

    def args(self, root=None):
        return argparse.Namespace(
            root=root or self.root, profiles=self.policy.profiles, role_policy=self.policy.role_policy,
            allowed_signers=self.policy.allowed_signers, revoked_signers=None, ssh_keygen=None,
        )

    def cli(self, command, root=None, *extra):
        args = self.args(root)
        with redirect_stdout(StringIO()), redirect_stderr(StringIO()):
            return main([command, str(args.root), *map(str, extra), "--profiles", str(args.profiles),
                         "--role-policy", str(args.role_policy), "--allowed-signers",
                         str(args.allowed_signers)])

    def test_relocated_held_bundle_reconstructs_and_never_overwrites(self):
        self.prepare([("qa", "held")], passed=False)
        self.assertEqual(self.cli("publish"), 2)
        original = (self.root / INDEX).read_bytes()
        index = json.loads(original)
        names = [row["path"] for row in index["files"]]
        self.assertEqual(names, sorted(names, key=lambda name: name.encode("utf-8")))
        self.assertIn("signoffs/qa-decision.sig", names)
        self.assertNotIn("signoffs/release-decision.sig", names)
        self.assertNotIn("allowed", names)
        relocated = self.fixture.root / "relocated"
        shutil.copytree(self.root, relocated)
        self.assertEqual(self.cli("verify", relocated), 2)
        output = self.fixture.root / "equal.json"
        self.assertEqual(self.cli("compare", self.root, relocated, output), 0)
        comparison = json.loads(output.read_bytes())
        self.assertEqual(comparison["prior"], comparison["current"])
        self.assertEqual(comparison["prior"]["status"], "held")
        for kind in ("added", "changed", "removed"):
            self.assertEqual(comparison[kind], [])
        with self.assertRaises(SystemExit) as occupied_comparison:
            self.cli("compare", self.root, relocated, output)
        self.assertEqual(occupied_comparison.exception.code, 3)
        with self.assertRaises(SystemExit) as occupied:
            self.cli("publish")
        self.assertEqual(occupied.exception.code, 3)
        self.assertEqual((self.root / INDEX).read_bytes(), original)

    def test_changed_verified_evidence_and_decisions_compare_without_modifying_handoffs(self):
        self.prepare([], passed=False)
        self.assertEqual(self.cli("publish"), 2)
        prior = self.root
        self.fresh_root("next")
        self.prepare([("qa", "held")])
        self.assertEqual(self.cli("publish"), 2)
        current = self.root
        before = [(root / INDEX).read_bytes() for root in (prior, current)]
        output = self.fixture.root / "changed.json"
        self.assertEqual(self.cli("compare", prior, current, output), 0)
        change = json.loads(output.read_bytes())
        self.assertEqual(change["prior"]["archive_sha256"], change["current"]["archive_sha256"])
        self.assertNotEqual(change["prior"]["packet_sha256"], change["current"]["packet_sha256"])
        self.assertEqual([row["path"] for row in change["changed"]], ["ledger.json", "packet.json"])
        self.assertIn("evidence/headless.log", [row["path"] for row in change["added"]])
        self.assertIn("signoffs/qa-decision.sig", [row["path"] for row in change["added"]])
        reversed_output = self.fixture.root / "reversed.json"
        self.assertEqual(self.cli("compare", current, prior, reversed_output), 0)
        self.assertEqual(json.loads(reversed_output.read_bytes())["removed"], change["added"])
        with self.assertRaises(SystemExit) as inside_handoff:
            self.cli("compare", prior, current, prior / "comparison.json")
        self.assertEqual(inside_handoff.exception.code, 3)
        self.assertFalse((prior / "comparison.json").exists())
        self.assertEqual([(root / INDEX).read_bytes() for root in (prior, current)], before)

    def test_expired_index_is_preserved_and_renewal_requires_both_current_roles(self):
        archived_at = datetime.now(timezone.utc) - timedelta(hours=2)
        self.prepare([("release", "accepted"), ("qa", "accepted")], now=archived_at,
                     reviewed_at=archived_at.strftime("%Y-%m-%dT%H:%M:%SZ"))
        prior = self.root
        original = _json_bytes(build_index(self.args(), now=archived_at))
        (prior / INDEX).write_bytes(original)
        self.assertEqual(verify_index(self.args(), now=archived_at)["status"], "accepted")
        with self.assertRaisesRegex(ValueError, "current verified evidence"):
            verify_index(self.args())

        self.fresh_root("one-renewed-role")
        self.prepare([("release", "accepted")])
        self.assertEqual(self.cli("publish"), 2)
        self.fresh_root("both-renewed-roles")
        self.prepare([("release", "accepted"), ("qa", "accepted")])
        self.assertEqual(self.cli("publish"), 0)
        relocated = self.fixture.root / "renewed-relocated"
        shutil.copytree(self.root, relocated)
        self.assertEqual(self.cli("verify", relocated), 0)
        output = self.fixture.root / "expired-comparison.json"
        with self.assertRaises(SystemExit) as expired:
            self.cli("compare", prior, relocated, output)
        self.assertEqual(expired.exception.code, 3)
        self.assertFalse(output.exists())
        self.assertEqual((prior / INDEX).read_bytes(), original)

    def test_current_rejection_and_later_expiration(self):
        self.prepare([("release", "rejected")])
        self.assertEqual(self.cli("publish"), 1)
        self.assertEqual(self.cli("verify"), 1)
        with self.assertRaisesRegex(ValueError, "current verified evidence"):
            build_index(self.args(), now=datetime.now(timezone.utc) + timedelta(hours=2))

    def test_changed_signature_and_host_log_invalidate_current_acceptance(self):
        self.prepare([("release", "accepted"), ("qa", "accepted")])
        self.assertEqual(self.cli("publish"), 0)
        self.assertEqual(self.cli("verify"), 0)
        for relative in ("signoffs/qa-decision.sig", "evidence/headless.log"):
            with self.subTest(relative=relative):
                file = self.root / relative
                original = file.read_bytes()
                # The retained evidence must match byte-for-byte, even if a
                # parser tolerates additional whitespace.
                file.write_bytes(original + b"\n")
                with self.assertRaises(SystemExit) as changed:
                    self.cli("verify")
                self.assertEqual(changed.exception.code, 3)
                file.write_bytes(original)

    def test_policy_paths_are_provisioned_independently(self):
        self.prepare([], passed=False)
        args = self.args()
        args.allowed_signers = self.root / "provided-signers"
        shutil.copyfile(self.policy.allowed_signers, args.allowed_signers)
        with self.assertRaisesRegex(ValueError, "outside the handoff"):
            build_index(args)

    def test_comparison_checks_trust_against_both_roots_before_verifying(self):
        current = self.fixture.root / "current"
        current.mkdir()
        args = self.args()
        args.prior_root = self.root
        args.current_root = current
        args.output = self.fixture.root / "comparison.json"
        args.profiles = current / "profiles.json"
        shutil.copyfile(self.policy.profiles, args.profiles)
        with patch("release_signoff_handoff.verify_index") as verify:
            with self.assertRaisesRegex(ValueError, "outside the handoff"):
                compare_handoffs(args)
            verify.assert_not_called()
        self.assertFalse(args.output.exists())


if __name__ == "__main__":
    unittest.main()
