"""Native ownership checks using disposable Python process trees, never engine apps."""
from pathlib import Path
import subprocess
import sys
import time
import unittest
from unittest.mock import patch

import launcher
from launcher_processes import WindowsJob


FIXTURE = (
	"import subprocess,sys,time; "
	"assert sys.stdin.readline() == 'START\\n'; "
	"child = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(60)']); "
	"time.sleep(60)"
)


@unittest.skipUnless(sys.platform == "win32", "Windows job-object integration")
class ProcessOwnershipTests(unittest.TestCase):
	def wait_for_tree(self, job):
		deadline = time.monotonic() + 5
		while job.active_count() < 2 and time.monotonic() < deadline:
			time.sleep(0.02)
		self.assertEqual(job.active_count(), 2, "Fixture parent and grandchild must both join the job")

	def test_stop_terminates_parent_and_inherited_descendant(self):
		job = WindowsJob()
		child = subprocess.Popen([sys.executable, "-c", FIXTURE], stdin=subprocess.PIPE,
			stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, creationflags=subprocess.CREATE_NO_WINDOW)
		try:
			job.attach(child)
			self.assertEqual(job.active_count(), 1)
			child.stdin.write(b"START\n")
			child.stdin.close()
			self.wait_for_tree(job)
			job.terminate()
			job.wait_empty()
			self.assertEqual(job.active_count(), 0)
			self.assertIsNotNone(child.wait(timeout=3))
		finally:
			job.close()
			if child.poll() is None:
				child.terminate()
			child.wait(timeout=3)

	def test_restart_replaces_the_tree_and_manager_close_cleans_up(self):
		native_popen = subprocess.Popen
		spawned = []

		def start_fixture(command, **options):
			child = native_popen([sys.executable, "-c", FIXTURE], stdin=subprocess.PIPE,
				stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, creationflags=subprocess.CREATE_NO_WINDOW)
			spawned.append(child)
			return child

		manager = launcher.TerminalManager()
		try:
			with patch.object(launcher, "launch_plan", return_value=(Path.cwd(), [])), \
					patch.object(launcher, "ensure_dev_port_free"), \
					patch.object(launcher.subprocess, "Popen", side_effect=start_fixture):
				self.assertIn("Opened", manager.start("engine"))
				self.wait_for_tree(manager.jobs["engine"])
				previous_job = manager.jobs["engine"]
				self.assertIn("Restarted", manager.start("engine"))
				self.assertIsNotNone(spawned[0].poll())
				self.assertIsNone(previous_job.handle)
				self.wait_for_tree(manager.jobs["engine"])
			manager.close()
			self.assertFalse(manager.jobs)
			self.assertTrue(all(child.poll() is not None for child in spawned))
		finally:
			manager.close()
			for child in spawned:
				if child.poll() is None:
					child.terminate()
				child.wait(timeout=3)

	def test_stopping_one_app_keeps_other_owned_apps_running(self):
		native_popen = subprocess.Popen
		spawned = []

		def start_fixture(command, **options):
			child = native_popen([sys.executable, "-c", FIXTURE], stdin=subprocess.PIPE,
				stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, creationflags=subprocess.CREATE_NO_WINDOW)
			spawned.append(child)
			return child

		manager = launcher.TerminalManager()
		try:
			with patch.object(launcher, "launch_plan", return_value=(Path.cwd(), [])), \
					patch.object(launcher, "ensure_dev_port_free"), \
					patch.object(launcher.subprocess, "Popen", side_effect=start_fixture):
				manager.start("engine")
				manager.start("client")
				for job in manager.jobs.values():
					self.wait_for_tree(job)
			# The app's console can exit while a server descendant is still alive.
			spawned[0].terminate()
			spawned[0].wait(timeout=3)
			self.assertEqual(manager.active_count("engine"), 1)
			self.assertTrue(manager.stop("engine"))
			self.assertEqual(manager.active_count("client"), 2)
			self.assertIsNone(spawned[1].poll())
			manager.stop_all()
			self.assertFalse(manager.jobs)
			self.assertTrue(all(child.poll() is not None for child in spawned))
		finally:
			manager.close()
			for child in spawned:
				if child.poll() is None:
					child.terminate()
				child.wait(timeout=3)


if __name__ == "__main__":
	unittest.main()
