# Build the Windows Editor and export your first game

Build the Windows installer, install the Editor, then make and play your own copy of **Cinder Oath**. The exercise
changes the arena lighting and exports a standalone game you can share.

## 1. Build the installer

On the build PC you need x64 Windows, Rust and Cargo with the `x86_64-pc-windows-msvc` toolchain, Microsoft C++ Build
Tools and the Windows SDK, Node.js and npm, and the Vulkan SDK with `glslc.exe`. The renderer finds `glslc.exe` under
`$env:VULKAN_SDK\Bin` or on `PATH`. [First-time setup](usage.md#1-first-time-setup)

Run these commands in PowerShell from the engine checkout. Change the paths if your checkout or build folder lives
elsewhere.

```powershell
Set-Location 'D:\Coding\Game Engine'

npm.cmd ci --prefix apps/editor
if ($LASTEXITCODE -ne 0) { throw 'Editor dependency install failed.' }

npm.cmd ci --prefix apps/admin-dashboard
if ($LASTEXITCODE -ne 0) { throw 'Dashboard dependency install failed.' }

$out = "D:\Builds\Game Engine\$(Get-Date -Format 'yyyyMMdd-HHmmss')"
.\apps\editor-host\build-windows-release.ps1 -OutputDirectory $out
```

The script prints the installer path and the portable Editor folder. It builds the Editor and Player from the same
checkout and includes the editable Cinder Oath sample. The optional Admin Dashboard is also inside the installer. The
example puts the deliverables outside Cargo’s `target` folder so `cargo clean` leaves them available.

| Output                            | Use it to                                                                                             |
|-----------------------------------|-------------------------------------------------------------------------------------------------------|
| `Game Engine_0.1.0_x64-setup.exe` | Install the Editor. The filename follows the version in `apps/editor-host/src-tauri/tauri.conf.json`. |
| `Editor\game-engine-editor.exe`   | Run the portable Editor. Keep the entire `Editor` folder, including its `release` subfolder.          |

For just the portable folder, add `-PortableOnly` to the build command and install only the Editor’s npm
dependencies. [Release packaging details](apps/editor-host/README.md#development-and-packaging)

## 2. Install and open the Editor

1. In File Explorer, open the output folder from step 1 and double-click the `*_x64-setup.exe` file. Use a setup file
   from a trusted build. Local builds without a signing configuration produce an unsigned installer.
2. Follow the setup prompts. On a fresh interactive install, choose **Yes** at the Admin Dashboard prompt to add logs,
   performance metrics, and a task board. Choose **No** to install the Editor only.
3. Open **Game Engine** from the Start menu. If you installed the optional Dashboard, it has its own **Game Engine Admin
   Dashboard** shortcut.

The installed Editor needs WebView2 and a Vulkan-capable graphics driver. Setup can download WebView2 if it is missing.
Creating and exporting a project from the installed Editor works without Rust, Node.js, or the source
checkout. [Editor desktop host](apps/editor-host/README.md) · [Admin Dashboard](apps/admin-dashboard/README.md)

## 3. Make your own copy of Cinder Oath

1. Create a folder for editable projects, such as `D:\Projects`. In the Editor, choose **File → Open Sample…**. If you
   are on **Create your project**, select **Cinder Oath sample** in **Start with**.
2. Set **Project name** to `Vault Trial`. Use **Browse…** to choose `D:\Projects` as the **Destination folder**, then
   click **Create project**. The Editor creates and opens `D:\Projects\Vault Trial\project.json`. If the project was
   created but did not open, use **Open in Editor**.
3. Open **Game → Scene Information…**, select `scenes/vault.json` in **Scene**, then close the dialog. This scene
   contains the arena.
4. Choose **Game → Scene Lighting…**. Select **Cold overhead fill (Directional)**, set **Intensity** to `0.9`, and click
   **Apply**. Close the dialog and click **Save All** in the workspace toolbar.
5. To preview the whole game, return to **Game → Scene Information…** and select `scenes/oath.json`. Click **Play** and
   click inside the **Game** view to give it keyboard focus. Watch the introduction or press **Enter** to skip it. Use
   **W**, **A**, **S**, and **D** to move and **F** to attack. Close in on the Warden and land three hits to win. Click
   **Stop** when you are done.

Editor Play starts from the scene you have open. The exported game starts from the project’s startup scene, which is the
introduction in this sample. To reopen the editable version later, choose **File → Open Project…** and select
`D:\Projects\Vault Trial\project.json`. [Scenes and saves](project-lifecycle.md#documents-switching-and-history) · [Editor authoring](scene-editing.md)

## 4. Export and play the standalone game

1. Create a separate output folder, such as `D:\Game Builds`. In the Editor, stop Play and click **Save All**. Apply or
   close any pending editor dialogs before exporting.
2. Click **Export Game…** in the workspace controls and select `D:\Game Builds`. The Editor creates
   `D:\Game Builds\Vault Trial`. If that folder already exists, choose a different output parent for this build.
3. Click **Run Exported Game**, or exit the Editor and double-click `D:\Game Builds\Vault Trial\game.exe`. Reaching the
   arena from the introduction confirms the standalone game can load both scenes.
4. To share the build, zip the whole `Vault Trial` export folder. Keep `game.exe`, `project.json`, and `assets`
   together. Share the Editor installer separately if the recipient also needs to edit projects.

The export contains the saved project and the same Windows Player that ships with the Editor. Use the project under
`D:\Projects` for further edits. [Export behavior](project-lifecycle.md#build-and-playerheadless-behavior)

## Start from a blank project

1. Choose **File → New Project…** and leave **Start with** on **Blank project**. The new project opens its Main scene
   with a Game Camera and a directional light.
2. Choose **Objects → Plane**. Select it in **Hierarchy**, open **Inspector → Add Component**, and choose
   **Collider**. A Plane starts with a thin Box collider. Resize the Plane from **Transform** to make a floor.
3. Choose **Objects → Capsule** to add a capsule-shaped mesh. Use **Add Component → Collider** for its collision
   shape. Use **Objects → Player Controller** if the project needs a controllable player object, then assign the local
   player through **Inspector → Scene roles**. Use **Controls** to edit the project's input layouts.
4. Choose **New Folder…** in Hierarchy. Click and hold an object's row, then drop it on the folder row. Open the
   object's Inspector card to edit its binding or launch its dedicated workspace. For example, choose **Open
   Animation** on an Animation card to open the bound clip.
5. To reuse a selection, open its Hierarchy context menu and choose **Create Prop from Selection…**. Choose the
   resulting asset in **Assets** and use **Place Linked Prop…** or **Place as Copy**. Open the asset in **Prop
   Workspace** to edit its source with its own undo history, and use **Show in Assets** to find the source again.
6. Use **Game → Scene Information…** for the scene camera, environment and other scene settings, and **Game →
   Scene Lighting…** for lights. Use **Project** for physics layers and input layouts. Choose **Save All** and **Play**
   to check the result.

You can import your own models through **Objects → Models… → Import GLB…**. Confirm the asset operation, then select
the model and choose **Add to Scene**. Select the object in Hierarchy and choose **Frame Selection** to position the
viewport around it. [Editor workspace and authoring tools](editor-workspace.md) · [Scene editing](scene-editing.md)
