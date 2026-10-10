# Game Engine documentation

Start with [setup and usage](usage.md), then read [architecture](architecture.md). Project guides follow the source layout under `apps`, `crates`, and `compiler`. Run commands from the directory stated in each guide.

## Workspace and development

- [CLI and dashboard usage](usage.md): Prerequisites, setup, installation, editor/launcher startup, dashboard workflows, and troubleshooting.
- [Workspace architecture](architecture.md): Project responsibilities, dependencies, command flow, rendering ownership, installation, and source generation.
- [Graphics and assets](graphics-and-assets.md): Caches, shader contracts, GPU lifetimes, adding assets, and model-import limitations.
- [Testing and validation](testing.md): Aggregate/focused checks, coverage, build limitations, and manual desktop/GPU validation.

- [Static documentation site](static-site.md): Build, preview, update, test, and host the Markdown-based website.

## Applications

- [Editor frontend](apps/editor/README.md): Setup UI, desktop/browser modes, contracts, source organization, builds, and tests.
- [Editor desktop host](apps/editor-host/README.md): Tauri configuration, engine state, native commands, launch, and packaging.
- [Native game demo](apps/game/README.md): Cube/triangle modes, assets, frame/resize lifecycle, and extension points.
- [Engine Admin](apps/admin-dashboard/README.md): Logs, processes, jobs, board, source generation, storage limits, and verification.

## Tools

- [Starter compiler and installer](compiler/README.md): CLI/library contracts, validation, release build/install behavior, and limits.

## Shared crates

- [Bridge](crates/bridge/README.md): Status fields, JSON names, consumers, and contract changes.
- [Logger](crates/logger/README.md): Diagnostics, records, configuration, storage, redaction, panic/shutdown behavior, and measurements.
- [Renderer](crates/renderer/README.md): Vulkan requirements, probe, APIs, shader builds, ownership, and verification.
- [Runtime](crates/runtime/README.md): Engine construction/status, errors, ownership, and current scope.

## Existing design and tool references

These documents remain in `designs`. Requirements can describe future work; consult project guides and implementation status for current behavior.

- [Automation and logging PRD](../designs/automation-and-logging-prd.md): Requirements, delivery steps, status, and future decisions.
- [Project installation pipeline](../designs/project-installation.md): Editor/compiler installation sequence and starter behavior.
- [Terminal launcher](../designs/terminal-launcher.md): Launcher actions, terminal/process lifetimes, and verification.
- [Repository test notes](../designs/testing.md): Original aggregate-runner instructions and coverage limits.
