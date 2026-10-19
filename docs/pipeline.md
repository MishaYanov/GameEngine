# Engine Pipeline

Reviewed on 2026-10-10 against commit `978f6e4` and the working tree. The engine is a rendering and scene prototype moving toward a reusable runtime. It is not yet a complete game engine or an integrated scene editor.

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

| System | Status | Current capability | Remaining work |
| --- | --- | --- | --- |
| Window and Vulkan rendering | Implemented | Surface/device selection, swapchain, frame submission, indexed drawing, depth testing, resize handling, shutdown wait. | Repeated resize, minimize/restore, and shutdown verification with validation enabled. |
| Camera and transforms | Partial | Perspective camera; per-instance position, Euler rotation, and scale. | Camera controls, input mapping, reusable timing and update API. |
| glTF/GLB import | Partial | All primitives of the first mesh, geometry, indices, UVs, base-color image/factor, double-sided setting. | Scene nodes and their transforms, other meshes, normals/tangents, broader image/material support. |
| Materials | Partial | Unlit texture multiplied by color factor, white fallback, single/double-sided pipelines. | Lighting, alpha modes, normal maps, metallic/roughness. This is not full PBR. |
| Asset caches | Implemented | Cached GPU meshes/textures and model-path-qualified embedded-image keys. | Stable asset references, reload/invalidation policy, model-level caching. Repeated loads can still decode the file again. |
| Model abstraction | Implemented | Model groups mesh/material parts; AssetManager imports and uploads them. | Move reusable types out of the game binary and define the boundary between portable data and Vulkan resources. |
| Model instances | Implemented | Multiple instances share one Arc<Model> but have independent transforms. | Reusable gameplay API and persistence. |
| Scene and IDs | Implemented | Flat scene with spawn, lookup, removal, iteration, and monotonically allocated entity IDs. | Lifecycle tests, serialized-reference rules, hierarchy when needed. There is no ECS yet. |
| Shared material layout | Implemented; runtime check pending | Textures share a layout; pipeline creation/recreation no longer needs a scene object. | Remove every object and resize an empty scene on a live Vulkan device. |
| Renderer ownership | Partial | Resource wrappers exist; GameApp still owns individual Vulkan resources and recreation logic. | Extract initialization, rendering, resize, upload access, and shutdown into a renderer owner. |
| Shared runtime | Partial | crates/runtime initializes a renderer and exposes status. | It does not own the game's scene/update/render loop. Reconcile existing Engine/Renderer with the extraction instead of duplicating lifecycles. |
| Diagnostics | Implemented | engine_logger, component health, error propagation, validation logging. | Extend diagnostics as systems arrive and measure actual runtime behavior. |
| Editor shell | Partial | Angular/Tauri project setup and GPU status UI. | No integrated scene viewport, entity inspector, or scene-editing workflow. |
| Packaging | Partial; known gap | Compiler installs the executable and selected textures. | It omits robot.glb required by startup. Package dependencies and verify outside the checkout. |
| Tests and documentation | Partial | Loader/renderer unit tests, logger tests, frontend/site checks, generated docs. | Native lifecycle, clean-build, packaged-run, and broader integration verification. |

Evidence: [game](apps/game/README.md), [renderer](crates/renderer/README.md), [runtime](crates/runtime/README.md), [editor](apps/editor/README.md), [compiler](compiler/README.md), and [testing](testing.md). Source inspected includes apps/game/src/main.rs, scene.rs, model_instance.rs, transform.rs, the assets module, crates/renderer/src/vulkan, crates/runtime/src/engine.rs, and compiler/src/lib.rs. Older subsystem guides may lag this roadmap; pending updates are recorded in the documentation checkpoint.

## Immediate sequence: complete the foundation

| Order | Work | Why it comes next | Done when |
| --- | --- | --- | --- |
| 1 | Verify scene/resource lifecycle | Ownership must be reliable before extraction. | Two robots, single-object removal, empty scene, resize/minimize/restore, and shutdown work without validation errors or stale entity access. |
| 2 | Extract a Vulkan renderer owner | GameApp mixes application behavior with low-level rendering. | GameApp requests draw/resize through a small API; renderer owns GPU resources and cleanup. Asset upload access is explicit rather than unrestricted public internals. |
| 3 | Establish shared runtime APIs | Game and editor need the same engine behavior. | Scene/transform/asset interfaces live in library crates without a game-binary dependency; existing runtime/status initialization is reconciled. A second graphics backend is not required. |
| 4 | Add timing, input, and gameplay boundaries | Elapsed-time demo rotation is not a gameplay loop. | Input actions and delta time control an object/camera; fixed-step updates exist where needed; focus loss clears held input. |
| 5 | Save and load scenes | Authoring needs persistent data. | Versioned scenes store asset references and transforms, restore entity references deliberately, and report missing assets. Never serialize Vulkan handles, pointers, or Arc values. |
| 6 | Integrate the editor | UI should call stable engine APIs. | Shared scene renders in a viewport; commands select/edit entities; reload and empty-scene lifecycle are verified. |

Fix the packaging omission alongside these steps, before treating installed builds as usable.

## When to build the UI

The editor shell already exists. First establish renderer ownership and shared runtime boundaries, then inspect the Angular/Tauri host to choose viewport presentation and edit-command transport.

| Editor stage | Scope | Acceptance test |
| --- | --- | --- |
| First useful editor | Viewport, scene list, selection, transform inspector. | Select either robot and change only its transform immediately in the viewport. |
| Authoring workflow | Create/delete, asset selection, save/load, undo/redo. | Edit a scene, undo an edit, restart, and reopen the saved result. |
| Play mode | Runtime start/stop with authored data separate from play state. | Gameplay changes do not silently overwrite the scene; stopping restores authoring state. |
| Specialist tools | Material, animation, audio, script, and particle panels as runtime systems arrive. | Each panel edits real engine data, persists it, and previews the corresponding behavior. |

Full lighting, skeletal animation, scripting, and particles are not prerequisites for the first inspector. Reliable ownership, shared APIs, and scene lifecycle are prerequisites. Runtime game UI (menus/HUD) is separate from editor UI.

## Gameplay and content systems

These are future systems, not capabilities already demonstrated by the robots. Their order should follow the first game's needs and the dependencies below.

| System | Status | Prerequisites | First implementation | Completion evidence |
| --- | --- | --- | --- | --- |
| Full static-model import | Partial | Stable asset/model API. | Traverse glTF nodes, compose hierarchy transforms, import all referenced meshes/materials. | A multi-node/multi-mesh fixture has correct placement and reuses shared meshes. |
| Lighting and richer materials | Planned beyond base color | Normals, material schema, pipeline variants. | Directional light, then metallic/roughness, normal maps, supported alpha modes. | Known assets show correct lighting, cutouts, and supported transparency. |
| Collision and physics | Planned | Shared scene, timing, transform ownership. | Colliders, queries, events, movement; rigid bodies as required. | Movement collides consistently across frame rates; removed entities leave no colliders. |
| Sound and audio | Planned | Asset references, lifecycle, settings, update loop. | Device/mixer, sound handles, play/stop/loop, master/music/effects volume; spatial listener/emitters next. | One-shots and music play; spatial emitters move; volume works; unloading releases voices safely. |
| Animation | Planned; demo rotation only | Node import, timing, transform ownership. | Clip/time/channel sampling and interpolation; then skeletons, inverse-bind matrices, skinning, blending. | A known clip plays, pauses, loops, and animates two instances independently; skinning follows joints correctly. |
| Custom scripts | Planned | Stable IDs, gameplay API, timing, lifecycle, logging, persistence. | Script properties and start/update/destroy callbacks; choose embedded language or native extensions after requirements are defined. | A script responds to input and moves an entity; errors are isolated and logged; stale references cannot access removed entities. |
| Particle system | Planned | Timing, camera, assets/materials; blending for translucent particles. | CPU emitters with spawn rate, lifetime, velocity, color/size curves, textured billboards. Defer GPU simulation. | Emitters start/stop, respect limits, expire particles, and clean up on scene unload. |
| Runtime game UI | Planned | Input routing, viewport sizing, fonts/images. | Text, buttons, simple layout, HUD, pause/menu focus. | Start/pause/restart work at supported resolutions without leaking menu input into gameplay. |
| Saves and settings | Planned | Versioned serialization, game-state API. | Separate authored scenes, user settings, and runtime saves. | State survives restart; invalid/older data has a defined recovery or migration path. |
| Asset tooling | Planned beyond caches | Stable IDs and dependency tracking. | Asset browser, import settings, reload, dependency-aware packaging. | Updates reach instances and installed games resolve all required assets. |
| Performance and scale | Planned | Representative scenes and timing measurements. | CPU/GPU timing, memory budgets, then culling/batching where justified. | A named test scene meets recorded frame-time/memory budgets on a named machine. |

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

The previous checkpoint was `2216ce9`. This review inspected later commits and staged, unstaged, and untracked changes. During review, current source was committed as `978f6e4` (adding scene); the source tree was clean before this rewrite. This supersedes the previous roadmap's tooling-blocked assessment.

Documentation validation uses `npm.cmd test --prefix docs-site`: it rebuilds docs-site/dist, checks local links, and runs site tests. Engine checks use `cargo check -p game --offline --locked` and `cargo test -p game -p renderer --lib --bins --offline --locked`. Exact results and pending guide updates are recorded in docs-site/docs-state.json.

This review does not certify native GPU behavior, empty-scene resizing, installer execution, browser visual appearance, or clean-checkout reproducibility. These remain explicit verification gates.
