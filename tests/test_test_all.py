"""Verify aggregate failures cannot be hidden by a later passing suite."""
import contextlib
import io
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch

import test_all


class TestAllTests(unittest.TestCase):
	def test_failure_continues_and_returns_failure(self):
		commands = [("first", Path.cwd(), ["first"]), ("second", Path.cwd(), ["second"])]
		with patch.object(test_all.subprocess, "run", side_effect=[subprocess.CompletedProcess([], 9), subprocess.CompletedProcess([], 0)]) as run, contextlib.redirect_stdout(io.StringIO()) as output:
			self.assertEqual(test_all.run_suites(commands), 1)
		self.assertEqual(run.call_count, 2)
		self.assertIn("FAIL: first (exit 9)", output.getvalue())
		self.assertIn("PASS: second", output.getvalue())

	def test_missing_tool_is_failure_and_other_suites_still_run(self):
		with patch.object(test_all.subprocess, "run", side_effect=[FileNotFoundError("missing tool"), subprocess.CompletedProcess([], 0)]) as run, contextlib.redirect_stdout(io.StringIO()):
			self.assertEqual(test_all.run_suites([("missing", Path.cwd(), ["missing"]), ("ok", Path.cwd(), ["ok"])]), 1)
		self.assertEqual(run.call_count, 2)

	def test_interrupt_stops_with_130(self):
		with patch.object(test_all.subprocess, "run", side_effect=KeyboardInterrupt), contextlib.redirect_stdout(io.StringIO()):
			self.assertEqual(test_all.main([]), 130)

	def test_native_build_is_explicit(self):
		self.assertEqual(len(test_all.suites()), 5)
		self.assertFalse(any("renderer_smoke" in command for _, _, command in test_all.suites()))
		self.assertFalse(any("scene_io_smoke" in command for _, _, command in test_all.suites()))

	def test_native_mode_includes_runtime_and_generated_game_checks(self):
		native = test_all.suites(True)
		self.assertEqual(len(native), 9)
		self.assertEqual([suite[0] for suite in native[-4:]], ["Native Vulkan runtime", "Native generated game", "Native scene rendering", "Native scene save/reload"])
		for _, _, command in native[-4:-2]:
			self.assertIn("--ignored", command)
			self.assertIn("--locked", command)
		for (_, _, command), example in zip(native[-2:], ["renderer_smoke", "scene_io_smoke"]):
			self.assertIn(example, command)
			self.assertIn("run", command)
			self.assertIn("--locked", command)
