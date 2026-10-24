# Game Engine documentation

Start with [setup and usage](usage.md), then read [architecture](architecture.md). Project guides follow the source layout under `apps`, `crates`, and `compiler`. Run commands from the directory stated in each guide.

## Workspace and development

- [PROJECT3 backend handoff](project3-handoff.md): Authored project-to-player workflow, steps 36–45 foundations, limits,
  and a copy-ready teaching-first prompt for proposed steps 46–55.

- [Angular editor and engine foundations: steps 21–35](editor-steps-21-35.md): Typed integration without UI composition,
  camera/snapping/group history, hierarchy/schema3/prefabs, static import, materials/lights, simulation/collision, GPU
  timestamps, instancing, examples and resources.

- [Editor and rendering changes: steps 11–20](editor-steps-11-20.md): Responsibility, reasoning, exact file map,
  controls, examples, resources, and automated/native evidence for the native editing prototype.

- [CLI and dashboard usage](usage.md): Prerequisites, setup, installation, editor/launcher startup, dashboard workflows, and troubleshooting.
- [Workspace architecture](architecture.md): Project responsibilities, dependencies, command flow, rendering ownership, installation, and source generation.
- [Graphics and assets](graphics-and-assets.md): Caches, shader contracts, GPU lifetimes, adding assets, glTF/GLB import, and material limitations.
- [Testing and validation](testing.md): Aggregate/focused checks, coverage, build limitations, and manual desktop/GPU validation.

- [Static documentation site](static-site.md): Build, preview, update, test, and host the Markdown-based website.

- [Timing, input, and camera — in progress](timing-input-camera.md): Current clock/key/camera contracts, provisional game controls, and remaining verification.

- [Scene data, save, and reload — in progress](scene-persistence.md): Schema3, older-version migration, persistent keys
  and hierarchy,
  authored persistence, prefabs, validation, and packaging limits.

- [Scene editing](scene-editing.md): Typed editor commands, Edit/Play, selection, transactions, hierarchy, assets, and
  legacy demo limits.

## Applications

- [Editor frontend](apps/editor/README.md): Setup screen, typed editor service/models, viewport directive,
  desktop/browser limits, and builds.
- [Editor desktop host](apps/editor-host/README.md): Tauri configuration, engine state, native commands, launch, and packaging.
- [Native game demo](apps/game/README.md): Multi-primitive GLB models, scene instances, development/installed modes, packaging limits, and renderer lifecycle.
- [Engine Admin](apps/admin-dashboard/README.md): Logs, processes, jobs, board, source generation, storage limits, and verification.

## Tools

- [Starter compiler and installer](compiler/README.md): CLI/library contracts, validation, release build/install behavior, and limits.

## Shared crates

- [Bridge](crates/bridge/README.md): Status and typed editor DTOs, protocol/schema versions, consumers, and contract
  changes.
- [Logger](crates/logger/README.md): Diagnostics, records, configuration, storage, redaction, panic/shutdown behavior, and measurements.
- [Renderer](crates/renderer/README.md): Vulkan requirements, probe, APIs, shader builds, ownership, and verification.
- [Runtime](crates/runtime/README.md): Shared scenes, models, assets, transforms, status engine, and verification.

## Existing design and tool references

These documents remain in `designs`. Requirements can describe future work; consult project guides and implementation status for current behavior.

- [Automation and logging PRD](../designs/automation-and-logging-prd.md): Requirements, delivery steps, status, and future decisions.
- [Project installation pipeline](../designs/project-installation.md): Editor/compiler installation sequence and starter behavior.
- [Terminal launcher](../designs/terminal-launcher.md): Launcher actions, terminal/process lifetimes, and verification.
- [Repository test notes](../designs/testing.md): Original aggregate-runner instructions and coverage limits.
