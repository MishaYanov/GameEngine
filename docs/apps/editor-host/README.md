# Editor desktop host

Source: `apps/editor-host/src-tauri`. Cargo package: `app`; library: `app_lib`. Tauri hosts the [Angular editor](../editor/README.md) and owns `runtime::Engine` behind `Mutex<Engine>`.

## Development and packaging

Install the [prerequisites](../../usage.md) and editor frontend dependencies. From the repository root:

```powershell
.\dev.ps1 editor
cargo check -p app --offline --locked
cargo test -p app --offline --locked
```

For a desktop bundle, from the repository root:

```powershell
Push-Location apps/editor-host
try {
    & ..\editor\node_modules\.bin\tauri.cmd build
} finally {
    Pop-Location
}
```

`tauri.conf.json` starts `npm start` in `apps/editor` for development and `npm run build` for packaging. Development uses port 4200; packaging reads `apps/editor/dist/editor/browser`. The default window is 1600 by 900 with a 1000 by 600 minimum. Installer execution needs manual verification.

## Startup and commands

Startup initializes the `editor-host` logger, constructs the runtime, manages state, and registers two commands. Vulkan startup failure currently reaches `expect` and exits before the UI opens.

| Command | Input | Result |
| --- | --- | --- |
| `engine_status` | None | `initialized`, `renderer`, `gpuName`, `vulkanApi`; state-lock failures return a string error |
| `create_project` | `name`, `destination` strings | `{ path, launched, launchError }`; installation failure rejects the invocation |

Creation executes through `spawn_blocking`. After installation, the host spawns the game with the installed directory as its working directory. `launched` reports spawn success, not rendering health. The command exposes no cancellation or stop/restart interface and does not register the child with the dashboard.

## Extension and verification

Commands live under `src/commands` and must be registered in `src/lib.rs`. Adding a module alone does not expose an invocation. Use bridge DTOs for shared data and keep blocking work off async execution threads. `entity.rs` is currently empty; no entity-editing command is registered.

The application initializes logging; libraries use the existing logger. See [runtime](../../crates/runtime/README.md), [bridge](../../crates/bridge/README.md), and [architecture](../../architecture.md).

Workspace tests compile the host, but it has no Rust command behavioral tests. Manually verify startup, status, invalid destination handling, successful creation, build failures, and launch-error reporting. See [testing](../../testing.md).
