# Engine Pipeline

Reviewed on 2026-10-23 against commit `b8c2301` and staged, unstaged, and untracked changes, including the
controller/gameplay split and scene persistence prototype. Renderer extraction and the five-step runtime move are
complete for their stated scope; timing/input/camera are now in progress in the working tree; initial scene persistence
and the controller/gameplay split are now also in progress; editor integration remains future work.

This roadmap belongs to Engine Developer. Game App Developer remains empty. The target is a small playable game built through reusable engine APIs, with a persistent authoring workflow and a distributable that runs outside the checkout.

## Status definitions

| Status | Meaning |
| --- | --- |
| Implemented | Present in source for the stated scope; not a claim of production readiness or full runtime verification. |
| Partial | Useful implementation exists, but important capabilities or boundaries are incomplete. |
| Planned | No integrated implementation found in the inspected game/runtime/rendering paths. |
| Runtime check pending | Code exists, but live behavior was not exercised in this review. |

No completion percentage is assigned: a rendering demo and an integrated editor have different acceptance criteria.

## What we have built

| System                      | Status                                        | Current capability                                                                                                                  | Remaining work                                                                                                                |
|-----------------------------|-----------------------------------------------|-------------------------------------------------------------------------------------------------------------------------------------|-------------------------------------------------------------------------------------------------------------------------------|
| Window and Vulkan rendering | Implemented                                   | Surface/device selection, swapchain, frame submission, indexed drawing, depth testing, resize handling, shutdown wait.              | User confirms resize/minimize and shutdown checks; repeat after renderer changes.                                             |
| Camera, timing, and input   | Implemented prototype                         | FrameClock/InputState/CameraController and context-aware native routing; Edit/Play controls.                                        | Visible control/capture acceptance, action mapping, fixed-step policy.                                                        |
| glTF/GLB import             | Partial                                       | All primitives of the first mesh, geometry, indices, UVs, base-color image/factor, double-sided setting.                            | Scene nodes and their transforms, other meshes, normals/tangents, broader image/material support.                             |
| Materials                   | Partial                                       | Texture × factor with imported/generated normals and fixed ambient/directional diffuse light; unlit editor draws.                   | Full PBR, alpha modes, normal maps, shadows, editable light data.                                                             |
| Asset caches                | Implemented                                   | Cached GPU meshes/textures and model-path-qualified embedded-image keys.                                                            | Stable asset references, reload/invalidation policy, model-level caching. Repeated loads can still decode the file again.     |
| Model abstraction           | Implemented in runtime                        | Shared Model/ModelPart types; AssetManager imports parts and delegates GPU upload to WindowRenderer.                                | Portable asset handles/backend independence remain future design work.                                                        |
| Model instances             | Implemented in runtime                        | Shared Arc<Model> and independent transforms; consumed by game through runtime APIs.                                                | Initial persistence is in progress; broader gameplay API remains to be defined.                                               |
| Scene and IDs               | Implemented prototype                         | Persistent keys/bindings, snapshots/restoration, mixed history, and generic editor.                                                 | Parent-child hierarchy, global references, full authoring acceptance.                                                         |
| Shared material layout      | Implemented; native checks user-confirmed     | Layout belongs to WindowRenderer; empty-scene pipeline setup does not need a model.                                                 | Repeat native checks after future presentation changes.                                                                       |
| Renderer ownership          | Complete for extraction milestone             | WindowRenderer owns initialization, upload, draw, deferred resize, and shutdown; GameApp owns application behavior.                 | External GPU resource lifetimes still depend on application drop order and idle waits.                                        |
| Shared runtime              | Complete for five-step migration              | Assets, models, instances, scenes, Transform, and draw-request construction are shared runtime APIs; tests and smoke imports moved. | Status-only Engine/Renderer path is still separate; timing/input/camera are in progress; editor viewport remains future work. |
| Diagnostics                 | Implemented                                   | engine_logger, component health, error propagation, validation logging.                                                             | Extend diagnostics as systems arrive and measure actual runtime behavior.                                                     |
| Editor shell                | Native prototype implemented; Angular partial | Native flat hierarchy, properties, gizmos, picking, catalog, dirty state, Edit/Play. Angular still setup/status.                    | Visual acceptance and deciding long-term host integration.                                                                    |
| Packaging                   | Recursive asset-copy fix implemented          | Compiler copies the complete nested asset tree, including robot.glb; a test verifies copied model/texture bytes.                    | Full installed release execution outside checkout remains separate validation.                                                |
| Tests and documentation     | Expanded                                      | Steps 11–20 automated tests, hidden native examples, full-workspace compile, and step report/resources.                             | Visible interaction/appearance, clean-checkout, installed-run, and CI execution evidence.                                     |

Evidence: [game](apps/game/README.md), [renderer](crates/renderer/README.md), [runtime](crates/runtime/README.md), [editor](apps/editor/README.md), [compiler](compiler/README.md), and [testing](testing.md). Source inspected includes apps/game/src/main.rs, its renderer_smoke example, crates/runtime scene/instance/transform/assets modules and public-API tests, crates/renderer/src/window_renderer.rs, and compiler asset copying. The reusable modules are no longer defined in the game binary.

## Steps 11–20 implemented: automated acceptance

The native game now has generic SceneEditor, Edit/Play clone isolation, mixed structural/rename/transform history,
cached bounds/highlight, box ray picking, flat hierarchy/property fields, undoable mouse gizmos, catalog placement,
normals/basic lighting, and conservative frustum culling with frame/update statistics. These are implementation-complete
for steps 11–20 with automated tests and supplemental hidden Vulkan examples; no new visible UI/lighting or
GPU-performance acceptance is claimed.

The [step-by-step change report](editor-steps-11-20.md) maps each step to source files, motivation, focused examples,
resources, controls, and verification. The native hierarchy is a flat list, not parent-child transforms. The authoring
UI is in game, not Angular/Tauri. Structural history now preserves IDs/keys/order/bindings/model sharing; no-op/error
redo is preserved. Play uses temporary authored clones, and F5 saves authored content during Play.

The standard test handoff is 183 passed, zero failed, one ignored. The explicitly enabled ignored initialization test
and both hidden renderer/scene-I/O examples also passed. Debug game build, full-workspace all-target compile, and
focused Clippy passed with existing warnings. See [testing](testing.md) for scope rather than treating these results as
visual acceptance.

## Completed foundation and next work

Renderer ownership extraction is complete: WindowRenderer owns GPU presentation resources, uploads, deferred resize/rebuild, and shutdown. The subsequent five-step sharing sequence is also complete:

| Step | Status | Evidence |
| --- | --- | --- |
| Introduce a game library | Complete as an intermediate structure | No permanent game-library dependency is needed after the move. |
| Move draw-request construction to Scene | Complete | Scene::render_objects builds borrowed requests. |
| Move Transform to runtime | Complete | Game uses runtime::Transform. |
| Move assets/models/instances/scenes | Complete | Public runtime modules and external scene_api tests exist. |
| Validate after moving | Complete, user-confirmed | Tests, native smoke, visible rendering, resize, and minimize passed per user confirmation. |

The subsequent timing/input, persistence, and native authoring sequence has progressed through implemented steps 11–20.
Do not continue to label them wholly planned: native edit/play, mixed history, panels/gizmos, catalog, lighting, and
culling now exist. The remaining next-step roadmap starts beyond 20 and is planning only; no further implementation is
included in this report.

Outstanding gates are visual acceptance of the new tools; generated-project JSON packaging; installed/clean-checkout
evidence; and broader engine systems such as complete glTF scenes, hierarchy parenting, richer materials,
audio/physics/game UI. See the report's explicit limits rather than repeating the old extraction TODOs.

## When to build the UI

The editor shell already exists. Renderer ownership and shared runtime scene boundaries now exist. The first authoring
prototype is implemented in the native game. Decide future Angular/Tauri viewport/transport integration separately; its
current setup/status UI is unchanged.

| Editor stage        | Scope                                                                                                 | Acceptance test                                                                          |
|---------------------|-------------------------------------------------------------------------------------------------------|------------------------------------------------------------------------------------------|
| First useful editor | Native game prototype implements scene list, selection and properties; visual acceptance pending.     | Select either robot and change only its transform immediately in the viewport.           |
| Authoring workflow  | Native prototype implements these features with mixed history; persistence/distribution gates remain. | Edit a scene, undo an edit, restart, and reopen the saved result.                        |
| Play mode           | Native temporary clones and authored save/restore implemented and tested.                             | Gameplay changes do not silently overwrite the scene; stopping restores authoring state. |
| Specialist tools    | Material, animation, audio, script, and particle panels as runtime systems arrive.                    | Each panel edits real engine data, persists it, and previews the corresponding behavior. |

Full lighting, skeletal animation, scripting, and particles are not prerequisites for the first inspector. Reliable ownership, shared APIs, and scene lifecycle are prerequisites. Runtime game UI (menus/HUD) is separate from editor UI.

## Gameplay and content systems

These are future systems, not capabilities already demonstrated by the robots. Their order should follow the first game's needs and the dependencies below.

| System                        | Status                                                        | Prerequisites                                                         | First implementation                                                                                                                | Completion evidence                                                                                                              |
|-------------------------------|---------------------------------------------------------------|-----------------------------------------------------------------------|-------------------------------------------------------------------------------------------------------------------------------------|----------------------------------------------------------------------------------------------------------------------------------|
| Full static-model import      | Partial                                                       | Stable asset/model API.                                               | Traverse glTF nodes, compose hierarchy transforms, import all referenced meshes/materials.                                          | A multi-node/multi-mesh fixture has correct placement and reuses shared meshes.                                                  |
| Lighting and richer materials | Basic directional light implemented; richer materials planned | Normals, material schema, pipeline variants.                          | Directional light, then metallic/roughness, normal maps, supported alpha modes.                                                     | Known assets show correct lighting, cutouts, and supported transparency.                                                         |
| Collision and physics         | Planned                                                       | Shared scene, timing, transform ownership.                            | Colliders, queries, events, movement; rigid bodies as required.                                                                     | Movement collides consistently across frame rates; removed entities leave no colliders.                                          |
| Sound and audio               | Planned                                                       | Asset references, lifecycle, settings, update loop.                   | Device/mixer, sound handles, play/stop/loop, master/music/effects volume; spatial listener/emitters next.                           | One-shots and music play; spatial emitters move; volume works; unloading releases voices safely.                                 |
| Animation                     | Planned; demo rotation only                                   | Node import, timing, transform ownership.                             | Clip/time/channel sampling and interpolation; then skeletons, inverse-bind matrices, skinning, blending.                            | A known clip plays, pauses, loops, and animates two instances independently; skinning follows joints correctly.                  |
| Custom scripts                | Planned                                                       | Stable IDs, gameplay API, timing, lifecycle, logging, persistence.    | Script properties and start/update/destroy callbacks; choose embedded language or native extensions after requirements are defined. | A script responds to input and moves an entity; errors are isolated and logged; stale references cannot access removed entities. |
| Particle system               | Planned                                                       | Timing, camera, assets/materials; blending for translucent particles. | CPU emitters with spawn rate, lifetime, velocity, color/size curves, textured billboards. Defer GPU simulation.                     | Emitters start/stop, respect limits, expire particles, and clean up on scene unload.                                             |
| Runtime game UI               | Planned                                                       | Input routing, viewport sizing, fonts/images.                         | Text, buttons, simple layout, HUD, pause/menu focus.                                                                                | Start/pause/restart work at supported resolutions without leaking menu input into gameplay.                                      |
| Saves and settings            | Planned                                                       | Versioned serialization, game-state API.                              | Separate authored scenes, user settings, and runtime saves.                                                                         | State survives restart; invalid/older data has a defined recovery or migration path.                                             |
| Asset tooling                 | Planned beyond caches                                         | Stable IDs and dependency tracking.                                   | Asset browser, import settings, reload, dependency-aware packaging.                                                                 | Updates reach instances and installed games resolve all required assets.                                                         |
| Performance and scale         | Planned                                                       | Representative scenes and timing measurements.                        | CPU/GPU timing, memory budgets, then culling/batching where justified.                                                              | A named test scene meets recorded frame-time/memory budgets on a named machine.                                                  |

Define the script trust model and allowed engine APIs before selecting an execution mechanism. Define the order in which animation, physics, scripts, and manual edits write transforms so they do not overwrite one another unpredictably.

An ECS, GPU particle simulation, networking, and hot reload are optional extensions until a concrete game or measured constraint requires them.

## Functional engine acceptance gates

Use one small playable game to prove integration. Agree its scope before making every optional feature mandatory.

| Gate | Required evidence |
| --- | --- |
| Reuse | Game rules use engine APIs without copying renderer internals. |
| Playability | Input, an objective, success/failure, and restart work together. |
| Content | The audio, animation, scripts, collision, and effects selected for the game work together. |
| Authoring | The supported workflow saves and reloads scenes predictably. |
| Lifecycle | Scene changes, empty scenes, removal, pause, and shutdown are reliable. |
| Distribution | Clean builds package required assets and run outside the development directory. |
| Diagnostics | Missing assets, import failures, script errors, and graphics errors produce useful logs. |
| Quality | Automated and manual scenarios pass; supported hardware and performance budgets are recorded. |

## Review and verification

This report uses Git baseline b8c2301 and staged/unstaged production/test additions for steps 11–20. HEAD alone is not
the documented source snapshot. Docs/checkpoint work does not stage, commit, or modify the authored scene. The current
large Left Robot scale remains as supplied.

The implementation/tester handoff reports standard 183 passes/one ignored, an additional explicit native-init pass, two
hidden native smoke successes, debug build, full-workspace compile, and focused Clippy with existing warnings. The user
chose automated-only verification; no visible panel/gizmo/lighting or GPU-performance result is inferred. Documentation
build results are recorded in docs-site/docs-state.json. Historical renderer migration confirmations are not used as
proof of new UI appearance.
