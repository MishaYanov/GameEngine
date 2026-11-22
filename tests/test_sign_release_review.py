"""Exercise real detached signatures with disposable keys and verified local evidence."""

import argparse
from contextlib import redirect_stdout
import io
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch
from zipfile import ZipFile

from test_release_qualification import FILES, receipt
import review_candidate as review
from sign_release_review import NAMESPACE, sign, ssh_keygen
from test_verify_host_gates import PROFILES
from verify_release_review_signature import verify

PRINCIPAL = "test-reviewer"


def has_system_ssh_keygen():
    try:
        ssh_keygen()
        return True
    except (OSError, ValueError):
        return False


class OpenSshLocationTests(unittest.TestCase):
    def test_only_fixed_system_paths_or_absolute_explicit_paths_are_admitted(self):
        with patch("sign_release_review.system_ssh_keygen_paths", return_value=()):
            for explicit in (None, Path("ssh-keygen.exe")):
                with self.assertRaisesRegex(ValueError, "absolute trusted --ssh-keygen"):
                    ssh_keygen(explicit)

    @unittest.skipUnless(has_system_ssh_keygen(), "requires system OpenSSH ssh-keygen")
    def test_explicit_system_executable_is_admitted(self):
        system_executable = Path(ssh_keygen())
        self.assertEqual(Path(ssh_keygen(system_executable)), system_executable)


@unittest.skipUnless(has_system_ssh_keygen(), "requires system OpenSSH ssh-keygen")
class SignedReviewTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.root = root = Path(temp.name)
        self.args = argparse.Namespace(
            receipt=root / "qualification.json", archive=root / "candidate.zip",
            evidence_dir=root / "evidence", profiles=root / "profiles.json",
            packet=root / "review.json", decision=root / "decision.json",
            key=root / "key", signature=root / "decision.sig", playtest=None,
            allowed_signers=root / "allowed-signers", principal=PRINCIPAL, revoked_signers=None,
            ssh_keygen=None,
        )
        args = self.args
        with ZipFile(args.archive, "x") as archive:
            for name, data in FILES.items():
                archive.writestr(name, data)
        args.receipt.write_text(json.dumps(receipt(args.archive)), encoding="utf-8")
        args.profiles.write_text(json.dumps(PROFILES), encoding="utf-8")
        args.evidence_dir.mkdir()
        packet = review.build_packet(args.receipt, args.archive, args.evidence_dir, args.profiles)
        review._write_new(args.packet, review._json_bytes(packet), review.MAX_PACKET_BYTES)
        review.decide(packet, args.packet, args.decision, scope="windows-player", status="held",
                      reviewer="Temporary QA", reason="Hardware result pending")
        generated = subprocess.run(
            [ssh_keygen(), "-q", "-t", "ed25519", "-N", "", "-f", str(args.key)],
            capture_output=True, timeout=10, check=False,
        )
        self.assertEqual(generated.returncode, 0, generated.stderr.decode(errors="replace"))
        self.public = args.key.with_suffix(".pub").read_text(encoding="utf-8").strip()
        self.policy()

    def policy(self, options=None):
        options = f'namespaces="{NAMESPACE}"' if options is None else options
        self.args.allowed_signers.write_text(f"{PRINCIPAL} {options} {self.public}\n", encoding="utf-8")

    def sign(self):
        with redirect_stdout(io.StringIO()):
            return sign(self.args)

    def test_held_review_stays_held_and_signature_cannot_replace_existing_file(self):
        original = self.args.decision.read_bytes()
        self.sign()
        result = verify(self.args)
        self.assertEqual(result["status"], "held")
        self.assertEqual(result["principal"], PRINCIPAL)
        self.assertEqual(self.args.decision.read_bytes(), original)
        signature = self.args.signature.read_bytes()
        with self.assertRaises(FileExistsError):
            self.sign()
        self.assertEqual(self.args.signature.read_bytes(), signature)
        self.args.decision.write_bytes(original + b"\n")
        with self.assertRaisesRegex(ValueError, "configured signer policy"):
            verify(self.args)

    def test_wrong_principal_namespace_expiry_and_revocation_fail(self):
        self.sign()
        self.args.principal = "other-reviewer"
        with self.assertRaisesRegex(ValueError, "configured signer policy"):
            verify(self.args)
        self.args.principal = PRINCIPAL
        for options in ('namespaces="unrelated"', 'valid-before="20000101000000Z"'):
            self.policy(options)
            with self.assertRaisesRegex(ValueError, "configured signer policy"):
                verify(self.args)
        self.policy()
        revoked = self.root / "revoked"
        revoked.write_text(self.public + "\n", encoding="utf-8")
        self.args.revoked_signers = revoked
        with self.assertRaisesRegex(ValueError, "configured signer policy"):
            verify(self.args)

    def test_changed_evidence_policy_mapping_requires_a_new_review(self):
        self.sign()
        profiles = dict(PROFILES, headless="another-runner")
        self.args.profiles.write_text(json.dumps(profiles), encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "packet no longer matches"):
            verify(self.args)


if __name__ == "__main__":
    unittest.main()
