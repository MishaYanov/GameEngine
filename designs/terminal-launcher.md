# Engine launcher

From the repository root:

```powershell
python launcher.py
```

A Tkinter window provides **Select apps**, an ordered launch queue and a running-app list. Tkinter ships with standard
Windows Python installations. Choose apps, use **Move up** / **Move down** to set their order, and click **Run queue**.
Each action opens a visible terminal with build/runtime output. **Terminal open** confirms launch, not application
readiness or build success. The queue starts terminals in order without waiting for the preceding build to finish.

| Selection    | Action                                                                                 |
|--------------|----------------------------------------------------------------------------------------|
| Engine Admin | Installed admin Tauri CLI `dev --features desktop` in `apps/admin-dashboard`           |
| Editor       | Installed client Tauri CLI `dev` in `apps/editor-host`                                 |
| Game         | `cargo run -p game`                                                                    |
| Vulkan probe | `cargo run -p renderer --example vulkan_probe`                                         |
| Rust checks  | `cargo check --workspace --all-targets`, then `cargo clippy --workspace --all-targets` |

**Cancel remaining** cancels queued launches and leaves opened apps running. A launch failure stops the remaining queue
and preserves earlier terminals for inspection. The whole queue's tools, dependencies and ports are checked before
starting; an operating-system failure during launch can still leave a partially started group.

Select a row under **Running apps** for **Stop app** or **Start / restart app**. Restart stops the old owned tree before
launching a replacement. **Stop all** stops all owned trees. Closing the launcher cancels pending queue entries and
stops its apps, including Angular dev servers. These controls do not adopt or terminate unrelated processes.

The Tauri configurations start their respective Angular dev servers. Requires Windows, Python with Tkinter, Cargo, and Node for desktop apps. Install frontend dependencies with `npm.cmd install` in `apps/editor` and `apps/admin-dashboard`. Graphics execution requires the existing Vulkan prerequisites. No additional Python packages are needed.

## Process ownership and browser previews

`TerminalManager` assigns each child console to a Windows job before a startup handshake permits it to create
descendants. Jobs use kill-on-close and retained native handles, avoiding ownership decisions based on reusable PIDs.
Stopping terminates the job and waits up to three seconds for it to empty; a timeout cancels restart.

Editor uses `http://localhost:4200`; Engine Admin uses `http://127.0.0.1:4301/?preview`. An occupied development port
is an error, even if another engine session owns it. The launcher does not attach to existing servers.
**Open in Chrome** becomes available for an owned app whose loopback server returns its expected `engine-app` HTML
marker. The readiness probe reads at most 64 KiB; it verifies app identity, not all native/runtime functionality.
Chrome opens a new tab in the user's browser session and is not placed in an app job. Closing the launcher leaves
Chrome open. Editor browser previews have no native bridge; the dashboard preview displays sample data.

To open the launcher without an extra parent console, run `pythonw launcher.py`.

The same file works as a tiny CLI:

```powershell
python launcher.py --dry-run
python launcher.py --run dashboard
python launcher.py --run client
python launcher.py --run engine
python launcher.py --run vulkan-probe
python launcher.py --run workspace
python launcher.py --run all
python launcher.py --run check --no-pause
```

Direct actions execute in the current terminal. Rust checks stop on the first failing command. Commands use separate argument lists without shell interpolation, and errors leave output available for inspection.

`workspace` starts Editor and Engine Admin; `all` adds Game. Group commands open managed child terminals and keep the
parent terminal alive; Ctrl+C stops their owned trees. Individual CLI actions use the current terminal and do not create
a GUI ownership session. `--dry-run` prints resolved commands without starting apps.

`dev.ps1` delegates to this file: no argument opens the GUI, and `editor`, `admin`, `game`, `workspace`, `all`,
`vulkan-probe` and `check` map to the corresponding actions. `-DryRun` passes through to the shared resolver. Python
must be available as `python` or `py -3`.

## Launcher verification

```powershell
python -m unittest tests.test_launcher tests.test_launcher_processes -v
```

Tests cover action/group mappings, separate client/admin hosts, missing tools/dependencies, safe argument passing,
dry runs, sequential Rust checks, queue order/cancellation/failures, identity/port checks, console Python selection,
process-tree ownership and stop/restart behavior. UI tests replace app execution with mocks; native ownership tests
use short-lived Python child processes. They do not establish interactive Tauri/GPU or real Chrome acceptance.

Tkinter import/window-creation errors produce actionable errors. When started through `pythonw` without stderr, startup
errors use a Windows error dialog. Queue and process management are separate from rendering so their behavior can be
verified without launching engine apps.
