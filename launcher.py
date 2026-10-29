"""Launch the editor, Engine Admin and game from one file. Uses Python's standard library."""
from __future__ import annotations

import argparse
from dataclasses import dataclass
import errno
from html.parser import HTMLParser
import http.client
import os
from pathlib import Path
import shutil
import socket
import subprocess
import sys
import time
from urllib.parse import urlsplit

from launcher_processes import WindowsJob

ROOT = Path(__file__).resolve().parent


@dataclass(frozen=True)
class Action:
	key: str
	label: str
	description: str
	members: tuple[str, ...] = ()
	browser_url: str | None = None
	browser_id: str | None = None


ACTIONS = (
	Action("workspace", "Start workspace", "Editor + Engine Admin", ("client", "dashboard")),
	Action("all", "Start all apps", "Editor + Engine Admin + game", ("client", "dashboard", "engine")),
	Action("client", "Editor", "Editor desktop application", browser_url="http://localhost:4200", browser_id="engine-editor"),
	Action("dashboard", "Engine Admin", "Independent administration app", browser_url="http://127.0.0.1:4301/?preview", browser_id="engine-admin"),
	Action("engine", "Game", "Native game / Vulkan renderer"),
	Action("vulkan-probe", "Vulkan probe", "Vulkan device diagnostics"),
	Action("check", "Rust checks", "Cargo check, then Clippy"),
)


def action_for(key: str) -> Action:
	for action in ACTIONS:
		if action.key == key:
			return action
	raise ValueError(f"Unknown action: {key}")


def executable(name: str) -> str:
	path = shutil.which(name)
	if path is None:
		raise RuntimeError(f"{name} is unavailable. Install it and add it to PATH.")
	return path


def chrome_executable() -> str:
	installed = shutil.which("chrome") or shutil.which("chrome.exe")
	if installed:
		return installed
	for variable in ("PROGRAMFILES", "PROGRAMFILES(X86)", "LOCALAPPDATA"):
		root = os.environ.get(variable)
		if root:
			candidate = Path(root) / "Google/Chrome/Application/chrome.exe"
			if candidate.is_file():
				return str(candidate)
	raise RuntimeError("Google Chrome was not found. Install Chrome or add chrome.exe to PATH.")


class _AppIdentity(HTMLParser):
	def __init__(self) -> None:
		super().__init__()
		self.application = None

	def handle_starttag(self, tag, attributes) -> None:
		values = dict(attributes)
		if tag == "meta" and values.get("name") == "engine-app":
			self.application = values.get("content")


def ensure_dev_port_free(key: str) -> None:
	action = action_for(key)
	if not action.browser_url:
		return
	url = urlsplit(action.browser_url)
	try:
		addresses = socket.getaddrinfo(url.hostname, url.port, type=socket.SOCK_STREAM)
		for family, kind, protocol, _, address in addresses:
			# Test local ownership without connecting. Windows can take longer than
			# a short connection timeout to report that no server is listening.
			with socket.socket(family, kind, protocol) as probe:
				if sys.platform == "win32":
					probe.setsockopt(socket.SOL_SOCKET, socket.SO_EXCLUSIVEADDRUSE, 1)
				probe.bind(address)
	except OSError as error:
		if error.errno in (errno.EADDRINUSE, errno.EACCES):
			raise RuntimeError(
				f"{action.label}: {url.scheme}://{url.netloc} is already in use or unavailable. "
				"Stop the existing server first; the launcher will not attach to another app."
			) from error
		raise RuntimeError(f"Could not check {action.label}'s dev-server port: {error}") from error


def launch_plan(key: str) -> tuple[Path, list[list[str]]]:
	"""Resolve allowlisted commands; never interpret user input as shell code."""
	action = action_for(key)
	if action.members:
		raise ValueError(f"{key} starts multiple apps; resolve its member actions separately")
	cargo = executable("cargo")
	if key in ("dashboard", "client"):
		frontend = ROOT / "apps" / ("admin-dashboard" if key == "dashboard" else "editor")
		cli = frontend / "node_modules" / "@tauri-apps" / "cli" / "tauri.js"
		if not cli.is_file():
			raise RuntimeError(f"Install frontend dependencies first: npm.cmd install in {frontend}")
		command = [executable("node"), str(cli), "dev"]
		if key == "dashboard":
			command.extend(["--features", "desktop"])
		directory = frontend if key == "dashboard" else ROOT / "apps" / "editor-host"
		return directory, [command]
	if key == "engine":
		return ROOT, [[cargo, "run", "-p", "game"]]
	if key == "vulkan-probe":
		return ROOT, [[cargo, "run", "-p", "renderer", "--example", "vulkan_probe"]]
	if key == "check":
		return ROOT, [
			[cargo, "check", "--workspace", "--all-targets"],
			[cargo, "clippy", "--workspace", "--all-targets"],
		]
	raise ValueError(f"Unknown action: {key}")


def run_action(key: str, dry_run: bool = False) -> int:
	action = action_for(key)
	if action.members:
		if dry_run:
			print(f"\n{action.label}: {action.description}", flush=True)
			for member in action.members:
				run_action(member, dry_run=True)
		else:
			manager = TerminalManager()
			try:
				print(manager.start(key), flush=True)
				print("Keep this terminal open. Ctrl+C stops all apps started here.", flush=True)
				manager.wait()
			finally:
				manager.close()
		return 0
	directory, commands = launch_plan(key)
	print(f"\n{action_for(key).label}", flush=True)
	print(f"Directory: {directory}", flush=True)
	for command in commands:
		print(f"\n> {subprocess.list2cmdline(command)}", flush=True)
		if not dry_run:
			ensure_dev_port_free(key)
			result = subprocess.run(command, cwd=directory, check=False)
			if result.returncode:
				return result.returncode
	return 0


def console_python() -> Path:
	"""A launcher started via pythonw still needs visible output terminals."""
	interpreter = Path(sys.executable)
	if interpreter.name.lower() == "pythonw.exe":
		interpreter = interpreter.with_name("python.exe")
	if not interpreter.is_file():
		raise RuntimeError("The Python console interpreter could not be found.")
	return interpreter


class TerminalManager:
	"""Own each app's console and descendants, including Angular dev servers."""

	def __init__(self) -> None:
		self.terminals: dict[str, subprocess.Popen] = {}
		self.jobs: dict[str, WindowsJob] = {}

	def preflight(self, keys: tuple[str, ...]) -> None:
		if sys.platform != "win32":
			raise RuntimeError("Opening separate command terminals currently supports Windows.")
		for key in keys:
			if action_for(key).members:
				raise ValueError("Select individual apps for the launch queue.")
			launch_plan(key)
			if not self.active_count(key):
				ensure_dev_port_free(key)
		console_python()

	def start(self, key: str) -> str:
		action = action_for(key)
		if sys.platform != "win32":
			raise RuntimeError("Opening separate command terminals currently supports Windows.")
		if action.members:
			# Validate the whole group before replacing any running app.
			self.preflight(action.members)
			messages = []
			for member in action.members:
				try:
					messages.append(f"{action_for(member).description}: {self.start(member)}")
				except (OSError, RuntimeError) as error:
					raise RuntimeError(
						f"Could not start {action_for(member).description}: {error}. "
						"Already opened terminals remain available; retry to start the rest."
					) from error
			return "\n".join(messages)
		launch_plan(key)  # Validate tools/dependencies before opening a terminal.
		interpreter = console_python()
		replaced = self.stop(key)
		ensure_dev_port_free(key)
		job = WindowsJob()
		terminal = None
		try:
			terminal = subprocess.Popen(
				[str(interpreter), str(Path(__file__).resolve()), "--run", key, "--no-pause", "--managed"],
				cwd=ROOT, stdin=subprocess.PIPE,
				creationflags=subprocess.CREATE_NEW_CONSOLE,
			)
			# The child waits for this handshake before it may create descendants.
			job.attach(terminal)
			terminal.stdin.write(b"START\n")
			terminal.stdin.flush()
		except BaseException:
			job.close()
			if terminal is not None:
				if terminal.poll() is None:
					terminal.terminate()
				terminal.wait(timeout=3)
			raise
		finally:
			if terminal is not None and terminal.stdin:
				terminal.stdin.close()
		self.terminals[key] = terminal
		self.jobs[key] = job
		return f"{'Restarted' if replaced else 'Opened'}: {action.description}"

	def active_count(self, key: str) -> int:
		job = self.jobs.get(key)
		return job.active_count() if job else 0

	def browser_ready(self, key: str) -> bool:
		action = action_for(key)
		if not action.browser_url or not action.browser_id:
			return False
		try:
			if not self.active_count(key):
				return False
			url = urlsplit(action.browser_url)
			connection = http.client.HTTPConnection(url.hostname, url.port, timeout=0.3)
			try:
				connection.request("GET", "/")
				response = connection.getresponse()
				if response.status != 200:
					return False
				identity = _AppIdentity()
				identity.feed(response.read(64 * 1024).decode("utf-8", errors="replace"))
				return identity.application == action.browser_id
			finally:
				connection.close()
		except (OSError, http.client.HTTPException):
			return False

	def open_in_chrome(self, key: str) -> str:
		action = action_for(key)
		if not action.browser_url:
			raise RuntimeError("This app has no browser UI.")
		if not self.browser_ready(key):
			raise RuntimeError(f"Start {action.label} and wait for its Angular server to be ready at {action.browser_url}. Another app on that port will not be opened.")
		# Chrome uses the user's browser session and is never added to an app job.
		subprocess.Popen([chrome_executable(), "--new-tab", action.browser_url], cwd=ROOT)
		return f"Opened {action.label} in Chrome. Browser preview uses no native desktop bridge."

	def stop(self, key: str) -> bool:
		if action_for(key).members:
			raise ValueError("Stop an individual app or use Stop all.")
		job = self.jobs.get(key)
		if job is None:
			return False
		active = job.active_count() > 0
		if active:
			job.terminate()
			job.wait_empty()
		job.close()
		self.jobs.pop(key, None)
		terminal = self.terminals.pop(key, None)
		if terminal is not None:
			try:
				terminal.wait(timeout=3)
			except subprocess.TimeoutExpired as error:
				raise RuntimeError("The old app terminal did not exit; restart was cancelled.") from error
		return active

	def stop_all(self) -> None:
		failures = []
		for key in tuple(self.jobs):
			try:
				self.stop(key)
			except (OSError, RuntimeError, subprocess.TimeoutExpired) as error:
				failures.append(f"{action_for(key).label}: {error}")
		if failures:
			raise RuntimeError("; ".join(failures))

	def wait(self) -> None:
		while any(job.active_count() for job in self.jobs.values()):
			time.sleep(0.2)

	def close(self) -> None:
		try:
			self.stop_all()
		finally:
			for job in self.jobs.values():
				job.close()
			self.jobs.clear()


@dataclass
class QueueEntry:
	key: str
	status: str = "Queued"
	detail: str = ""


class LaunchQueue:
	"""Ordered launch requests; apps keep running after their terminal opens."""

	def __init__(self, manager: TerminalManager) -> None:
		self.manager = manager
		self.entries: list[QueueEntry] = []
		self.running = False
		self.cursor = 0
		self.notice = "Select one or more apps to build your queue."

	@property
	def keys(self) -> tuple[str, ...]:
		return tuple(entry.key for entry in self.entries)

	def editable(self) -> None:
		if self.running:
			raise RuntimeError("Cancel the remaining queue before editing it.")

	def select(self, key: str, selected: bool) -> None:
		self.editable()
		action = action_for(key)
		if action.members:
			raise ValueError("Select individual apps for the launch queue.")
		if selected and key not in self.keys:
			self.entries.append(QueueEntry(key))
		elif not selected:
			self.entries = [entry for entry in self.entries if entry.key != key]
		self.notice = f"{len(self.entries)} app(s) selected. Launch order is shown below."

	def move(self, key: str, offset: int) -> None:
		self.editable()
		index = self.keys.index(key)
		target = index + offset
		if 0 <= target < len(self.entries):
			self.entries[index], self.entries[target] = self.entries[target], self.entries[index]

	def clear(self) -> None:
		self.editable()
		self.entries.clear()
		self.notice = "Select one or more apps to build your queue."

	def start(self) -> None:
		self.editable()
		if not self.entries:
			raise ValueError("Select at least one app first.")
		# A missing dependency must not leave half the selected workspace open.
		self.manager.preflight(self.keys)
		for entry in self.entries:
			entry.status, entry.detail = "Queued", ""
		self.cursor = 0
		self.running = True
		self.notice = "Starting the launch queue..."

	def step(self) -> bool:
		if not self.running:
			return False
		entry = self.entries[self.cursor]
		try:
			entry.detail = self.manager.start(entry.key)
		except (OSError, RuntimeError) as error:
			entry.status, entry.detail = "Failed", str(error)
			for remaining in self.entries[self.cursor + 1:]:
				remaining.status = "Not started"
			self.running = False
			self.notice = f"Stopped at {action_for(entry.key).label}: {error}. Remaining apps were not started."
			return False
		entry.status = "Terminal open"
		self.cursor += 1
		self.running = self.cursor < len(self.entries)
		self.notice = (
			f"{self.cursor}/{len(self.entries)}: {action_for(entry.key).label} - {entry.detail}"
			if self.running else
			f"{len(self.entries)} launch request(s) completed. Check each terminal for build and runtime output."
		)
		return self.running

	def cancel(self) -> None:
		if self.running:
			for entry in self.entries[self.cursor:]:
				entry.status = "Cancelled"
			self.running = False
			self.notice = "Remaining launches cancelled. Already opened apps keep running."


def report_error(message: str) -> None:
	if sys.stderr is not None:
		print(f"\nLauncher error: {message}", file=sys.stderr)
	elif sys.platform == "win32":
		# pythonw has no stderr. Show startup failures instead of silently exiting.
		import ctypes
		ctypes.windll.user32.MessageBoxW(None, message, "Engine Launcher error", 0x10)


class LauncherWindow:
	def __init__(self, window, tk, ttk) -> None:
		self.window = window
		self.queue = LaunchQueue(TerminalManager())
		self.pending_step = None
		self.pending_poll = None
		window.title("Engine Launcher")
		window.geometry("780x760")
		window.minsize(700, 700)
		window.configure(background="#151a20")
		style = ttk.Style(window)
		style.theme_use("clam")
		style.configure("Launcher.TButton", font=("Segoe UI", 9), padding=(10, 6))
		style.configure("Launcher.TMenubutton", font=("Segoe UI", 10), padding=(10, 8))
		style.configure("Launcher.Treeview", font=("Segoe UI", 10), rowheight=28,
						background="#1d242c", fieldbackground="#1d242c", foreground="#e5eaf0")
		style.configure("Launcher.Treeview.Heading", font=("Segoe UI", 9, "bold"), padding=6)
		style.map("Launcher.Treeview", background=[("selected", "#354c42")], foreground=[("selected", "#ffffff")])
		frame = tk.Frame(window, name="launcher", background="#151a20", padx=20, pady=16)
		frame.pack(fill="both", expand=True)
		tk.Label(frame, text="ENGINE LAUNCHER", font=("Segoe UI", 17, "bold"),
				 foreground="#a6dfb4", background="#151a20").pack(anchor="w")
		tk.Label(frame, text="Select apps from the dropdown, then arrange the launch queue.",
				 foreground="#abb6c3", background="#151a20").pack(anchor="w", pady=(6, 12))
		self.picker = ttk.Menubutton(frame, name="apps", text="Select apps", style="Launcher.TMenubutton")
		self.picker.pack(fill="x")
		self.menu = tk.Menu(self.picker, tearoff=False, font=("Segoe UI", 11))
		self.selections = {}
		for action in ACTIONS:
			if not action.members:
				variable = tk.BooleanVar(window, value=False)
				self.selections[action.key] = variable
				self.menu.add_checkbutton(label=action.label, variable=variable,
									  command=lambda key=action.key: self.select(key))
		self.picker.configure(menu=self.menu)
		tk.Label(frame, text="LAUNCH QUEUE", font=("Segoe UI", 10, "bold"),
				 foreground="#a6dfb4", background="#151a20").pack(anchor="w", pady=(14, 6))
		table = tk.Frame(frame, background="#151a20")
		table.pack(fill="both", expand=True)
		self.tree = ttk.Treeview(table, name="queue", columns=("order", "app", "status"), show="headings",
								style="Launcher.Treeview", selectmode="browse", height=5)
		for column, label, width in (("order", "#", 45), ("app", "Application", 280), ("status", "Status", 180)):
			self.tree.heading(column, text=label, anchor="w")
			self.tree.column(column, width=width, minwidth=40, stretch=column != "order")
		scrollbar = ttk.Scrollbar(table, orient="vertical", command=self.tree.yview)
		self.tree.configure(yscrollcommand=scrollbar.set)
		self.tree.pack(side="left", fill="both", expand=True)
		scrollbar.pack(side="right", fill="y")
		queue_footer = tk.Frame(frame, background="#151a20")
		queue_footer.pack(side="bottom", fill="x", before=table)
		controls = tk.Frame(queue_footer, background="#151a20")
		controls.pack(fill="x", pady=(10, 0))
		self.up = ttk.Button(controls, name="up", text="Move up", style="Launcher.TButton", command=lambda: self.move(-1))
		self.down = ttk.Button(controls, name="down", text="Move down", style="Launcher.TButton", command=lambda: self.move(1))
		self.remove = ttk.Button(controls, name="remove", text="Remove", style="Launcher.TButton", command=self.remove_selected)
		self.clear = ttk.Button(controls, name="clear", text="Clear queue", style="Launcher.TButton", command=self.clear_queue)
		for button in (self.up, self.down, self.remove, self.clear):
			button.pack(side="left", padx=(0, 8))
		self.status = tk.StringVar(window)
		tk.Label(queue_footer, name="status", textvariable=self.status, wraplength=680, justify="left", height=2,
				 foreground="#c6d0db", background="#151a20").pack(anchor="w", pady=(8, 4))
		run_controls = tk.Frame(queue_footer, background="#151a20")
		run_controls.pack(fill="x")
		self.run = ttk.Button(run_controls, name="run", text="Run queue", style="Launcher.TButton", command=self.start)
		self.run.pack(side="left")
		self.cancel = ttk.Button(run_controls, name="cancel", text="Cancel remaining", style="Launcher.TButton", command=self.cancel_remaining)
		self.cancel.pack(side="left", padx=8)
		process_panel = tk.Frame(frame, background="#151a20")
		process_panel.pack(side="bottom", fill="x", before=queue_footer)
		tk.Label(process_panel, text="RUNNING APPS", font=("Segoe UI", 10, "bold"),
				 foreground="#a6dfb4", background="#151a20").pack(anchor="w", pady=(14, 6))
		self.process_tree = ttk.Treeview(process_panel, name="processes", columns=("app", "state"), show="headings",
										style="Launcher.Treeview", selectmode="browse", height=5)
		self.process_tree.heading("app", text="Application", anchor="w")
		self.process_tree.heading("state", text="Process status", anchor="w")
		self.process_tree.column("app", width=320)
		self.process_tree.column("state", width=240)
		self.process_tree.pack(fill="x")
		for action in ACTIONS:
			if not action.members:
				self.process_tree.insert("", "end", iid=action.key, values=(action.label, "Not running"))
		process_controls = tk.Frame(process_panel, background="#151a20")
		process_controls.pack(fill="x", pady=(10, 0))
		self.stop = ttk.Button(process_controls, name="stop", text="Stop app", style="Launcher.TButton", command=self.stop_selected)
		self.restart = ttk.Button(process_controls, name="restart", text="Start / restart app", style="Launcher.TButton", command=self.restart_selected)
		self.chrome = ttk.Button(process_controls, name="chrome", text="Open in Chrome", style="Launcher.TButton", command=self.open_chrome)
		self.stop_all = ttk.Button(process_controls, name="stop_all", text="Stop all", style="Launcher.TButton", command=self.stop_all_apps)
		for button in (self.stop, self.restart, self.chrome, self.stop_all):
			button.pack(side="left", padx=(0, 8))
		tk.Label(process_panel, text="Chrome opens the browser preview; native features use the desktop window.\nClosing the launcher stops all apps it started.",
				 foreground="#abb6c3", background="#151a20", wraplength=680, justify="left").pack(anchor="w", pady=(10, 0))
		self.tree.bind("<<TreeviewSelect>>", lambda event: self.update_controls())
		self.process_tree.bind("<<TreeviewSelect>>", lambda event: self.update_controls())
		self.tree.bind("<Delete>", lambda event: self.remove_selected())
		self.tree.bind("<Alt-Up>", lambda event: self.move(-1))
		self.tree.bind("<Alt-Down>", lambda event: self.move(1))
		window.protocol("WM_DELETE_WINDOW", self.close)
		window.bind("<Escape>", lambda event: self.cancel_remaining() if self.queue.running else self.close())
		self.render()
		self.poll_processes()
		self.picker.focus_set()

	def update_controls(self) -> None:
		selected = self.tree.selection()
		index = self.queue.keys.index(selected[0]) if selected and selected[0] in self.queue.keys else -1
		process_selection = self.process_tree.selection()
		for widget, enabled in (
			(self.picker, not self.queue.running),
			(self.run, not self.queue.running and bool(self.queue.entries)),
			(self.cancel, self.queue.running),
			(self.up, not self.queue.running and index > 0),
			(self.down, not self.queue.running and 0 <= index < len(self.queue.entries) - 1),
			(self.remove, not self.queue.running and index >= 0),
			(self.clear, not self.queue.running and bool(self.queue.entries)),
			(self.stop, not self.queue.running and bool(process_selection)),
			(self.restart, not self.queue.running and bool(process_selection)),
			(self.chrome, bool(process_selection) and self.queue.manager.browser_ready(process_selection[0])),
			(self.stop_all, True),
		):
			widget.configure(state="normal" if enabled else "disabled")

	def render(self, selected: str | None = None) -> None:
		selection = self.tree.selection()
		selected = selected or (selection[0] if selection else None)
		self.tree.delete(*self.tree.get_children())
		for index, entry in enumerate(self.queue.entries, 1):
			self.tree.insert("", "end", iid=entry.key, values=(index, action_for(entry.key).label, entry.status))
		if selected in self.queue.keys:
			self.tree.selection_set(selected)
			self.tree.focus(selected)
		for key, variable in self.selections.items():
			variable.set(key in self.queue.keys)
		self.picker.configure(text=f"Select apps ({len(self.queue.entries)} selected)")
		self.status.set(self.queue.notice)
		self.update_controls()

	def select(self, key: str) -> None:
		self.queue.select(key, self.selections[key].get())
		self.render(key)

	def move(self, offset: int) -> None:
		selected = self.tree.selection()
		if selected and not self.queue.running:
			self.queue.move(selected[0], offset)
			self.render(selected[0])

	def remove_selected(self) -> None:
		selected = self.tree.selection()
		if selected and not self.queue.running:
			self.queue.select(selected[0], False)
			self.render()

	def clear_queue(self) -> None:
		if not self.queue.running:
			self.queue.clear()
			self.render()

	def start(self) -> None:
		try:
			self.queue.start()
		except (OSError, RuntimeError, ValueError) as error:
			self.status.set(str(error))
			return
		self.render()
		self.pending_step = self.window.after(100, self.step)

	def step(self) -> None:
		self.pending_step = None
		more = self.queue.step()
		self.render()
		if more:
			self.pending_step = self.window.after(300, self.step)

	def cancel_remaining(self) -> None:
		if self.pending_step is not None:
			self.window.after_cancel(self.pending_step)
			self.pending_step = None
		self.queue.cancel()
		self.render()

	def poll_processes(self) -> None:
		self.pending_poll = None
		for key in self.process_tree.get_children():
			try:
				count = self.queue.manager.active_count(key)
				state = f"Running ({count} processes)" if count else "Not running"
			except OSError as error:
				state = f"Status unavailable: {error}"
			self.process_tree.item(key, values=(action_for(key).label, state))
		self.update_controls()
		self.pending_poll = self.window.after(1000, self.poll_processes)

	def stop_selected(self) -> None:
		selected = self.process_tree.selection()
		if not selected or self.queue.running:
			return
		try:
			active = self.queue.manager.stop(selected[0])
			self.queue.notice = f"{action_for(selected[0]).label}: {'stopped all app processes' if active else 'already stopped'}."
		except (OSError, RuntimeError) as error:
			self.queue.notice = f"Stop failed: {error}"
		self.render()

	def restart_selected(self) -> None:
		selected = self.process_tree.selection()
		if not selected or self.queue.running:
			return
		try:
			self.queue.notice = self.queue.manager.start(selected[0])
		except (OSError, RuntimeError) as error:
			self.queue.notice = f"Start failed: {error}"
		self.render()

	def stop_all_apps(self) -> None:
		self.cancel_remaining()
		try:
			self.queue.manager.stop_all()
			self.queue.notice = "All apps started by this launcher are stopped."
		except (OSError, RuntimeError) as error:
			self.queue.notice = f"Some apps could not stop: {error}"
		self.render()

	def open_chrome(self) -> None:
		selected = self.process_tree.selection()
		if selected:
			try:
				self.queue.notice = self.queue.manager.open_in_chrome(selected[0])
			except (OSError, RuntimeError) as error:
				self.queue.notice = str(error)
			self.render()

	def close(self) -> None:
		self.cancel_remaining()
		try:
			self.queue.manager.close()
		except (OSError, RuntimeError) as error:
			self.status.set(f"Could not stop all apps: {error}")
			return
		if self.pending_poll is not None:
			self.window.after_cancel(self.pending_poll)
		self.window.destroy()


def show_window() -> None:
	try:
		import tkinter as tk
		from tkinter import ttk
	except ImportError as error:
		raise RuntimeError("Tkinter is unavailable. Install Python with Tcl/Tk support, or use --run in a terminal.") from error
	try:
		window = tk.Tk()
	except tk.TclError as error:
		raise RuntimeError(f"Could not create the launcher window: {error}") from error
	LauncherWindow(window, tk, ttk)
	window.mainloop()


def main() -> int:
	parser = argparse.ArgumentParser(description=__doc__)
	parser.add_argument("--run", choices=[action.key for action in ACTIONS], help="Run an action; workspace/all open apps in separate terminals")
	parser.add_argument("--dry-run", action="store_true", help="Print commands without launching them")
	parser.add_argument("--no-pause", action="store_true", help="Exit immediately after a direct command")
	parser.add_argument("--managed", action="store_true", help=argparse.SUPPRESS)
	args = parser.parse_args()
	result = 0
	try:
		if args.managed:
			if not args.run or args.dry_run or action_for(args.run).members:
				raise RuntimeError("Invalid managed app launch.")
			if sys.stdin is None or sys.stdin.readline() != "START\n":
				raise RuntimeError("Launcher process ownership handshake failed.")
		if args.dry_run:
			for action in ACTIONS:
				if (args.run is None and not action.members) or args.run == action.key:
					run_action(action.key, dry_run=True)
		elif args.run:
			result = run_action(args.run)
		else:
			show_window()
	except KeyboardInterrupt:
		print("\nCommand interrupted.", flush=True)
		result = 130
	except (OSError, RuntimeError) as error:
		report_error(str(error))
		result = 1
	if args.run and not args.dry_run and not args.no_pause and sys.stdin is not None and sys.stdin.isatty():
		try:
			input(f"\nCommand finished (exit {result}). Press Enter to close.")
		except (EOFError, KeyboardInterrupt):
			pass
	return result


if __name__ == "__main__":
	raise SystemExit(main())
