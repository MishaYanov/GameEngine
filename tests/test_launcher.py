"""Regression tests for the launcher; tools and terminals are never launched."""
import contextlib
import io
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import MagicMock, patch

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

    def test_duplicate_launch_is_blocked_and_closed_terminal_can_relaunch(self):
        terminal = MagicMock()
        terminal.poll.return_value = None
        manager = launcher.TerminalManager()
        with patch.object(launcher.sys, "platform", "win32"), patch.object(launcher, "console_python", return_value=self.console), \
                patch.object(launcher.subprocess, "Popen", return_value=terminal) as popen, \
                patch.object(launcher.subprocess, "CREATE_NEW_CONSOLE", 16, create=True):
            self.assertIn("Opened", manager.start("engine"))
            self.assertIn("already", manager.start("engine"))
            self.assertEqual(popen.call_count, 1)
            self.assertEqual(popen.call_args.args[0], [str(self.console), str(Path(launcher.__file__).resolve()), "--run", "engine"])
            self.assertEqual(popen.call_args.kwargs, {"cwd": self.root, "creationflags": 16})
            terminal.poll.return_value = 0
            self.assertIn("Opened", manager.start("engine"))
            self.assertEqual(popen.call_count, 2)

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

    def test_real_buttons_dispatch_individual_actions_and_display_errors(self):
        try:
            import tkinter as tk
            from tkinter import ttk
        except ImportError:
            self.skipTest("Tkinter not installed")
        original_tk = tk.Tk
        created = []

        def hidden_window():
            try:
                window = original_tk()
            except tk.TclError as error:
                self.skipTest(f"Tk display unavailable: {error}")
            window.withdraw()
            created.append(window)
            return window

        def verify_buttons(window, *args):
            def descendants(widget):
                for child in widget.winfo_children():
                    yield child
                    yield from descendants(child)
            buttons = [widget for widget in descendants(window) if isinstance(widget, ttk.Button)]
            self.assertEqual([button.cget("text") for button in buttons], [action.label for action in launcher.ACTIONS])
            for button in buttons:
                button.invoke()
            self.assertEqual([call.args[0] for call in start.call_args_list], ["dashboard", "client", "engine", "check"])
            start.side_effect = RuntimeError("Missing dependency fixture")
            buttons[0].invoke()
            labels = [widget.cget("text") for widget in descendants(window) if isinstance(widget, tk.Label)]
            self.assertIn("Missing dependency fixture", labels)
            self.assertEqual(window.title(), "Engine Launcher")

        try:
            with patch.object(tk, "Tk", side_effect=hidden_window), \
                    patch.object(original_tk, "mainloop", verify_buttons), \
                    patch.object(launcher.TerminalManager, "start", return_value="Opened fixture") as start:
                launcher.show_window()
        finally:
            for window in created:
                window.destroy()


if __name__ == "__main__":
    unittest.main()
