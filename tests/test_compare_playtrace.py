"""Focused comparator checks for semantic playtest marks."""

import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
from compare_playtrace import first_difference, marker_rows


def mark(label, gate=False):
    return {
        "label": label,
        "scene_id": "courtyard",
        "simulation_tick": 175,
        "completed": False,
        "score": 1,
        "has_courtyard_key": True,
        "route_gate_open": gate,
    }


class ComparePlaytraceTests(unittest.TestCase):
    def test_reports_the_first_semantic_change(self):
        expected = [mark("before"), mark("opened", True), mark("after")]
        observed = [mark("before"), mark("opened"), mark("after", True)]
        self.assertEqual(first_difference(expected, observed), {
            "index": 1,
            "kind": "facts",
            "label": "opened",
            "fields": {"route_gate_open": {"expected": True, "observed": False}},
        })

    def test_distinguishes_label_order_and_length(self):
        self.assertIsNone(first_difference([mark("one")], [mark("one")]))
        self.assertEqual(first_difference([mark("one")], [mark("two")])["kind"], "label")
        change = first_difference([mark("one"), mark("two")], [mark("one")])
        self.assertEqual(change["kind"], "length")
        self.assertEqual(change["index"], 1)
        self.assertIsNone(change["observed"])

    def test_rejects_duplicate_labels_and_reports_untyped_fields(self):
        with self.assertRaisesRegex(ValueError, "distinct"):
            marker_rows({"markers": [mark("one"), mark("one")]})
        broken = mark("one")
        broken["simulation_tick"] = True
        with self.assertRaisesRegex(ValueError, "facts"):
            marker_rows({"markers": [broken]})

    def test_cli_returns_zero_for_match_one_for_difference_and_two_for_invalid(self):
        with tempfile.TemporaryDirectory() as workdir:
            root = Path(workdir)
            expected = root / "expected.json"
            observed = root / "observed.json"
            expected.write_text(json.dumps({"markers": [mark("open", True)]}))
            command = [
                sys.executable, "-B", str(ROOT / "scripts/compare_playtrace.py"),
                expected.name, observed.name,
            ]
            for value, code in (
                (json.dumps({"markers": [mark("open", True)]}), 0),
                (json.dumps({"markers": [mark("open")]}), 1),
                ('{"markers":[],"markers":[]}', 2),
            ):
                observed.write_text(value)
                result = subprocess.run(command, cwd=root, capture_output=True, check=False)
                self.assertEqual(result.returncode, code, result.stderr)
                if code < 2:
                    self.assertEqual(json.loads(result.stdout)["match"], code == 0)
