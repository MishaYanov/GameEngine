# Authoring roadmap, Milestones 134–153

The roadmap extends the [Controls and object script proposal](conditional-controls-and-object-scripts.md). It starts from 932b537 and adds twenty milestones. Each milestone has an authoring or runtime problem, concrete files, a contract sketch, and a check that can catch a regression. File paths in code spans are relative to the repository.

## Use it in the Editor

- Open **Controls → Input Actions Assets**, select the Player Controller's asset, choose a **Layout preset**, and select **Add Layout**. Edit the mappings or use the preview motion, scene, and device to see which layout matches. Select **Save Input Actions** when finished.
- Select an object and use **Add Component → Script** in the Inspector. Choose **Open Door on Trigger**, set the angle and speed, and pick a collider marked as a sensor in **Trigger sensor**. Run **Game → Scene Information → Check Scenes** to locate a missing script or sensor. On a linked instance, edit the selected instance's override or open the source Prop to change future instances.
- Open **Game → Physics Layers…** in Edit mode. Choose **Preview Standard World → Use preset → Apply** to add the policy to project history. Use **Save All** to write it to disk. Select each collider's **Collision layer** in the Inspector. The **Apply Trigger preset** previews the sensor and layer change. **Advanced collision filters** retains access to the individual mask.
- For a playable exercise, copy the folder `apps/headless/tests/fixtures/conditional-authoring/`, open its `project.json` from **File → Open Project…**, and follow the fixture README to place two linked doors. Space jumps on Land and sends Attack on a fresh press in the air. Use Right Arrow to approach the near door's sensor.

## Controls layouts and their priority

The Controls dialog answers which physical controls produce which game actions at a particular time. A template gives the author an editable starting point. Choosing a template fills in its condition and starting overrides. It becomes eligible after the author chooses any required target and saves it.

| Preset | Eligibility | Starting behavior |
| --- | --- | --- |
| Default | Used when no condition matches | Existing project's full action map. The fallback remains available. |
| Land | The possessed character has committed motion and is grounded | Inherit Default. Add explicit ground-specific bindings when needed. |
| Airborne | The possessed character has committed motion and is airborne | Inherit movement and look. The example disables Jump and maps the same physical button to Attack. |
| World Scene | The installed scene has the chosen stable Scene ID | Inherit Default. Example override for a specific world, room, or stage. |

The topmost saved match wins. For example, order **World Scene, Airborne, Land** to give a specific scene priority over movement state.

| Current input context | Eligible layouts | Effective layout in that order |
| --- | --- | --- |
| Grounded in an ordinary scene | Land | Land |
| Airborne in an ordinary scene | Airborne | Airborne |
| Airborne in the selected World Scene | World Scene, Airborne | World Scene |
| Entered a new scene and no motion has committed yet | Any matching World Scene row | That row or Default |
| No possessed character or no motion sample | Any matching World Scene row | That row or Default |
| The owning window loses input, controller disconnects, or gameplay capture ends | Follow the existing input owner and clear its captured inputs | Resume from fresh held/edge state when input returns |

Land and Airborne look at the installed actor's committed movement state, including readiness. A prepared scene or default-initialized character contributes no movement fact. A switch retains unchanged held actions, releases outgoing ones once, and requires a fresh press before a remapped held button emits an action. A future Swimming template requires authoritative swimming state and a Vehicle template requires seat possession. An overlap with a water volume can supply an event for that work.

## Terrain, World, and the interaction matrix

The Physics Layers dialog answers which colliders can interact. The starter **Standard World** template includes Terrain for land and floors, World for walls and fixed structures, Player for the controlled actor, Prop for dynamic colliders, and Trigger for sensors. Default represents unclassified colliders. Terrain or World geometry can provide support when its surface and the existing motor rules allow it.

This is a proposed starter matrix for explicit application. Existing projects without a matrix use the current all-allowed project policy and keep their collider layers and masks.

| Layer | Default | Terrain | World | Player | Prop | Trigger |
| --- | --- | --- | --- | --- | --- | --- |
| Default | Allow | Allow | Allow | Allow | Allow | Ignore |
| Terrain | Allow | Ignore | Ignore | Allow | Allow | Ignore |
| World | Allow | Ignore | Ignore | Allow | Allow | Ignore |
| Player | Allow | Allow | Allow | Allow | Allow | Allow |
| Prop | Allow | Allow | Allow | Allow | Allow | Ignore |
| Trigger | Ignore | Ignore | Ignore | Allow | Ignore | Ignore |

- One cell covers both directions. **Allow** permits a pair that both collider masks already accept. Two solid colliders can block. A pair with a sensor can produce an overlap through the existing sensor rules. **Ignore** excludes the pair.
- Keep the native 32-bit assignments. Rename display labels freely. Archive slots without recycling any bit that authored colliders, prefab instances, masks, or camera settings could still reference.
- The **Collision layer** picker selects membership. **Apply Trigger preset** previews the sensor and membership edits. Per-collider masks remain available in **Advanced collision filters** and can further restrict the matrix.
- Edit the matrix through **Game → Physics Layers…**. Apply commits one project history entry and takes effect on the next Play. **Save All** persists it. A manual collider or linked instance override retains its own data.
- The orbit camera keeps its one-way obstruction query. Set its numeric **Obstacle layer mask** in Camera settings.
- Grounding can indirectly change the Controls layout. A solid floor can support the Player and make Land eligible. A sensor can notify a script while Land remains active. Turning off the Player–Terrain pair can remove support and make Airborne eligible after motion commits.

The matrix belongs to project data and compiles once into query rows. Two multi-layer colliders must have an **allowed pair among the bits accepted by both existing masks**.

```rust
let accepted_left = left.layers & right.mask;
let accepted_right = right.layers & left.mask;
matrix.any_allowed_pair(accepted_left, accepted_right)
```

See [Collider and scene editing ownership](../../docs/scene-editing.md) · [Input actions and possession](../../docs/adr/0010-input-actions-and-possession.md).

## Controls milestones

### 134. Open Controls from the top menu

The Input Actions editor is hard to discover from the workspace toolbar. Add **Controls** beside File and Game. Reuse the existing managed dialog, input monitor and edit guard.

In `apps/editor/src/app/modules/editor/components/editor-header/editor-header.{ts,html}`, emit the request. Forward it through `apps/editor/src/app/modules/editor/editor.html` and `apps/editor/src/app/modules/editor/workspace/editor-workspace.ts` to `apps/editor/src/app/modules/editor/workspace/game-input/game-input-manager.{ts,html}`. Keep one modal and a single desktop input capture owner.

```html
<button type="button" (click)="controlsRequested.emit()">Controls</button>
```

**Verify** keyboard open, Escape, focus restored to Controls, the current asset inside the dialog, and unavailable states during project replacement.

Resources [Controls dialog design](conditional-controls-and-object-scripts.md#1-controls-dialog) · [Modal behavior](runtime-and-rendering.md#102-modal-behavior).

### 135. Read and save versioned layout assets

An author needs one asset shared by the controller and graph. Keep existing action IDs and types. Preserve v1 assets as Default-only, keep their binding digest, and write v2 when the author saves conditional rows.

Extend `crates/engine-core/src/input/action_map.rs`, `crates/bridge/src/input_actions.rs`, and `apps/editor/src/app/core/game-input/input-actions.models.ts`. Keep the asset fingerprint and revision in `apps/editor-host/src-tauri/src/commands/editor/input_actions.rs`.

```json
{ "schema_version": 2, "actions": [], "layouts": [] }
```

**Verify** an existing v1 asset opens and keeps its digest, while a v2 asset reads back with its row order, stable IDs and explicit empty binding overrides.

Resources [Input Actions data and validation](conditional-controls-and-object-scripts.md#draft-data-and-validation) · [Asset identity and import](../../docs/adr/0006-asset-identity-and-import-pipeline.md).

### 136. Validate complete effective maps

An override can accidentally claim a control already inherited by another action. Compile each row against Default. Validate the full map for that row, condition fact IDs and bounds. Return both conflicting action IDs and the layout.

Own the native rules in `crates/engine-core/src/input/action_map.rs` and use them from the shared loader and `compiler/src/preflight.rs`. An omitted action inherits. An explicit empty array removes its binding for the row.

```rust
let effective = apply_overrides(&default.actions, &layout.overrides)?;
validate_effective_bindings(&effective)?;
```

**Verify** Jump/Attack reuse across different layouts, reject duplicate controls inside a layout, and allow the example after explicitly unbinding Jump.

Resources [Input Actions validation](conditional-controls-and-object-scripts.md#draft-data-and-validation) · [Compiler and Player](../../docs/project-lifecycle.md#build-and-playerheadless-behavior).

### 137. Author Land, Airborne, and World Scene presets

Authors should be able to start from a common case and see which bindings are inherited. Add the preset chooser, condition form, ordered list, binding table and save/cancel states in `apps/editor/src/app/modules/editor/workspace/game-input/input-actions-editor.{ts,html,scss}`. Keep stable IDs when renaming. Duplicate creates a fresh ID. World Scene chooses a catalog Scene ID.

```ts
type Condition =
    | {kind: 'character_grounded'; value: boolean}
    | {kind: 'scene_is'; sceneId: SceneId};
```

**Verify** that presets are editable, a deleted or unavailable scene leaves an actionable field error, and a stale native save keeps the draft intact.

Resources [Controls dialog design](conditional-controls-and-object-scripts.md#1-controls-dialog) · [Project scene identity](../../docs/project-lifecycle.md#project-format-and-scene-identity).

### 138. Explain which layout wins in the dialog

Overlapping conditions are difficult to reason about from a list of names. Add an isolated test pane in `apps/editor/src/app/modules/editor/workspace/game-input/` with Scene, motion state, current device, and an action trace. Show whether a row matches, is shadowed, does not match, or is unavailable. Keep dialog key capture within the modal.

```ts
type MotionPreview = 'grounded' | 'airborne' | 'unavailable';
const winner = firstMatchingLayout(layouts, previewFacts) ?? defaultLayout;
```

**Verify** the decision table above, including scene-only matching without motion. Test keyboard and gamepad capture without changing the open scene or triggering Live Play actions.

Resources [Controls test condition](conditional-controls-and-object-scripts.md#1-controls-dialog) · [Keyboard dispatch](scene-view-interactions.md#8-keyboard-dispatch-priorities).

### 139. Choose the active layout from committed gameplay state

A fresh character commonly has a default grounded value before its motor first runs. Surface readiness for the current actor in `crates/engine-core/src/character_controller.rs`. Resolve the matching layout at the start of the fixed step in `crates/engine-core/src/simulation_session/player_control.rs` using installed scene and current possession. Leave Controls Graph consuming logical IDs.

```rust
let grounded = committed_motion_for(possessed_handle).map(|motion| motion.grounded);
let active = layouts.resolve(installed_scene_id, grounded);
```

**Verify** the startup fallback, the first committed Land/Airborne sample, scene adoption, restart, and replacement of the possessed actor.

Resources [Layout switching contract](conditional-controls-and-object-scripts.md#how-the-game-switches-layouts) · [Lifecycle and scheduling](../../docs/adr/0005-runtime-lifecycle-and-scheduling.md).

### 140. Preserve short taps and handle held buttons during switches

The same held button must never produce a fresh Attack merely because its binding changed. Move applicable physical state and edge handling ahead of the resolver in `crates/runtime/src/scene_editor/play_input.rs`, `crates/engine-core/src/simulation_session/{input,player_control}.rs`, and the existing Player entry in `apps/game/src/main.rs`.

```rust
release_departed_actions_once();
retain_held_actions_with_unchanged_bindings();
require_fresh_press_for_remapped_held_controls();
```

**Verify** a short tap between display frames, a held jump through Land → Airborne, a fresh attack after release, stick resolution, pointer reset, disconnect and catch-up grouping.

Resources [Sampling and delivery](../../docs/adr/0010-input-actions-and-possession.md#sampling-and-bounded-delivery) · [Layout switching contract](conditional-controls-and-object-scripts.md#how-the-game-switches-layouts).

### 141. Validate personal rebinds against conditional layouts

A Player rebind that looks valid on Land can conflict with the Airborne override. Keep `InputActionLayout` as a personal asset-bound overlay. Validate it in `crates/engine-core/src/input/action_map.rs` and surface the result through the existing Player Controls flow. First apply valid changes to Default and use them for inherited entries.

```rust
let rebound_default = map.with_layout(asset_id, personal_layout)?;
validate_all_conditional_layouts(&rebound_default)?;
```

**Verify** a conflict identifies the affected layout and action, an old digest falls back safely, and a valid rebind changes inherited rows while leaving explicit overrides effective.

Resources [Saved controls and scoped overrides](conditional-controls-and-object-scripts.md#draft-data-and-validation) · [Input actions and possession](../../docs/adr/0010-input-actions-and-possession.md).

## Object script milestones

### 142. Add a saved Script attachment

Authors need a recognizable component and a stable identity for Undo, references, and multiple instances. Add an explicit Script record in `crates/engine-core/src/components.rs` and a new `crates/engine-core/src/components/script.rs`. Persist it through `components/entity.rs`, `scene_data.rs`, and `scene_format.rs` in the same crate. Teach the bridge and `apps/editor/src/app/core/editor/editor-components.ts` the component kind.

```json
{ "id": "door-control", "script_id": "project.open_door_on_trigger", "enabled": true, "config_version": 1 }
```

**Verify** legacy scenes read with zero scripts, Undo restores an attachment, save/reopen retains ID and data, and an unknown ID remains visible for repair.

Resources [Script attachment data](conditional-controls-and-object-scripts.md#what-gets-saved) · [Object composition](../../docs/adr/0002-object-composition-and-components.md).

### 143. Generate Inspector fields from trusted script descriptors

A JSON box alone gives authors little guidance. Register named compiled scripts, their versions, typed property fields, reference slots and capabilities in `crates/gameplay-catalog/src/lib.rs` and the Script descriptor module. Extend `apps/editor/src/app/modules/editor/workspace/components/component-editor.{ts,html}` and `scene-component-inspector.ts` in the same directory. Reuse component commands in `crates/engine-core/src/editor/operations.rs`.

```rust
properties.number("open_angle_degrees", default(90.0), range(0.0..=180.0));
references.required_entity("sensor", Requires::TriggerCollider);
```

**Verify** selecting a script draws its properties, the picker filters incompatible objects, invalid numbers or versions produce field errors, and changing settings uses Undo.

Resources [Script configuration and references](conditional-controls-and-object-scripts.md#what-gets-saved) · [Rust–TypeScript bridge](../../docs/adr/0003-rust-typescript-bridge.md).

### 144. Run scripts within the fixed-step commit boundary

A script can write state or issue actions at the wrong time if it starts during scene preparation. Add prepared instances and private state through `crates/engine-core/src/game_session.rs`, `simulation_session.rs`, and a new `simulation_session/object_scripts.rs`. Resolve generation-checked handles with `runtime_lifecycle.rs`.

```rust
candidate.scripts.plan_tick(&view, routed_input, &mut bounded_actions)?;
commit_candidate_only_after_validation(candidate);
```

**Verify** preparation has no effects, Start runs once on installation, two instances have separate state, a rejected action rolls the step back, and Stop/Restart retires or recreates instances.

Resources [Script game-loop hooks](conditional-controls-and-object-scripts.md#how-scripts-join-the-game-loop) · [Scheduling and rollback](../../docs/adr/0005-runtime-lifecycle-and-scheduling.md).

### 145. Deliver trigger events and logical input to subscribed scripts

A script should see events relevant to its declared sensor or actor. Use committed trigger edges from `crates/engine-core/src/triggers.rs` and routed actions in `simulation_session/input.rs`. Index trigger subscribers by resolved sensor handle in the new object script adapter. Direct player input is available to opted-in scripts on the possessed actor.

```rust
let events = committed_triggers.for_sensor(references.sensor);
script.on_event(context, candidate_state, event, effects)?;
```

**Verify** the event reaches the selected sensor's subscriber once, unrelated sensors and actors are excluded according to the descriptor, a remapped held input does not synthesize a press, and a rejected candidate publishes no trigger event.

Resources [Script hooks](conditional-controls-and-object-scripts.md#how-scripts-join-the-game-loop) · [Input possession](../../docs/adr/0010-input-actions-and-possession.md).

### 146. Build Open Door on Trigger with typed object access

Authors need a working example that proves a script can read live properties, receive an event, and request a change. Add the compiled example under `crates/cinder-oath-gameplay/src/scripts/open_door_on_trigger.rs` with registration through that gameplay package. Exercise it in a dedicated fixture. Adapt validated effects through `crates/engine-core/src/simulation_session/gameplay.rs`.

```rust
if state.opening {
    let next = approach_angle(ctx.transform().local_yaw(), state.goal, config.speed * ctx.step_seconds());
    out.set_local_yaw(ctx.owner(), next)?;
}
```

**Verify** that changing angle and speed in the Inspector changes the result, that the chosen sensor opens its door, and that Play edits no authored Transform. Reject effects outside the allowed target or component.

Resources [Door hook API sketch](conditional-controls-and-object-scripts.md#proposed-rust-hook-shape) · [Game modules and scripting](../../docs/adr/0008-game-modules-and-scripting.md).

### 147. Make script references survive Prop edits and instances

A copied door must reference its own sensor. Support script editing in the existing Prop draft path in `apps/editor/src/app/modules/editor/workspace/prop-workspace/prop-workspace.service.ts` and `apps/editor-host/src-tauri/src/prefab_sources.rs`. Remap declared local refs in `crates/engine-core/src/components/entity.rs` and `editor/prefab/instantiate.rs`. Add instance override/revert in `linked_prefab/overrides.rs` using stable source object and component ID.

```rust
let sensor = remap_local_ref(source_script.references.sensor, instance_addresses)?;
```

**Verify** two nested or copied Props bind their own sensors, an explicit scene slot survives reopen, an override survives an unrelated source refresh, and Revert restores the pinned source.

Resources [Prop reference and state rules](conditional-controls-and-object-scripts.md#what-gets-saved) · [Component ownership](../../docs/adr/0002-object-composition-and-components.md).

### 148. Show the same script problem in the editor and build preflight

An unresolved script should point to a specific component before an export starts. Use one typed problem in `crates/bridge/src/scene_dependencies.rs` and `apps/editor-host/src-tauri/src/editor/scene_verification.rs`. Share the registry across `apps/editor-host/src-tauri/src/editor.rs`, `apps/game/src/main.rs`, `apps/headless/src/replay.rs`, and `compiler/src/preflight.rs`.

```json
{ "code": "missing_script", "entity_key": "door", "component_id": "door-control" }
```

**Verify** Problems opens the exact attachment, required invalid scripts block Play and export, a repair clears both results, and valid registered code plus dependent data are included in the staged build.

Resources [Script files and acceptance](conditional-controls-and-object-scripts.md#files-and-acceptance-1) · [Build preflight](../../docs/project-lifecycle.md#build-and-playerheadless-behavior).

## Physics interaction milestones

### 149. Name stable collision layers in project data

Raw bits are hard to use and easy to accidentally reinterpret. Add optional named layers and canonical ignored pairs through `crates/engine-core/src/project.rs` and `project_format.rs`. Use existing collider bit slots. A display rename changes presentation. Old files have no custom matrix and use all-Allow policy.

```json
{ "layers": [{ "bit": 1, "id": "terrain", "label": "Terrain" }], "ignored_pairs": [[1, 5]] }
```

**Verify** a legacy project loads with identical collider filters, bit 31 round-trips, and renaming or archiving a layer preserves collider, prefab, and camera bit references.

Resources [Terrain, World, and the interaction matrix](#terrain-world-and-the-interaction-matrix) · [Project format](../../docs/project-lifecycle.md#project-format-and-scene-identity).

### 150. Use the same matrix predicate for runtime contacts

Movement, overlaps and line of sight can disagree if they each reinterpret a cell. Compile the optional project policy into 32 reciprocal rows. Apply it through `crates/engine-core/src/collision.rs`, `collision/geometry.rs`, and `collision/movement.rs`. Carry the policy through each collision world rebuild in `simulation_session.rs`. Keep the scene-only legacy constructor as the all-Allow path.

```rust
fn allows_pair(&self, a: ColliderInteraction, b: ColliderInteraction) -> bool {
    self.any_allowed_pair(a.layers & b.mask, b.layers & a.mask)
}
```

**Verify** symmetric and diagonal cells, bit 31, two solid contacts, a sensor edge, and the multi-bit case where individually accepted raw masks only intersect a different pair than the matrix permits. Confirm the camera's one-way obstruction query behaves as before.

Resources [Terrain, World, and the interaction matrix](#terrain-world-and-the-interaction-matrix) · [Runtime ownership](runtime-and-rendering.md#3-state-model).

### 151. Edit and apply the world matrix from Game

Authors need a compact, keyboard-accessible place to configure all pairs. Add **Game → Physics Layers** through `apps/editor/src/app/modules/editor/components/editor-header/`. Put the modal in the workspace and use the revisioned project path in `crates/engine-core/src/editor/mod.rs` and `editor/factory.rs`. Show names, diagonal pairs, an explicit Standard World preset preview, and an undoable Apply.

```ts
const orderedPair = [Math.min(bitA, bitB), Math.max(bitA, bitB)] as const;
```

**Verify** keyboard edits update one cell, the mirror follows, Cancel preserves the saved policy, an invalid stale Apply leaves the draft visible, and Undo restores the previous project policy.

Resources [Standard World matrix](#terrain-world-and-the-interaction-matrix) · [Project history](../../docs/project-lifecycle.md#documents-switching-and-history).

### 152. Assign named layers and explain suppressed trigger pairs

Authors should be able to select Terrain, World, Player, Prop, or Trigger on a collider and see why an expected trigger is silent. Extend `apps/editor/src/app/modules/editor/workspace/collider/collider-inspector.{ts,html}` and the existing component editor. Keep manual masks in Advanced. Extend project-aware checks in `crates/engine-core/src/components/scene_validation.rs` and `state_machine/validation.rs` and share effective-scene checks with `compiler/src/preflight.rs`.

```rust
if required_pair && !project_policy.allows_pair(sensor, actor) {
    report_suppressed_pair(sensor_owner, actor_owner);
}
```

**Verify** named assignments work on a scene or a linked collider override, preview changing a blocking shape to a sensor, expose a suppressed required pair and its reason, and give optional sensor pairs a diagnostic without making them required.

Resources [Layer ownership and presets](#terrain-world-and-the-interaction-matrix) · [Scene editing](../../docs/scene-editing.md).

## Integrated acceptance milestone

### 153. Rehearse a small world through Editor, headless, and the exported Player

Independent tests can miss a mismatch between the authoring paths and shipping game. Add a dedicated fixture under `apps/headless/tests/fixtures/conditional-authoring/`. Cover project physics settings, control presets, a linked door with a sensor and a typed Script attachment. Exercise the shared Editor authoring API, the Engine fixed-step path, the headless binary and the packaged Player from `apps/headless/tests/conditional_authoring.rs`. Cover pending dialog changes in `apps/editor/src/app/modules/editor/workspace/editor-workspace.editing.spec.ts`.

```text
Land support → fresh Jump → Airborne → fresh Attack → Land
Player touches Trigger → one Open Door event → only its linked door opens
```

**Verify** source and staged data, reopen and restart, replay under two frame groupings, and check negative copies with a blocked pair or missing script. Run an exported Player on the target platform and record which window/input checks were exercised.

Resources [Verification workflow](../../docs/testing.md) · [Project build and Player behavior](../../docs/project-lifecycle.md#build-and-playerheadless-behavior).

## Delivery order

Complete 134–141 so the existing actions can move through one context-aware input path. Complete 142–148 with the current sensor and collider model. Complete 149–152 so a project-wide policy can intentionally restrict those same contacts. Use 153 as the combined gate.

Later presets can add committed swimming or seat facts. Keep that work tied to the movement behavior, authoring setup, and replay evidence required by each new preset.
