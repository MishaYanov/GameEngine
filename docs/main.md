# Game Engine documentation

Start with the [Windows Editor quickstart](windows-editor-quickstart.md) to build the
installer, open an editable project, and export a standalone game. For a development
checkout, follow [setup and usage](usage.md).

## Work in the Editor

- [Editor workspace](editor-workspace.md) explains where to edit objects, assets, controls, and scene settings.
- [Objects, components, and Props](authoring-objects.md) shows how to assemble a scene and reuse objects.
- [Native scene viewport](viewport-implementation.md) covers camera navigation, transform handles, picking, and rendering ownership.
- [Editor command flow](editor-flow.md) follows requests from Angular through the native host to saved projects and game export.

## Develop the engine

- [Workspace architecture](architecture.md) maps the applications, crates, and boundaries.
- [Editor commands](scene-editing.md) describes validation, transactions, selection, and undo.
- [Project lifecycle](project-lifecycle.md) explains the manifest, multiple scenes, save scopes, and packaging.
- [Scene format](scene-persistence.md) describes serialized scene data and migrations.
- [Graphics and assets](graphics-and-assets.md) explains imports, model resources, shaders, and GPU lifetimes.
- [Timing, input, and camera](timing-input-camera.md) explains the fixed step, game input, and camera behavior.
- [Testing and validation](testing.md) explains which tests to run for each subsystem.

## Applications and libraries

- [Angular Editor](apps/editor/README.md) and [desktop Editor host](apps/editor-host/README.md)
- [Native Player](apps/game/README.md) and [Admin Dashboard](apps/admin-dashboard/README.md)
- [Project compiler](compiler/README.md)
- [Bridge](crates/bridge/README.md), [Runtime](crates/runtime/README.md), [Renderer](crates/renderer/README.md), and [Logger](crates/logger/README.md)
- [Documentation site](static-site.md)
