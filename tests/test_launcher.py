"""Regression tests for the launcher; tools and terminals are never launched."""
import contextlib
import io
import json
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import MagicMock, patch
from urllib.parse import urlsplit

import launcher


class LauncherTests(unittest.TestCase):
	def setUp(self):
		self.temp = tempfile.TemporaryDirectory(prefix="launcher tests with spaces ")
		self.addCleanup(self.temp.cleanup)
		self.root = Path(self.temp.name)
		self.addCleanup(patch.stopall)
		patch.object(launcher, "ROOT", self.root).start()
		self.tools = {"cargo": "C:/Tools With Spaces/cargo.exe", "node": "C:/Tools With Spaces/node.exe"}
		patch.object(launcher.shutil, "which", side_effect=self.tools.get).start()
		for folder in ("admin-dashboard", "editor"):
			cli = self.root / "apps" / folder / "node_modules/@tauri-apps/cli/tauri.js"
			cli.parent.mkdir(parents=True)
			cli.write_text("fixture", encoding="utf-8")
		self.console = self.root / "python.exe"
		self.console.touch()
		self.created_jobs = []

		def new_job():
			job = MagicMock()
			job.active_count.return_value = 0
			job.attach.side_effect = lambda child: setattr(job.active_count, "return_value", 1)
			job.wait_empty.side_effect = lambda: setattr(job.active_count, "return_value", 0)
			job.close.side_effect = lambda: setattr(job.active_count, "return_value", 0)
			self.created_jobs.append(job)
			return job

		self.job_factory = patch.object(launcher, "WindowsJob", side_effect=new_job).start()
		self.port_check = patch.object(launcher, "ensure_dev_port_free").start()

	@staticmethod
	def launched_keys(popen):
		return [call.args[0][call.args[0].index("--run") + 1] for call in popen.call_args_list]

	def test_dashboard_and_client_use_separate_frontends_and_correct_hosts(self):
		directory, commands = launcher.launch_plan("dashboard")
		self.assertEqual(directory, self.root / "apps/admin-dashboard")
		self.assertEqual(commands[0][-3:], ["dev", "--features", "desktop"])
		self.assertIn("admin-dashboard", commands[0][1])
		directory, commands = launcher.launch_plan("client")
		self.assertEqual(directory, self.root / "apps/editor-host")
		self.assertEqual(commands[0][-1], "dev")
		self.assertEqual(Path(commands[0][1]), self.root / "apps/editor/node_modules/@tauri-apps/cli/tauri.js")

	def test_engine_and_rust_checks_target_the_workspace(self):
		directory, commands = launcher.launch_plan("engine")
		self.assertEqual(directory, self.root)
		self.assertEqual(commands, [[self.tools["cargo"], "run", "-p", "game"]])
		_, commands = launcher.launch_plan("check")
		self.assertEqual(commands, [[self.tools["cargo"], "check", "--workspace", "--all-targets"],
									[self.tools["cargo"], "clippy", "--workspace", "--all-targets"]])
		_, commands = launcher.launch_plan("vulkan-probe")
		self.assertEqual(commands, [[self.tools["cargo"], "run", "-p", "renderer", "--example", "vulkan_probe"]])

	def test_workspace_and_all_dry_runs_print_members_without_starting_any_process(self):
		for key, expected in (("workspace", ("client", "dashboard")), ("all", ("client", "dashboard", "engine"))):
			with self.subTest(key=key), patch.object(launcher.subprocess, "run") as run, \
					patch.object(launcher.subprocess, "Popen") as popen, contextlib.redirect_stdout(io.StringIO()) as output:
				self.assertEqual(launcher.run_action(key, dry_run=True), 0)
				run.assert_not_called()
				popen.assert_not_called()
				for member in expected:
					self.assertIn(launcher.action_for(member).label, output.getvalue())
				self.assertEqual("run -p game" in output.getvalue(), key == "all")

	def test_missing_dependencies_explain_which_frontend_to_install(self):
		cli = self.root / "apps/editor/node_modules/@tauri-apps/cli/tauri.js"
		cli.unlink()
		with self.assertRaisesRegex(RuntimeError, "npm.cmd install in .*editor"):
			launcher.launch_plan("client")
		launcher.launch_plan("dashboard")

	def test_missing_tools_are_reported(self):
		for name, key in (("cargo", "check"), ("node", "dashboard")):
			with self.subTest(name=name):
				original = self.tools.pop(name)
				try:
					with self.assertRaisesRegex(RuntimeError, f"{name} is unavailable"):
						launcher.launch_plan(key)
				finally:
					self.tools[name] = original

	def test_unknown_actions_are_rejected_before_tool_resolution(self):
		with patch.object(launcher, "executable") as resolve:
			with self.assertRaises(ValueError):
				launcher.launch_plan("check & arbitrary command")
			resolve.assert_not_called()

	def test_dry_run_does_not_start_processes(self):
		with patch.object(launcher.subprocess, "run") as run, contextlib.redirect_stdout(io.StringIO()) as output:
			self.assertEqual(launcher.run_action("check", dry_run=True), 0)
			run.assert_not_called()
			self.assertIn("clippy", output.getvalue())

	def test_rust_checks_stop_on_failure_and_preserve_exit_code(self):
		with patch.object(launcher.subprocess, "run", return_value=subprocess.CompletedProcess([], 7)) as run:
			with contextlib.redirect_stdout(io.StringIO()):
				self.assertEqual(launcher.run_action("check"), 7)
			self.assertEqual(run.call_count, 1)

	def test_successful_check_runs_clippy_without_shell_interpolation(self):
		with patch.object(launcher.subprocess, "run", return_value=subprocess.CompletedProcess([], 0)) as run:
			with contextlib.redirect_stdout(io.StringIO()):
				self.assertEqual(launcher.run_action("check"), 0)
			self.assertEqual(run.call_count, 2)
			self.assertEqual(run.call_args.args[0][1], "clippy")
			self.assertEqual(run.call_args.kwargs, {"cwd": self.root, "check": False})
			self.assertIsInstance(run.call_args.args[0], list)

	def test_pythonw_uses_console_interpreter_and_checks_it_exists(self):
		with patch.object(launcher.sys, "executable", str(self.root / "pythonw.exe")):
			self.assertEqual(launcher.console_python(), self.console)
			self.console.unlink()
			with self.assertRaisesRegex(RuntimeError, "console interpreter"):
				launcher.console_python()

	def test_running_app_is_stopped_and_drained_before_relaunch(self):
		terminal = MagicMock()
		terminal.poll.return_value = None
		manager = launcher.TerminalManager()
		with patch.object(launcher.sys, "platform", "win32"), patch.object(launcher, "console_python", return_value=self.console), \
				patch.object(launcher.subprocess, "Popen", return_value=terminal) as popen, \
				patch.object(launcher.subprocess, "CREATE_NEW_CONSOLE", 16, create=True):
			self.assertIn("Opened", manager.start("engine"))
			self.assertIn("Restarted", manager.start("engine"))
			self.assertEqual(popen.call_count, 2)
			self.created_jobs[0].terminate.assert_called_once()
			self.created_jobs[0].wait_empty.assert_called_once()
			self.created_jobs[0].close.assert_called_once()
			self.assertEqual(popen.call_args.args[0], [str(self.console), str(Path(launcher.__file__).resolve()), "--run", "engine", "--no-pause", "--managed"])
			self.assertEqual(popen.call_args.kwargs, {"cwd": self.root, "creationflags": 16, "stdin": subprocess.PIPE})
			terminal.stdin.write.assert_called_with(b"START\n")
			terminal.poll.return_value = 0
			self.created_jobs[1].active_count.return_value = 0
			self.assertIn("Opened", manager.start("engine"))
			self.assertEqual(popen.call_count, 3)

	def test_failed_terminal_creation_allows_retry(self):
		manager = launcher.TerminalManager()
		with patch.object(launcher.sys, "platform", "win32"), patch.object(launcher, "console_python", return_value=self.console), \
				patch.object(launcher.subprocess, "Popen", side_effect=[OSError("spawn failed"), MagicMock()]) as popen, \
				patch.object(launcher.subprocess, "CREATE_NEW_CONSOLE", 16, create=True):
			with self.assertRaisesRegex(OSError, "spawn failed"):
				manager.start("engine")
			self.assertNotIn("engine", manager.terminals)
			self.assertIn("Opened", manager.start("engine"))
			self.assertEqual(popen.call_count, 2)

	def test_group_launches_replace_selected_running_apps_and_leave_other_apps_alone(self):
		terminal = MagicMock()
		terminal.poll.return_value = None
		manager = launcher.TerminalManager()
		with patch.object(launcher.sys, "platform", "win32"), patch.object(launcher, "console_python", return_value=self.console), \
				patch.object(launcher.subprocess, "Popen", return_value=terminal) as popen, \
				patch.object(launcher.subprocess, "CREATE_NEW_CONSOLE", 16, create=True):
			manager.start("client")
			manager.start("workspace")
			self.assertEqual(self.launched_keys(popen), ["client", "client", "dashboard"])
			manager.start("all")
			self.assertEqual(self.launched_keys(popen), ["client", "client", "dashboard", "client", "dashboard", "engine"])
			manager.start("workspace")
			self.assertEqual(popen.call_count, 8)
			self.assertEqual(manager.active_count("engine"), 1)

	def test_group_preflights_every_member_before_opening_a_terminal(self):
		(self.root / "apps/admin-dashboard/node_modules/@tauri-apps/cli/tauri.js").unlink()
		with patch.object(launcher.sys, "platform", "win32"), patch.object(launcher.subprocess, "Popen") as popen:
			with self.assertRaisesRegex(RuntimeError, "Install frontend dependencies"):
				launcher.TerminalManager().start("all")
			popen.assert_not_called()

	def test_partial_group_failure_preserves_started_terminals_and_retry_replaces_them(self):
		terminal = MagicMock()
		terminal.poll.return_value = None
		manager = launcher.TerminalManager()
		with patch.object(launcher.sys, "platform", "win32"), patch.object(launcher, "console_python", return_value=self.console), \
				patch.object(launcher.subprocess, "Popen", side_effect=[terminal, OSError("spawn failed"), terminal, terminal, terminal]) as popen, \
				patch.object(launcher.subprocess, "CREATE_NEW_CONSOLE", 16, create=True):
			with self.assertRaisesRegex(RuntimeError, "Already opened terminals remain available"):
				manager.start("all")
			self.assertEqual(list(manager.terminals), ["client"])
			manager.start("all")
			self.assertEqual(self.launched_keys(popen), ["client", "dashboard", "client", "dashboard", "engine"])
			self.assertEqual(set(manager.terminals), {"client", "dashboard", "engine"})

	def test_group_cli_opens_terminals_instead_of_blocking_on_the_first_app(self):
		with patch.object(sys, "argv", ["launcher.py", "--run", "all", "--no-pause"]), \
				patch.object(launcher.TerminalManager, "start", return_value="Opened all") as start, \
				patch.object(launcher.subprocess, "run") as run, contextlib.redirect_stdout(io.StringIO()):
			self.assertEqual(launcher.main(), 0)
			start.assert_called_once_with("all")
			run.assert_not_called()

	def test_selection_order_deduplication_and_reordering_define_the_queue(self):
		queue = launcher.LaunchQueue(MagicMock())
		for key in ("dashboard", "engine", "client", "dashboard"):
			queue.select(key, True)
		self.assertEqual(queue.keys, ("dashboard", "engine", "client"))
		queue.move("client", -1)
		queue.move("dashboard", -1)  # Already first.
		self.assertEqual(queue.keys, ("dashboard", "client", "engine"))
		queue.select("client", False)
		queue.select("client", True)
		self.assertEqual(queue.keys, ("dashboard", "engine", "client"))
		with self.assertRaises(ValueError):
			queue.select("all", True)
		with self.assertRaises(ValueError):
			queue.select("not-an-app", True)
		queue.clear()
		with self.assertRaisesRegex(ValueError, "at least one"):
			queue.start()

	def test_queue_preflights_before_any_launch_and_starts_in_selected_order(self):
		terminal = MagicMock()
		terminal.poll.return_value = None
		queue = launcher.LaunchQueue(launcher.TerminalManager())
		for key in ("dashboard", "engine", "client"):
			queue.select(key, True)
		with patch.object(launcher.sys, "platform", "win32"), patch.object(launcher, "console_python", return_value=self.console), \
				patch.object(launcher.subprocess, "Popen", return_value=terminal) as popen, \
				patch.object(launcher.subprocess, "CREATE_NEW_CONSOLE", 16, create=True):
			queue.start()
			popen.assert_not_called()
			self.assertTrue(queue.step())
			self.assertTrue(queue.step())
			self.assertFalse(queue.step())
			self.assertEqual(self.launched_keys(popen), ["dashboard", "engine", "client"])
			self.assertEqual([entry.status for entry in queue.entries], ["Terminal open"] * 3)
			self.assertFalse(queue.step())
			self.assertEqual(popen.call_count, 3)

	def test_queue_preflight_failure_does_not_start_any_app(self):
		manager = MagicMock()
		manager.preflight.side_effect = RuntimeError("missing tool")
		queue = launcher.LaunchQueue(manager)
		queue.select("client", True)
		queue.select("dashboard", True)
		with self.assertRaisesRegex(RuntimeError, "missing tool"):
			queue.start()
		self.assertFalse(queue.running)
		manager.start.assert_not_called()

	def test_queue_failure_stops_remaining_launches_and_exposes_the_error(self):
		manager = MagicMock()
		manager.start.side_effect = ["Opened", OSError("cannot create terminal")]
		queue = launcher.LaunchQueue(manager)
		for key in ("engine", "client", "dashboard"):
			queue.select(key, True)
		queue.start()
		self.assertTrue(queue.step())
		self.assertFalse(queue.step())
		self.assertFalse(queue.step())
		self.assertEqual([call.args[0] for call in manager.start.call_args_list], ["engine", "client"])
		self.assertEqual([entry.status for entry in queue.entries], ["Terminal open", "Failed", "Not started"])
		self.assertIn("cannot create terminal", queue.notice)

	def test_queue_cancel_preserves_open_apps_blocks_editing_during_launch_and_allows_retry(self):
		manager = MagicMock()
		queue = launcher.LaunchQueue(manager)
		for key in ("client", "dashboard"):
			queue.select(key, True)
		queue.start()
		for edit in (lambda: queue.select("engine", True), lambda: queue.move("client", 1), queue.clear, queue.start):
			with self.assertRaisesRegex(RuntimeError, "Cancel"):
				edit()
		queue.step()
		queue.cancel()
		self.assertEqual([entry.status for entry in queue.entries], ["Terminal open", "Cancelled"])
		self.assertFalse(queue.step())
		manager.start.assert_called_once_with("client")
		queue.select("client", False)
		queue.start()
		self.assertFalse(queue.step())
		self.assertEqual(manager.start.call_args.args, ("dashboard",))

	def test_restart_validation_keeps_a_running_app_when_dependencies_are_missing(self):
		manager = launcher.TerminalManager()
		job = MagicMock()
		manager.jobs["client"] = job
		(self.root / "apps/editor/node_modules/@tauri-apps/cli/tauri.js").unlink()
		with patch.object(launcher.sys, "platform", "win32"), patch.object(launcher.subprocess, "Popen") as popen:
			with self.assertRaisesRegex(RuntimeError, "Install frontend"):
				manager.start("client")
			job.terminate.assert_not_called()
			popen.assert_not_called()

	def test_restart_does_not_spawn_until_old_process_tree_has_drained(self):
		manager = launcher.TerminalManager()
		job = MagicMock()
		job.active_count.return_value = 2
		job.wait_empty.side_effect = RuntimeError("old processes still running")
		manager.jobs["engine"] = job
		with patch.object(launcher.sys, "platform", "win32"), patch.object(launcher, "console_python", return_value=self.console), \
				patch.object(launcher.subprocess, "Popen") as popen:
			with self.assertRaisesRegex(RuntimeError, "old processes still running"):
				manager.start("engine")
			job.terminate.assert_called_once()
			job.close.assert_not_called()
			popen.assert_not_called()

	def test_stop_all_attempts_every_owned_job_even_if_one_fails(self):
		manager = launcher.TerminalManager()
		first, second = MagicMock(), MagicMock()
		first.active_count.return_value = second.active_count.return_value = 1
		first.terminate.side_effect = OSError("stop failed")
		manager.jobs.update(client=first, dashboard=second)
		with self.assertRaisesRegex(RuntimeError, "stop failed"):
			manager.stop_all()
		first.terminate.assert_called_once()
		second.terminate.assert_called_once()
		self.assertEqual(manager.jobs, {"client": first})

	def test_failed_job_attachment_never_releases_the_app_start_handshake(self):
		terminal = MagicMock()
		terminal.poll.return_value = None
		job = MagicMock()
		job.attach.side_effect = OSError("cannot own process")
		self.job_factory.side_effect = None
		self.job_factory.return_value = job
		manager = launcher.TerminalManager()
		with patch.object(launcher.sys, "platform", "win32"), patch.object(launcher, "console_python", return_value=self.console), \
				patch.object(launcher.subprocess, "Popen", return_value=terminal), \
				patch.object(launcher.subprocess, "CREATE_NEW_CONSOLE", 16, create=True):
			with self.assertRaisesRegex(OSError, "cannot own process"):
				manager.start("client")
			terminal.stdin.write.assert_not_called()
			terminal.terminate.assert_called_once()
			job.close.assert_called_once()
			self.assertFalse(manager.jobs)

	def test_managed_child_rejects_a_missing_handshake_before_running_an_app(self):
		with patch.object(sys, "argv", ["launcher.py", "--run", "engine", "--managed", "--no-pause"]), \
				patch.object(sys, "stdin", io.StringIO("not-start\n")), patch.object(launcher, "run_action") as run, \
				contextlib.redirect_stderr(io.StringIO()):
			self.assertEqual(launcher.main(), 1)
			run.assert_not_called()

	def test_chrome_opens_ready_web_apps_as_browser_previews_without_job_ownership(self):
		manager = launcher.TerminalManager()
		self.tools["chrome"] = "C:/Tools With Spaces/chrome.exe"
		for key, url, host, port, identity in (("client", "http://localhost:4200", "localhost", 4200, "engine-editor"),
							("dashboard", "http://127.0.0.1:4301/?preview", "127.0.0.1", 4301, "engine-admin")):
			with self.subTest(key=key):
				job = MagicMock()
				job.active_count.return_value = 1
				manager.jobs[key] = job
				with patch.object(launcher.http.client, "HTTPConnection") as connect, \
						patch.object(launcher.subprocess, "Popen") as popen:
					response = connect.return_value.getresponse.return_value
					response.status = 200
					response.read.return_value = f"<meta content='{identity}' name='engine-app'>".encode()
					self.assertIn("no native desktop bridge", manager.open_in_chrome(key))
					connect.assert_called_once_with(host, port, timeout=0.3)
					connect.return_value.request.assert_called_once_with("GET", "/")
					connect.return_value.close.assert_called_once()
					popen.assert_called_once_with([self.tools["chrome"], "--new-tab", url], cwd=self.root)
					job.attach.assert_not_called()
					self.job_factory.assert_not_called()

	def test_chrome_requires_a_ready_server_and_rejects_native_only_apps(self):
		manager = launcher.TerminalManager()
		with patch.object(launcher.subprocess, "Popen") as popen:
			with self.assertRaisesRegex(RuntimeError, "no browser UI"):
				manager.open_in_chrome("engine")
			with self.assertRaisesRegex(RuntimeError, "Angular server"):
				manager.open_in_chrome("client")
			job = MagicMock()
			job.active_count.return_value = 1
			manager.jobs["client"] = job
			with patch.object(launcher.http.client, "HTTPConnection", side_effect=ConnectionRefusedError()):
				self.assertFalse(manager.browser_ready("client"))
				with self.assertRaisesRegex(RuntimeError, "Angular server"):
					manager.open_in_chrome("client")
			popen.assert_not_called()

	def test_an_open_port_serving_another_app_is_not_browser_ready(self):
		manager = launcher.TerminalManager()
		job = MagicMock()
		job.active_count.return_value = 1
		manager.jobs["dashboard"] = job
		with patch.object(launcher.http.client, "HTTPConnection") as connect, \
				patch.object(launcher.subprocess, "Popen") as popen:
			response = connect.return_value.getresponse.return_value
			response.status = 200
			for wrong_page in (b"<title>Editor Design Workbench</title><app-root></app-root>",
							   b"<meta name='engine-app' content='engine-editor'>"):
				response.read.return_value = wrong_page
				self.assertFalse(manager.browser_ready("dashboard"))
				with self.assertRaisesRegex(RuntimeError, "Another app on that port"):
					manager.open_in_chrome("dashboard")
			response.status = 503
			response.read.return_value = b"<meta name='engine-app' content='engine-admin'>"
			self.assertFalse(manager.browser_ready("dashboard"))
			popen.assert_not_called()

	def test_busy_dev_port_is_rejected_before_a_native_window_can_attach(self):
		self.port_check.side_effect = RuntimeError("dev port is already in use")
		with patch.object(launcher.sys, "platform", "win32"), patch.object(launcher, "console_python", return_value=self.console), \
				patch.object(launcher.subprocess, "Popen") as popen:
			with self.assertRaisesRegex(RuntimeError, "already in use"):
				launcher.TerminalManager().start("dashboard")
			popen.assert_not_called()
			self.job_factory.assert_not_called()

	def test_admin_server_native_window_and_browser_share_a_dedicated_endpoint(self):
		root = Path(launcher.__file__).resolve().parent
		package = json.loads((root / "apps/admin-dashboard/package.json").read_text(encoding="utf-8"))
		config = json.loads((root / "apps/admin-dashboard/src-tauri/tauri.conf.json").read_text(encoding="utf-8"))
		url = urlsplit(launcher.action_for("dashboard").browser_url)
		arguments = package["scripts"]["start"].split()
		self.assertEqual(arguments[:3], ["ng", "serve", "admin-dashboard"])
		self.assertEqual(arguments[arguments.index("--host") + 1], url.hostname)
		self.assertEqual(int(arguments[arguments.index("--port") + 1]), url.port)
		self.assertEqual(config["build"]["devUrl"], f"{url.scheme}://{url.netloc}")
		design_config = (root / "apps/editor/angular.json").read_text(encoding="utf-8").split('"design":', 1)[1]
		design_port = int(re.search(r'"port"\s*:\s*(\d+)', design_config).group(1))
		self.assertNotEqual(url.port, design_port, "Admin must not share the design-guide port")
		for key, index_file in (("client", "apps/editor/src/index.html"), ("dashboard", "apps/admin-dashboard/src/index.html")):
			identity = launcher._AppIdentity()
			identity.feed((root / index_file).read_text(encoding="utf-8"))
			self.assertEqual(identity.application, launcher.action_for(key).browser_id)

	def test_missing_chrome_has_an_actionable_error(self):
		with patch.dict(launcher.os.environ, {}, clear=True):
			with self.assertRaisesRegex(RuntimeError, "Chrome was not found"):
				launcher.chrome_executable()

	def test_preflight_failure_and_unsupported_platform_do_not_open_terminals(self):
		manager = launcher.TerminalManager()
		with patch.object(launcher.subprocess, "Popen") as popen:
			with patch.object(launcher.sys, "platform", "win32"), patch.object(launcher, "launch_plan", side_effect=RuntimeError("missing node")):
				with self.assertRaisesRegex(RuntimeError, "missing node"):
					manager.start("dashboard")
			with patch.object(launcher.sys, "platform", "linux"):
				with self.assertRaisesRegex(RuntimeError, "Windows"):
					manager.start("engine")
			popen.assert_not_called()

	def test_cli_interrupt_and_errors_return_failure(self):
		for error, expected in ((KeyboardInterrupt(), 130), (RuntimeError("missing cargo"), 1)):
			with self.subTest(error=type(error).__name__), patch.object(sys, "argv", ["launcher.py", "--run", "check", "--no-pause"]), \
					patch.object(launcher, "run_action", side_effect=error), \
					contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
				self.assertEqual(launcher.main(), expected)

	def test_missing_stdin_does_not_crash_after_command(self):
		with patch.object(sys, "argv", ["launcher.py", "--run", "check"]), patch.object(sys, "stdin", None), \
				patch.object(launcher, "run_action", return_value=0):
			self.assertEqual(launcher.main(), 0)

	def test_pythonw_startup_error_uses_dialog_when_stderr_is_absent(self):
		import ctypes
		windll = MagicMock()
		with patch.object(sys, "stderr", None), patch.object(sys, "platform", "win32"), patch.object(ctypes, "windll", windll, create=True):
			launcher.report_error("Tk unavailable")
			windll.user32.MessageBoxW.assert_called_once_with(None, "Tk unavailable", "Engine Launcher error", 0x10)

	def test_missing_tkinter_has_actionable_error(self):
		with patch.dict(sys.modules, {"tkinter": None}):
			with self.assertRaisesRegex(RuntimeError, "Tcl/Tk support"):
				launcher.show_window()

	def test_tk_window_creation_failure_is_handled(self):
		try:
			import tkinter as tk
		except ImportError:
			self.skipTest("Tkinter not installed")
		with patch.object(tk, "Tk", side_effect=tk.TclError("no display")):
			with self.assertRaisesRegex(RuntimeError, "Could not create.*no display"):
				launcher.show_window()

	def test_real_dropdown_queue_controls_cancel_and_preflight_errors(self):
		try:
			import tkinter as tk
			from tkinter import ttk
		except ImportError:
			self.skipTest("Tkinter not installed")
		original_tk = tk.Tk
		created = []
		views = []
		original_view = launcher.LauncherWindow

		def capture_view(window, tk_module, ttk_module):
			view = original_view(window, tk_module, ttk_module)
			views.append(view)
			return view

		def hidden_window():
			try:
				window = original_tk()
			except tk.TclError as error:
				self.skipTest(f"Tk display unavailable: {error}")
			window.withdraw()
			window.geometry("+10000+10000")
			created.append(window)
			return window

		def verify_buttons(window, *args):
			view = views[0]
			window.deiconify()
			window.update()
			self.assertGreaterEqual(view.tree.winfo_height(), 56,
				f"Launch queue must retain visible rows; window={window.winfo_geometry()}, frame={view.tree.master.master.winfo_geometry()}")
			for button in (view.stop, view.restart, view.chrome, view.stop_all):
				bottom = button.winfo_rooty() - window.winfo_rooty() + button.winfo_height()
				self.assertLessEqual(bottom, window.winfo_height(), f"{button.cget('text')} must fit in the window")
			window.withdraw()
			self.assertTrue(view.run.instate(["disabled"]))
			self.assertEqual([view.menu.entrycget(i, "label") for i in range(view.menu.index("end") + 1)],
						 [action.label for action in launcher.ACTIONS if not action.members])
			# A real checkbutton toggles selection; deselecting and selecting again
			# appends that app to the end, matching selection order.
			for index in (1, 0, 2, 0, 0):
				view.menu.invoke(index)
			self.assertEqual(view.tree.get_children(), ("dashboard", "engine", "client"))
			view.tree.selection_set("client")
			view.update_controls()
			view.up.invoke()
			self.assertEqual(view.tree.get_children(), ("dashboard", "client", "engine"))
			self.assertEqual(view.tree.item("client", "values"), ("2", "Editor", "Queued"))
			view.remove.invoke()
			self.assertFalse(view.selections["client"].get())
			callbacks = {}

			def schedule(delay, callback):
				identifier = f"fixture-{len(callbacks) + 1}"
				callbacks[identifier] = callback
				return identifier

			with patch.object(window, "after", side_effect=schedule), \
					patch.object(window, "after_cancel", side_effect=lambda identifier: callbacks.pop(identifier)):
				view.run.invoke()
				self.assertTrue(view.picker.instate(["disabled"]))
				self.assertTrue(view.run.instate(["disabled"]))
				start.assert_not_called()
				callbacks.pop(next(iter(callbacks)))()
				start.assert_called_once_with("dashboard")
				view.cancel.invoke()
				self.assertFalse(callbacks)
				self.assertEqual(view.tree.item("engine", "values")[-1], "Cancelled")
				self.assertTrue(view.cancel.instate(["disabled"]))
			view.clear.invoke()
			self.assertFalse(view.tree.get_children())
			self.assertTrue(view.run.instate(["disabled"]))
			view.menu.invoke(0)
			preflight.side_effect = RuntimeError("Missing dependency fixture")
			view.run.invoke()
			self.assertEqual(view.status.get(), "Missing dependency fixture")
			start.assert_called_once_with("dashboard")
			start.reset_mock()
			view.process_tree.selection_set("engine")
			view.update_controls()
			view.restart.invoke()
			start.assert_called_once_with("engine")
			view.stop.invoke()
			stop.assert_called_once_with("engine")
			view.stop_all.invoke()
			stop_all.assert_called_once()
			self.assertIn("are stopped", view.status.get())
			view.process_tree.selection_set("dashboard")
			view.update_controls()
			self.assertFalse(view.chrome.instate(["disabled"]))
			view.chrome.invoke()
			chrome.assert_called_once_with("dashboard")
			self.assertEqual(window.title(), "Engine Launcher")

		try:
			with patch.object(tk, "Tk", side_effect=hidden_window), \
					patch.object(launcher, "LauncherWindow", side_effect=capture_view), \
					patch.object(original_tk, "mainloop", verify_buttons), \
					patch.object(launcher.TerminalManager, "preflight") as preflight, \
					patch.object(launcher.TerminalManager, "stop", return_value=True) as stop, \
					patch.object(launcher.TerminalManager, "stop_all") as stop_all, \
					patch.object(launcher.TerminalManager, "browser_ready", return_value=True), \
					patch.object(launcher.TerminalManager, "open_in_chrome", return_value="Opened browser preview") as chrome, \
					patch.object(launcher.TerminalManager, "start", return_value="Opened fixture") as start:
				launcher.show_window()
		finally:
			for window in created:
				window.destroy()


if __name__ == "__main__":
	unittest.main()
