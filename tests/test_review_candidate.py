"""Exercise verified local release packets and scoped decisions."""

import io
import json
from pathlib import Path
import tempfile
import unittest
from zipfile import ZipFile

from test_release_qualification import FILES, fingerprint, receipt
from test_verify_host_gates import PROFILES
import review_candidate as review
from verify_candidate import digest_exact
from verify_host_gates import gate_result


class CandidateReviewTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.directory = Path(temp.name)
        self.archive = self.directory / "candidate.zip"
        with ZipFile(self.archive, "x") as package:
            for name, data in FILES.items():
                package.writestr(name, data)
        self.candidate = receipt(self.archive)
        self.receipt_path = self.directory / "qualification.json"
        self.receipt_path.write_text(json.dumps(self.candidate), encoding="utf-8")
        self.profiles = self.directory / "profiles.json"
        self.profiles.write_text(json.dumps(PROFILES), encoding="utf-8")
        self.gates = self.directory / "gates"
        self.gates.mkdir()
        self.packet_path = self.directory / "packet.json"
        self.decision_path = self.directory / "decision.json"

    def packet(self):
        return review.build_packet(self.receipt_path, self.archive, self.gates, self.profiles)

    def write_gate(self, gate, status="passed", **overrides):
        log = self.gates / f"{gate}.log"
        data = b"Observed gate outcome\n"
        log.write_bytes(data)
        record = gate_result(self.candidate, gate, PROFILES[gate], status, {log.name: fingerprint(data)})
        path = self.gates / f"{gate}.gate.json"
        path.write_text(json.dumps({**record, **overrides}), encoding="utf-8")
        return path, log

    def prepare(self):
        packet = self.packet()
        review._write_new(self.packet_path, review._json_bytes(packet), review.MAX_PACKET_BYTES)
        return packet

    def decide(self, packet, status):
        return review.decide(
            packet, self.packet_path, self.decision_path,
            scope="windows-player", status=status, reviewer="QA operator", reason="Observed in the rehearsal",
        )

    def test_exact_size_hash_closes_over_growing_and_shrinking_files(self):
        self.assertEqual(digest_exact(io.BytesIO(b"abc"), 3), fingerprint(b"abc"))
        with self.assertRaisesRegex(ValueError, "grew"):
            digest_exact(io.BytesIO(b"abcd"), 3)
        with self.assertRaisesRegex(ValueError, "shrank"):
            digest_exact(io.BytesIO(b"ab"), 3)

    def test_missing_and_failed_gates_are_visible(self):
        empty = self.packet()
        self.assertEqual(empty["verdict"], "pending")
        self.assertEqual({gate["status"] for gate in empty["gates"]}, {"pending"})
        self.write_gate("clean_player", "failed")
        result = self.packet()
        self.assertEqual(result["verdict"], "rejected")
        self.assertEqual(result["gates"][0]["status"], "failed")
        self.assertEqual(len(result["gates"][0]["record_sha256"]), 64)

    def test_pending_can_be_held_and_checked_but_cannot_be_accepted(self):
        packet = self.prepare()
        with self.assertRaisesRegex(ValueError, "mandatory"):
            self.decide(packet, "accepted")
        decision = self.decide(packet, "held")
        self.assertEqual(review.verify_decision(self.packet(), self.packet_path, self.decision_path), decision)
        with self.assertRaises(FileExistsError):
            self.decide(packet, "held")

    def test_passes_allow_acceptance_until_exact_records_or_evidence_change(self):
        records = {gate: self.write_gate(gate) for gate in PROFILES}
        packet = self.prepare()
        self.assertEqual(packet["verdict"], "accepted")
        self.decide(packet, "accepted")
        record, log = records["headless"]
        original = record.read_bytes()
        record.write_bytes(original + b"\n")
        with self.assertRaisesRegex(ValueError, "packet no longer matches"):
            review.verify_decision(self.packet(), self.packet_path, self.decision_path)
        record.write_bytes(original)
        log.write_bytes(b"Different evidence")
        with self.assertRaisesRegex(ValueError, "evidence hash"):
            self.packet()

    def test_wrong_and_duplicate_gate_results_are_rejected(self):
        record, _ = self.write_gate("editor", archive_sha256="1" * 64)
        with self.assertRaisesRegex(ValueError, "another revision or candidate"):
            self.packet()
        self.write_gate("editor")
        (self.gates / "copy.gate.json").write_bytes(record.read_bytes())
        with self.assertRaisesRegex(ValueError, "Duplicate result"):
            self.packet()

    def test_dirty_candidate_cannot_be_accepted_even_with_four_passes(self):
        self.candidate["source_dirty"] = True
        self.receipt_path.write_text(json.dumps(self.candidate), encoding="utf-8")
        for gate in PROFILES:
            self.write_gate(gate)
        packet = self.prepare()
        self.assertEqual(packet["verdict"], "pending")
        with self.assertRaisesRegex(ValueError, "clean candidate"):
            self.decide(packet, "accepted")


if __name__ == "__main__":
    unittest.main()
