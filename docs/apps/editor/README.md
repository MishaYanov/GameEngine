# Editor frontend

Source: `apps/editor`. This Angular application provides project setup and GPU information through the [Tauri host](../editor-host/README.md). See [setup](../../usage.md) for prerequisites.

## Run

From the repository root:

```powershell
npm.cmd ci --prefix apps/editor
.\dev.ps1 editor
# Alternative:
python launcher.py --run client
```

The host starts Angular on port 4200 and opens the desktop window. Rust/Cargo, Windows Tauri prerequisites, Vulkan 1.3 support, and the SDK are required. Vulkan initializes before the host shows its window.

For frontend layout work, run `npm.cmd start` from `apps/editor` and open `http://localhost:4200/`. Browser mode disables creation because native commands require Tauri.

## Current workflow and contracts

Enter a project name and existing absolute destination parent. **Create & launch project** builds a release game, installs a new named child folder, and attempts to launch it. Submission is disabled during the operation; failures preserve form values. Installation success and launch failure are reported separately.

The form still describes a triangle starter, but the game now loads `props/robot.glb`. The installer omits that model, so installation/spawn success can be followed by a missing-model exit. See the [game guide](../game/README.md). Output contains an executable/assets, not editable scenes or independent sources. Use admin Project Builder for source generation. See [usage](../../usage.md).

- `src/app/app.ts` and `app.html`: validation, busy/error/result state, browser detection, and UI.
- `src/app/core/engine/engine.service.ts`: invokes `engine_status`.
- `src/app/core/engine/engine.models.ts`: frontend status type.
- `src/app/app.spec.ts`: frontend behavioral tests.

`engine_status` returns `initialized`, `renderer`, `gpuName`, and `vulkanApi`. `create_project` accepts `{ name, destination }` and returns `{ path, launched, launchError }`. Keep models aligned with [host](../editor-host/README.md) and [bridge](../../crates/bridge/README.md) contracts.

Frontend validation limits names to 80 characters. Backend validation uses 80 UTF-8 bytes and also checks reserved names, paths, and collisions; it is authoritative for multibyte names.

## Build and verify

From `apps/editor`:

```powershell
npm.cmd run build
npm.cmd test -- --watch=false
npm.cmd run ng -- generate component component-name
```

The production frontend goes to `dist/editor/browser`. See [host packaging](../editor-host/README.md). There is no configured end-to-end target in `angular.json`; desktop creation/launch needs manual checks. See [testing](../../testing.md).

Scene editing, entity manipulation, and an embedded viewport are not implemented. If controls are disabled, use desktop mode. If port 4200 is occupied, stop the old development session. If the host exits at startup, check terminal/shared logs, Vulkan support, and debug validation layers. Frontend status errors are suppressed; host initialization errors can prevent any window from opening. A spawned game can fail later, so inspect its logs separately.
