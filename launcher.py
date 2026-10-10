"""Four-button launcher for this workspace. Uses only Python's standard library."""
from __future__ import annotations

import argparse
from dataclasses import dataclass
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parent


@dataclass(frozen=True)
class Action:
    key: str
    label: str
    description: str


ACTIONS = (
    Action("dashboard", "1. Run the dashboard", "Independent administration app"),
    Action("client", "2. Run Angular app in Tauri", "Client desktop application"),
    Action("engine", "3. Run backend graphics engine", "Native game / Vulkan renderer"),
    Action("check", "4. Check Rust for issues", "Cargo check, then Clippy"),
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


def launch_plan(key: str) -> tuple[Path, list[list[str]]]:
    """Resolve allowlisted commands; never interpret user input as shell code."""
    action_for(key)
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
    if key == "check":
        return ROOT, [
            [cargo, "check", "--workspace", "--all-targets"],
            [cargo, "clippy", "--workspace", "--all-targets"],
        ]
    raise ValueError(f"Unknown action: {key}")


def run_action(key: str, dry_run: bool = False) -> int:
    directory, commands = launch_plan(key)
    print(f"\n{action_for(key).label}", flush=True)
    print(f"Directory: {directory}", flush=True)
    for command in commands:
        print(f"\n> {subprocess.list2cmdline(command)}", flush=True)
        if not dry_run:
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
    """Own launcher terminals, while leaving their lifetime under user control."""

    def __init__(self) -> None:
        self.terminals: dict[str, subprocess.Popen] = {}

    def start(self, key: str) -> str:
        action = action_for(key)
        if sys.platform != "win32":
            raise RuntimeError("Opening separate command terminals currently supports Windows.")
        previous = self.terminals.get(key)
        if previous is not None and previous.poll() is None:
            return "This action already has an open terminal. Close it to relaunch."
        launch_plan(key)  # Validate tools/dependencies before opening a terminal.
        terminal = subprocess.Popen(
            [str(console_python()), str(Path(__file__).resolve()), "--run", key],
            cwd=ROOT,
            creationflags=subprocess.CREATE_NEW_CONSOLE,
        )
        self.terminals[key] = terminal
        return f"Opened: {action.description}"


def report_error(message: str) -> None:
    if sys.stderr is not None:
        print(f"\nLauncher error: {message}", file=sys.stderr)
    elif sys.platform == "win32":
        # pythonw has no stderr. Show startup failures instead of silently exiting.
        import ctypes
        ctypes.windll.user32.MessageBoxW(None, message, "Engine Launcher error", 0x10)


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
    window.title("Engine Launcher")
    window.geometry("440x420")
    window.minsize(400, 400)
    window.configure(background="#151a20")
    style = ttk.Style(window)
    style.theme_use("clam")
    style.configure("Launcher.TButton", font=("Segoe UI", 11), padding=(14, 12))
    style.map("Launcher.TButton", background=[("active", "#a6dfb4")])
    frame = tk.Frame(window, background="#151a20", padx=24, pady=22)
    frame.pack(fill="both", expand=True)
    tk.Label(frame, text="ENGINE LAUNCHER", font=("Segoe UI", 17, "bold"),
             foreground="#a6dfb4", background="#151a20").pack(anchor="w")
    tk.Label(frame, text="Choose an action. Output opens in a terminal.",
             foreground="#abb6c3", background="#151a20").pack(anchor="w", pady=(4, 16))
    status = tk.StringVar(value="Ready. Ctrl+C in a command terminal stops it.")
    manager = TerminalManager()

    def start(key: str) -> None:
        try:
            status.set(manager.start(key))
        except (OSError, RuntimeError) as error:
            status.set(str(error))

    first_button = None
    for action in ACTIONS:
        button = ttk.Button(frame, text=action.label, style="Launcher.TButton",
                            command=lambda key=action.key: start(key))
        button.pack(fill="x", pady=4)
        window.bind(str(ACTIONS.index(action) + 1), lambda event, key=action.key: start(key))
        if first_button is None:
            first_button = button
    tk.Label(frame, textvariable=status, wraplength=385, justify="left",
             foreground="#abb6c3", background="#151a20").pack(anchor="w", pady=(12, 0))
    window.bind("<Escape>", lambda event: window.destroy())
    if first_button is not None:
        first_button.focus_set()
    # Closing the launcher leaves command terminals running.
    window.mainloop()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run", choices=[action.key for action in ACTIONS], help="Run one action directly")
    parser.add_argument("--dry-run", action="store_true", help="Print commands without launching them")
    parser.add_argument("--no-pause", action="store_true", help="Exit immediately after a direct command")
    args = parser.parse_args()
    result = 0
    try:
        if args.dry_run:
            for action in ACTIONS:
                if args.run is None or args.run == action.key:
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
