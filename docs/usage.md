# CLI and dashboard usage

This guide covers the Windows development checkout. Run commands in PowerShell. Examples use `D:\Coding\Game Engine` as the repository and `D:\Projects` for generated projects; substitute your own paths.

## 1. First-time setup

Install these tools before building:

- Rust/Cargo, with a toolchain satisfying the workspace packages (Rust 1.89 or newer).
- Node.js and npm compatible with the checked-in Angular 22 dependencies.
- Windows Tauri build prerequisites: Microsoft C++ Build Tools and WebView2.
- Vulkan SDK with `glslc.exe`, plus a Vulkan-capable GPU and driver.
- Python with Tkinter if you want to use `launcher.py`.

The renderer looks for `glslc.exe` in `$env:VULKAN_SDK\Bin`, then on PATH. Open a new terminal after installing tools or changing PATH.

Install both frontend dependency sets:

```powershell
Set-Location 'D:\Coding\Game Engine'
npm.cmd ci --prefix apps/editor
npm.cmd ci --prefix apps/admin-dashboard
```

Check the available tools:

```powershell
cargo --version
node --version
npm.cmd --version
```

## 2. Install the triangle demo with the CLI

The compiler CLI takes two positional arguments:

```text
compiler <project-name> <destination-folder>
```

The destination is an **existing parent folder**. The installer creates a child folder using the project name.

```powershell
Set-Location 'D:\Coding\Game Engine'
New-Item -ItemType Directory -Path 'D:\Projects' -Force
cargo run -p compiler -- 'My Game' 'D:\Projects'
```

This builds the game runtime in release mode and installs it into `D:\Projects\My Game`. A successful command prints:

```text
Project installed at D:\Projects\My Game
```

The installed folder contains:

```text
My Game/
  game.exe
  project-name.txt
  README.txt
  assets/
    textures/
      crate.png
      bricks.jpg
    scenes/
      robots.json
    props/
      robot.glb
```

Launch the installed demo:

```powershell
& 'D:\Projects\My Game\game.exe'
```

The window title uses the project name. The installer copies the complete nested asset tree, including `assets/scenes/robots.json` and `assets/props/robot.glb`; the earlier missing-model omission is fixed. Installed mode draws stationary robot model instances. Preserve executable, name file, and the asset tree together. See [the game guide](apps/game/README.md).

The CLI installs the project; it does not automatically launch it. The first build can take several minutes. Compiler output from the game build is collected and reported if it fails.

Names accept letters, numbers, spaces, hyphens and underscores, with a limit of 80 bytes in the backend. Reserved Windows names such as `CON` and `LPT1` are rejected. The destination must be absolute and already exist. Existing project folders are rejected; use a new name or destination for another installation.

Exit codes are `0` for success, `1` for build/install failure, and `2` for an incorrect number of arguments.

This CLI is a development tool tied to the engine source checkout. Its installed output is a runtime stub; it does not include an independent source workspace or editable scene.

### Development scene controls — in progress

The development game now reads `apps/game/assets/scenes/robots.json`. Tab selects either robot; WASD moves the selected robot; arrows and captured mouse motion control the camera. F5 saves transforms/camera to the active scene and F9 reloads it, discarding unsaved state. Saving modifies the checkout asset file. These commands are skipped in installed/name-file mode. See [scene persistence](scene-persistence.md) before using save/reload.

## 3. Use the editor project setup screen

Start the Angular editor inside its Tauri desktop host:

```powershell
Set-Location 'D:\Coding\Game Engine'
python launcher.py --run client
```

Without the Python launcher, use:

```powershell
.\dev.ps1 editor
```

In the setup screen:

1. Enter a **Project name**, such as `My Game`.
2. Enter an existing **Destination folder**, such as `D:\Projects`.
3. Click **Create & launch project**.
4. Wait for the build and installation. The form disables submission while working.
5. Read the installed path and launch status. The host attempts to open the graphics window after installation. Process-spawn success does not certify that the child subsequently initialized Vulkan or rendered successfully.

This uses the same installer as the CLI, including its destination and overwrite rules. If launch fails after installation, the screen reports that separately; run `game.exe` from the installed folder to retry.

A browser-only editor preview cannot build or launch projects. Use the Tauri desktop application for these actions.

## 4. Launch the admin dashboard

From the repository root:

```powershell
python launcher.py --run dashboard
```

Or launch directly:

```powershell
Set-Location 'D:\Coding\Game Engine\apps\admin-dashboard'
npm.cmd run desktop
```

This starts the Angular development server on port 4300 and opens the Engine Admin desktop window. Keep its terminal open while working. Press Ctrl+C in that terminal to stop the development session.

For the four-button launcher, run `python launcher.py` from the repository root. Select **Run the dashboard**, **Run Angular app in Tauri**, **Run backend graphics engine**, or **Check Rust for issues**. Each button opens its own terminal. Closing the launcher leaves those terminals running; stop them individually with Ctrl+C.

For a read-only dashboard preview:

```powershell
Set-Location 'D:\Coding\Game Engine\apps\admin-dashboard'
npm.cmd start
```

Open `http://127.0.0.1:4300/?preview`. This displays sample data; process controls and job execution require the desktop app.

### Overview: configure and manage applications

Open **Overview → Workspace settings**. Set the engine workspace path and log directory, then click **Save settings**. Defaults point to the development checkout and shared logger directory. On Windows the logger normally uses `%LOCALAPPDATA%\GameEngine\logs`; `ENGINE_LOG_DIR` can override it.

Under **Managed applications**, use **Start**, **Stop**, and **Restart**. If an application says **Build executable first**, build its package in **Builds** using the Development profile. These controls manage applications started by the dashboard. The separate Python launcher terminals have their own lifetime.

**Component health** shows lifecycle and heartbeat information. **Process history → Logs** opens records for a run. Stop applications and finish or cancel active jobs before changing the workspace. Closing the dashboard stops its owned application processes.

### Builds and Tests: queue Cargo jobs

1. Open **Builds** or **Tests**.
2. Choose a package from the dropdown.
3. Choose **Development** or **Release**.
4. Click **Queue build** or **Queue tests**.
5. Follow the status in **Run history**.

Jobs run one at a time. Use **Logs** to inspect diagnostics, **Artifacts** to view output paths, and **Cancel** to stop queued or running work. An interrupted run is recorded and is not automatically resumed. Test status reports the command outcome rather than a structured count of individual tests.

### Logs: inspect a failure

Search messages and filter by severity or application. **More filters** adds component, session, operation and time ranges. Run-history **Logs** links apply the relevant filter automatically.

Select a record to inspect source information, error causes, stack trace and related operation. Use **Copy record**, **Create task**, or **Open source**. Opening source requires a local VS Code installation and a file inside the configured workspace.

**Pause** freezes the displayed records while collection continues. **Resume** refreshes the view. **Export** writes retained matching JSONL records into the admin data directory. History is bounded, so older records can disappear; export records you need to preserve.

### Board: track work and related runs

Create a card with a title, description, priority, component and status. Move it among **Backlog**, **In Progress**, **Blocked**, and **Done** by dragging or using its Status selector. The arrow buttons reorder cards.

Expand **Automation settings** to choose a package/profile or an existing job to link. On a card, expand **Jobs and diagnostics** to queue a **Build** or **Test**, link the selected job, or open diagnostics. Successful jobs provide a suggestion; move the card to Done yourself when the work is complete.

### Project Builder: generate an independent source workspace

The admin builder generates a native Rust game workspace with engine sources and supported assets. It excludes the Angular editor and administration application.

1. Open **Project Builder**.
2. Enter a **Project name**.
3. Set **Source workspace** to the engine checkout, or use the configured workspace default.
4. Set **New destination folder** to the full path of a new project folder outside the source workspace, such as `D:\Projects\IndependentGame`. Its parent (`D:\Projects`) must already exist; the destination itself must not exist.
5. Choose Development or Release. The target is the current local platform.
6. Leave **Use cached Cargo dependencies (offline)** checked if dependencies are cached. Clear it to allow dependency downloads.
7. Click **Preview files** and inspect included files, exclusions and destination.
8. Click **Generate and validate**.
9. Wait for **SUCCEEDED** in Generation history. Use **Validation logs** to inspect failures, or **Cancel** while validation is running.

Source changes invalidate the preview; preview again before generating. Validation happens in staging, and only a successful run publishes the destination. Failed, cancelled or interrupted staging is retained for inspection; its path appears in history.

Run the generated game's package from its new workspace:

```powershell
Set-Location 'D:\Projects\IndependentGame'
cargo run -p game
# For a release build:
cargo run -p game --release
```

Keep the generated workspace layout and read its generated README for details. This source-generation workflow is separate from the CLI/editor runtime installation:

| Workflow | Destination field | Result |
| --- | --- | --- |
| Compiler CLI / editor setup | Existing parent folder, e.g. `D:\Projects` | Installed runtime and complete asset tree in a named child |
| Admin Project Builder | Full new folder path, e.g. `D:\Projects\IndependentGame` | Independent native source workspace, validated before publication |

## 5. Common problems

| Problem | Action |
| --- | --- |
| Cargo, Node or Python is unavailable | Install the required tool, check PATH and reopen the terminal. |
| Launcher reports missing frontend dependencies | Run the corresponding `npm.cmd ci --prefix ...` command from first-time setup. |
| Shader build cannot execute `glslc` | Install the Vulkan SDK; set `VULKAN_SDK` to its installation folder or add its `Bin` directory to PATH. |
| Destination validation fails | Use an absolute path and follow the parent-folder versus full-folder rules above. |
| Project folder already exists | Choose a fresh project name/path; neither workflow overwrites an existing project. |
| Offline builder validation fails on dependencies | Clear the offline checkbox and retry with network access. |
| Preview was invalidated | Click Preview files again after changing fields or source files. |
| Dashboard controls are disabled in a browser | Start the desktop app with `npm.cmd run desktop`. |
| Managed application executable is missing | Queue a Development build for its package, then return to Overview. |
| Game exits or cannot initialize Vulkan | Check console/dashboard logs and GPU driver support; confirm assets remain alongside the installed runtime. |
| Frontend development port is occupied | Stop the earlier development session; editor uses 4200 and admin dashboard uses 4300. |

Additional references: [launcher details](../designs/terminal-launcher.md), [installation pipeline](../designs/project-installation.md), and [admin dashboard technical notes](apps/admin-dashboard/README.md).
