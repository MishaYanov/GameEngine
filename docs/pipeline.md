# Engine Pipeline

Reviewed on 2026-10-21 against commit `72dad5d` and staged, unstaged, and untracked changes, including the controller/gameplay split and scene persistence prototype. Renderer extraction and the five-step runtime move are complete for their stated scope; timing/input/camera are now in progress in the working tree; initial scene persistence and the controller/gameplay split are now also in progress; editor integration remains future work.

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
| Window and Vulkan rendering | Implemented | Surface/device selection, swapchain, frame submission, indexed drawing, depth testing, resize handling, shutdown wait. | User confirms resize/minimize and shutdown checks; repeat after renderer changes. |
| Camera, timing, and input | In progress | Runtime FrameClock, InputState/Key, and Camera; provisional delta-scaled object/camera movement and captured mouse look in game. | Native control validation, input/action design, fixed-step policy, and completion review; see [guide](timing-input-camera.md). |
| glTF/GLB import | Partial | All primitives of the first mesh, geometry, indices, UVs, base-color image/factor, double-sided setting. | Scene nodes and their transforms, other meshes, normals/tangents, broader image/material support. |
| Materials | Partial | Unlit texture multiplied by color factor, white fallback, single/double-sided pipelines. | Lighting, alpha modes, normal maps, metallic/roughness. This is not full PBR. |
| Asset caches | Implemented | Cached GPU meshes/textures and model-path-qualified embedded-image keys. | Stable asset references, reload/invalidation policy, model-level caching. Repeated loads can still decode the file again. |
| Model abstraction | Implemented in runtime | Shared Model/ModelPart types; AssetManager imports parts and delegates GPU upload to WindowRenderer. | Portable asset handles/backend independence remain future design work. |
| Model instances | Implemented in runtime | Shared Arc<Model> and independent transforms; consumed by game through runtime APIs. | Initial persistence is in progress; broader gameplay API remains to be defined. |
| Scene and IDs | Implemented in runtime | Spawn, mutation, removal, iteration, monotonically allocated IDs, and borrowed render_objects output. | Initial JSON snapshots, names/source paths, and file I/O now exist in progress; hierarchy, durable references, and ECS remain absent. |
| Shared material layout | Implemented; native checks user-confirmed | Layout belongs to WindowRenderer; empty-scene pipeline setup does not need a model. | Repeat native checks after future presentation changes. |
| Renderer ownership | Complete for extraction milestone | WindowRenderer owns initialization, upload, draw, deferred resize, and shutdown; GameApp owns application behavior. | External GPU resource lifetimes still depend on application drop order and idle waits. |
| Shared runtime | Complete for five-step migration | Assets, models, instances, scenes, Transform, and draw-request construction are shared runtime APIs; tests and smoke imports moved. | Status-only Engine/Renderer path is still separate; timing/input/camera are in progress; editor viewport remains future work. |
| Diagnostics | Implemented | engine_logger, component health, error propagation, validation logging. | Extend diagnostics as systems arrive and measure actual runtime behavior. |
| Editor shell | Partial | Angular/Tauri project setup and GPU status UI. | No integrated scene viewport, entity inspector, or scene-editing workflow. |
| Packaging | Recursive asset-copy fix implemented | Compiler copies the complete nested asset tree, including robot.glb; a test verifies copied model/texture bytes. | Full installed release execution outside checkout remains separate validation. |
| Tests and documentation | Expanded | Runtime public-API/import/scene tests, renderer lifecycle smoke, default/native runner, and docs/site checks. User confirms native/visible checks after migration. | Clean-checkout, installed-run, CI deployment, and broader coverage remain separate. |

Evidence: [game](apps/game/README.md), [renderer](crates/renderer/README.md), [runtime](crates/runtime/README.md), [editor](apps/editor/README.md), [compiler](compiler/README.md), and [testing](testing.md). Source inspected includes apps/game/src/main.rs, its renderer_smoke example, crates/runtime scene/instance/transform/assets modules and public-API tests, crates/renderer/src/window_renderer.rs, and compiler asset copying. The reusable modules are no longer defined in the game binary.

## Current working-tree additions — in progress

`CameraController` now separates camera movement/mouse math from platform events. `RobotGame` separates robot selection/movement/rotation from GameApp and chooses the first two scene entities by file order. Tab toggles the selected robot.

Initial persistence now exists: schema-version-1 SceneData, named source-path scene entries, JSON validation, scene_io load/save, bundled robots.json, and F5/F9 commands. Save writes the development asset scene; reload builds a replacement before swapping it after GPU idle. These are implemented prototypes awaiting complete native/visual and distribution checks, not a finished authoring workflow. See [scene persistence](scene-persistence.md).

Compiler recursive copying includes scene JSON; the admin builder asset allowlist still excludes it, creating a new generated-runtime packaging gap.

## Completed foundation and next work

Renderer ownership extraction is complete: WindowRenderer owns GPU presentation resources, uploads, deferred resize/rebuild, and shutdown. The subsequent five-step sharing sequence is also complete:

| Step | Status | Evidence |
| --- | --- | --- |
| Introduce a game library | Complete as an intermediate structure | No permanent game-library dependency is needed after the move. |
| Move draw-request construction to Scene | Complete | Scene::render_objects builds borrowed requests. |
| Move Transform to runtime | Complete | Game uses runtime::Transform. |
| Move assets/models/instances/scenes | Complete | Public runtime modules and external scene_api tests exist. |
| Validate after moving | Complete, user-confirmed | Tests, native smoke, visible rendering, resize, and minimize passed per user confirmation. |

The next implementation sequence is:

1. **In progress:** FrameClock supplies delta seconds capped at 100 ms; game rotation/movement now uses delta time.
2. **In progress:** InputState tracks key transitions/held state, Camera supplies movement/look data, and game adapts keyboard/mouse/focus events. Validate native interaction before closing the milestone.
3. Establish broader gameplay/update boundaries and fixed steps where required.
4. **In progress:** versioned scene JSON, asset-relative model paths, names/transforms/camera, save/reload, and validation now exist. Durable entity references, migration, and full native/packaging evidence remain.
5. Integrate the editor viewport and scene commands with shared APIs.

Timing/input/camera, gameplay separation, and initial persistence are ongoing after extraction. Editor integration remains planned. Recursive asset copying has resolved the old robot omission; running a fully installed release outside the checkout remains a separate check.

## When to build the UI

The editor shell already exists. Renderer ownership and shared runtime scene boundaries now exist. Next inspect the Angular/Tauri host to choose viewport presentation and edit-command transport.

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

This review compared the previous checkpoint `978f6e4`, later commits through `72dad5d`, and current staged/unstaged/untracked changes. The runtime migration is currently in the working tree, so the commit alone does not identify all documented source.

The user confirmed post-move tests, native renderer smoke, and visible-game resize/minimize checks passed. The attached earlier notes report RTX 4070 smoke execution and the 640×360 empty-scene extent. These are attributed results rather than a claim of rerunning visible GPU checks during documentation editing.

Documentation rebuilds and focused automated tests run in this review are recorded in `docs-site/docs-state.json`. Browser appearance, installed release execution, clean-checkout reproducibility, and the draft Jenkins configuration are not certified by the migration confirmation.
