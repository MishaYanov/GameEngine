# Engine launcher

From the repository root:

```powershell
python launcher.py
```

A small Tkinter window offers four buttons. Tkinter is the GUI toolkit underneath Turtle and ships with standard Windows Python installations. Click a button, use Tab/Space, or press 1–4. Each action opens its own visible terminal, showing output and errors. Press Ctrl+C there to stop it, then Enter to close the terminal. Close its terminal before relaunching the same action. Closing the launcher leaves launched terminals running.

| Button | Action |
| --- | --- |
| Dashboard | Installed admin Tauri CLI `dev --features desktop` in `apps/admin-dashboard` |
| Angular app in Tauri | Installed client Tauri CLI `dev` in `apps/editor-host` |
| Backend graphics engine | `cargo run -p game` |
| Check Rust | `cargo check --workspace --all-targets`, then `cargo clippy --workspace --all-targets` |

The Tauri configurations start their respective Angular dev servers. Requires Windows, Python with Tkinter, Cargo, and Node for desktop apps. Install frontend dependencies with `npm.cmd install` in `apps/editor` and `apps/admin-dashboard`. Graphics execution requires the existing Vulkan prerequisites. No additional Python packages are needed.

To open the button window without an extra parent console, run `pythonw launcher.py`.

The same file works as a tiny CLI:

```powershell
python launcher.py --dry-run
python launcher.py --run dashboard
python launcher.py --run client
python launcher.py --run engine
python launcher.py --run check --no-pause
```

Direct actions execute in the current terminal. Rust checks stop on the first failing command. Commands use separate argument lists without shell interpolation, and errors leave output available for inspection.

## Launcher verification

```powershell
python -m unittest discover -s tests -p "test_launcher.py" -v
```

Tests cover all four action mappings, separate client/admin hosts, missing dependencies and tools, safe argument passing with spaces, dry runs, sequential Rust checks and exit codes, duplicate/retried terminal launches, console Python selection, Ctrl+C, missing standard input, and GUI startup errors. A Tkinter smoke test invokes each real button while replacing terminal execution with a mock; it skips explicitly if Tkinter/display support is unavailable. Tests do not launch the dashboard, client or graphics engine.

Tkinter import/window-creation errors produce actionable errors. When started through `pythonw` without stderr, startup errors use a Windows error dialog. Command terminals are tracked separately from button rendering so duplicate launch and retry behavior can be verified without opening terminals.
