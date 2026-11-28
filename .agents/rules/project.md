# Game Engine project workflow

Paths in this document are relative to the repository root. This is the full Rust engine workspace with Angular
applications, a Tauri editor host, a native player, compiler/export tooling, and maintained engine documentation.
Recheck versions, capabilities, commands, and APIs against current manifests and source.

## Existing guidance

Follow the [documentation maintenance instructions](../../AGENTS.md#documentation-maintenance).
For editor code, follow the [editor instructions](../../apps/editor/AGENTS.md).

Read an applicable skill when its workflow helps the requested task. Preserve unrelated staged, unstaged, and untracked
work. Inspect the relevant source and Git diff before editing. Use scoped `rg` searches and exclude dependency folders,
caches, temporary artifacts, and build output.

Complete the requested functional slice and relevant verification. Reuse existing typed commands, services, engine
capabilities, shared UI, styles, and the shared logger. Keep game-specific rules in the gameplay module rather than
moving them into reusable engine infrastructure.

Resolve routine reversible choices using the current implementation and the user's preferences. Ask when missing
information materially affects correctness or authorization. These rules do not authorize committing, publishing,
sending messages, or unrelated maintenance.

## Source map

| Area                                 | Entry points                                                                                                  | Ownership                                                                                      |
|--------------------------------------|---------------------------------------------------------------------------------------------------------------|------------------------------------------------------------------------------------------------|
| Editor contracts and bridge          | `apps/editor/src/app/core/editor/editor.models.ts`, `apps/editor/src/app/core/editor/editor.service.ts`       | Typed commands/capabilities, frontend command serialization, snapshots, revisions, and errors  |
| UI edit lifetimes                    | `apps/editor/src/app/modules/editor/workspace/scene-editing.service.ts`                                       | Temporary drafts, previews, acceptance, cancellation, and teardown                             |
| Viewport client and panels           | `apps/editor/src/app/core/editor/viewport-editor.service.ts`, `apps/editor/src/app/modules/editor/workspace/` | Frontend input, panel state, and viewport attachment lifetimes                                 |
| Native editor host                   | `apps/editor-host/src-tauri/src/`                                                                             | Host commands, workspace ownership, native viewport and interaction                            |
| Engine data and simulation           | `crates/engine-core/src/`                                                                                     | Authored data, command execution, simulation, runtime sessions, and typed progress foundations |
| Runtime, bridge, and renderer        | `crates/runtime/src/`, `crates/bridge/src/`, `crates/renderer/src/`                                           | Runtime behavior, integration contracts/assets, and native rendering                           |
| Shared game module and progress      | `crates/cinder-oath-gameplay/src/lib.rs`                                                                       | Cinder Oath gameplay factory, rules, and progress                                               |
| Native game and sample configuration | `apps/game/src/main.rs`, `apps/game/project.json`                                                             | Application integration, project module selection, player behavior, and restart                |
| Compiler/export                      | `compiler/src/lib.rs`                                                                                         | Validated creation, asset staging, and player export                                           |
| Shared logging                       | `crates/logger/src/lib.rs`, `crates/logger/src/storage.rs`                                                    | Logger configuration, context, background JSONL persistence, rotation, and error sink          |
| Shared editor UI and styles          | `apps/editor/src/app/shared/components/`, `apps/editor/src/app/styles/`                                       | Reusable controls, themes, spacing, layout, and motion                                         |
| Design guide and component tooling   | `apps/editor/projects/design-guide/`, `apps/editor/projects/editor-schematics/`                               | UI previews, generated component registry, and component/panel generators                      |

For authoring and viewport work, read the relevant chapters in `designs/editor-viewport/`, especially
`game-creation-workflows.md`, `scene-view-interactions.md`, and `runtime-and-rendering.md`. Treat designs and teaching
guides as guidance; confirm implementation claims against current code and tests.

## State and integration boundaries

Authored scene documents, editor-session selection/navigation, temporary gesture previews, runtime simulation, and typed
game progress have different lifetimes. Keep these boundaries explicit. Play must not silently overwrite authored state.
Editor camera navigation and explicit game-camera authoring are distinct actions.

Preserve the existing command queue, revision/conflict handling, native ownership, and cleanup lifetimes. Verify both
editor and standalone player use the same gameplay module and progress/restart policy before claiming parity. Browser
mocks establish frontend behavior; native rendering and export claims need the corresponding native checks.

The shared logger already supports `ENGINE_LOG_MODE=regular|debug`, `ENGINE_LOG_DIR`, `ENGINE_LOG_CONSOLE`, and
session-local `errors.jsonl`. Inspect its current implementation before changing logging; do not introduce a parallel
logger. Application entry points own initialization and guard lifetime; library code emits through the shared facade.

## Development commands

Run launcher and Rust commands at the repository root:

| Command                                          | Purpose                                                                                    |
|--------------------------------------------------|--------------------------------------------------------------------------------------------|
| `.\dev.ps1 editor`                               | Launch the editor through the shared Python launcher                                       |
| `.\dev.ps1 admin` / `.\dev.ps1 game`             | Launch the dashboard or native game                                                        |
| `.\dev.ps1 check`                                | Run the launcher's check workflow                                                          |
| `.\dev.ps1 editor -DryRun`                       | Inspect launch commands without starting the application                                   |
| `cargo test -p engine-logger --offline --locked` | Check shared logging changes                                                               |
| `cargo test -p engine-core --offline --locked`   | Check core behavior; use a focused test filter when appropriate                            |
| `python test_all.py`                             | Run Rust, Rustdoc, both Angular suites, and Python tooling tests                           |
| `python test_all.py --native`                    | Also run opt-in native/Vulkan checks; requires the native toolchain and Vulkan environment |

Choose other affected Rust packages from current `Cargo.toml` files, including `runtime`, `renderer`, `bridge`,
`compiler`, `cinder-oath-gameplay`, and `game`.

Run editor npm commands in `apps/editor/`:

| Command                                                                          | Purpose                                                          |
|----------------------------------------------------------------------------------|------------------------------------------------------------------|
| `npm run start:dev`                                                              | Run the development editor and registry watcher                  |
| `npm run start:design`                                                           | Run the design guide; opens a browser                            |
| `npm run build:dev` / `npm run build:production`                                 | Build the development or production editor                       |
| `npm run check:production`                                                       | Build production and check development/design bundle exclusions  |
| `npm test -- --watch=false`                                                      | Run Angular tests once; use supported filters for focused checks |
| `npm run test:design-registry` / `npm run test:schematics`                       | Test registry generation or component/panel generators           |
| `npm run generate:ui -- <kebab-name>` / `npm run generate:panel -- <kebab-name>` | Generate a shared component or workspace panel                   |
| `npm run generate:design`                                                        | Regenerate the design component registry                         |

Do not hand-edit `apps/editor/projects/design-guide/src/component-registry.generated.ts`. Use custom previews under
`apps/editor/projects/design-guide/src/previews/` when a specific shared-component export needs a preview.

## Verification and reporting

Match checks to the changed behavior: focused Rust/Angular tests for logic and lifetimes, generator tests for tooling,
production checks for routing/bundle boundaries, and native checks for rendering, host integration, or exported-player
behavior. Use the aggregate runner when the change crosses the relevant packages. Avoid redundant broad reruns once the
required checks pass without a new reason.

Inspect visual changes in the editor or design guide when available, including keyboard access, focus, contrast, and
cancellation. State which checks ran and any concrete limits. Keep performance claims tied to a measured fixture and
environment.

Agent-rule and skill packaging changes need link/path and skill-structure checks. Engine documentation updates are a
separate workflow governed by root `AGENTS.md`: explicit user request, Git/tree inspection, documentation checkpoint,
static site rebuild/checks, and recorded verification limits. Do not advance `docs-site/docs-state.json` merely because
agent instructions changed.
