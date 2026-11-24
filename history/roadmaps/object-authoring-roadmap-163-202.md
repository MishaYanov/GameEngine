# Object authoring roadmap, Milestones 163–202

This roadmap tracks the implemented Create → Hierarchy → Inspector → specialist editor workflow. It follows [Milestones 154–162](editor-access-and-particles-154-162.md), which introduced the Objects menu, the Animation layout, and Particle Designer. The milestones retain their file-level implementation guidance, examples, and acceptance checks. See [Windows Editor quickstart](../../docs/windows-editor-quickstart.md) and [object authoring and Agent package acceptance](../../docs/testing.md#object-authoring-and-agent-package-acceptance) for the current workflow and verification procedure.

## Authoring model

| Area | Owns |
| --- | --- |
| Create | One searchable catalog of scene objects, starter shapes, and useful presets. The top Objects menu and Hierarchy share it. |
| Hierarchy | Placed objects, parent-child relationships, and folders for organizing scene roots. |
| Inspector | The selected object’s identity, transform, component bindings, and short property cards. Add Component attaches capabilities. |
| Specialist panes | Detailed authoring for Animation, Animator, Controls Graph, Audio, Particles, and behavior settings. Cards open the relevant pane with the selected object and asset in context. |
| Prop Workspace | The reusable definition and its object tree. Placed instances show their inherited properties and supported overrides. |
| Agent | An alternate mode for the right drawer. It receives the same selected object or Prop address as Inspector and combines current project facts with published guidance. |
| Scene and Project settings | Shared responsibilities such as Scene Lighting, active cameras, input layouts, and collision layers. |

Generate the first four primitives as ordinary project GLBs using the existing mesh builders and asset transaction. This keeps picking, render bounds, Prop previews, and export on the existing asset path. Keep the first Inspector integration on the current component views and typed commands. Each specialist pane owns its draft and preview, and receives the exact selected object when it opens.

## Delivery checkpoints

| Milestones | Result to demonstrate |
| --- | --- |
| 163–170 | Plane, Cube, Sphere, and Capsule can be created, edited, saved, and exported from a blank project. |
| 171–177 | Scene objects use a predictable Inspector and one Add Component picker. |
| 178–184 | Component cards lead directly to the correct detailed editor, with safe draft navigation. |
| 185–188 | Editing a reusable Prop and inspecting a placed copy use the same object language and explicit document scope. |
| 189–192 | A new author can complete a small playable project through the connected workflow. Existing projects and the packaged Editor pass compatibility checks. |
| 193–202 | The future Agent swaps with Inspector, resolves the exact selected object or Prop, and answers with current project context and cited reference material. |

## Create objects and ship the starter shapes

### 163. Pin the object authoring baseline

**Problem.** Creation, authoring, and export use separate entry points today. Record the same starting workflow before changing any of them.

**Implementation.** Extend `apps/editor/src/app/modules/editor/workspace/editor-workspace.editing.spec.ts` with a blank-project fixture for creating and selecting one object. Record existing scene, Prop, Animation, and Controls behavior in `apps/editor/src/app/modules/editor/workspace/workspace-draft-review.spec.ts`. Use `apps/headless/tests/fixtures/authoring-controller/full/project.json` and `projects/cinder-oath/project.json` as older-project controls. Keep folder membership, transform parenting, authored component IDs, and preview state as distinct facts in the fixtures.

```ts
expect(snapshot.active_scene_id).toBe(snapshot.project.startup_scene_id);
expect(object.components.map(component => component.id)).toEqual(initialIds);
```

**Verify.** Run the current Angular and Rust tests and record the inspected commit and known limits. Open a blank project, save it, reopen it, and export the existing fixture before comparing later changes.

**Resources.** [Object identity and hierarchy](../adr/0002-object-composition-and-components.md#base-object-identity-and-hierarchy) · [State ownership](../../designs/editor-viewport/runtime-and-rendering.md#3-state-model).

### 164. Give both Create menus one catalog

**Problem.** Objects and Hierarchy currently offer different creation shortcuts.

**Implementation.** Move the shared action definitions into `apps/editor/src/app/core/editor/editor-object-menu.ts`. Feed the same capability-filtered list into `apps/editor/src/app/modules/editor/components/editor-header/editor-header.ts` and `apps/editor/src/app/modules/editor/workspace/scene-entities-panel/scene-entities-panel.ts`. Keep `apps/editor/src/app/modules/editor/workspace/editor-workspace.ts` as the execution owner. Group creation, asset browsing, and scene settings as separate actions. Carry the focused folder or Scene root as a placement context only for object actions that support it.

```ts
interface CreateEntry {
    readonly id: CreateActionId;
    readonly group: 'objects' | 'presets' | 'assets' | 'scene';
    readonly label: string;
    readonly capability?: CreateCapability;
    readonly action: CreateAction;
}
```

**Verify.** Objects and Hierarchy show the same available creation choices. Both support keyboard selection, disable mutations during Play, and make the same undoable edit for the same action and destination.

**Resources.** [Existing Objects menu milestone](editor-access-and-particles-154-162.md#156-add-the-objects-menu) · [Commands and history](../adr/0001-editor-command-pipeline.md).

### 165. Define the four starter primitives

**Problem.** A display name in a menu needs a stable model identity and predictable size.

**Implementation.** Add `crates/engine-core/src/primitive_kind.rs` and export it from `crates/engine-core/src/lib.rs`. Limit the first catalog to Plane, Cube, Sphere, and Capsule. Define documentable dimensions and pivots in a small manifest beside the generator in `crates/runtime/examples/generate_primitive_glbs.rs`. Confirm the actual positions from `crates/runtime/src/assets/mesh_data.rs` before choosing whether to normalize the mesh. Use the XZ plane and Y up. Put the Plane pivot on the surface and center the solid primitives on their bounds.

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrimitiveKind { Plane, Cube, Sphere, Capsule }
```

**Verify.** Every ID round-trips through serde. Each fixture defines the pivot, expected local bounds, unit scale, and orientation. Unknown IDs fail at the command boundary.

**Resources.** `crates/runtime/src/assets/mesh_data.rs` · [Asset identity](../adr/0006-asset-identity-and-import-pipeline.md).

### 166. Generate self-contained primitive models

**Problem.** The existing mesh generators need portable assets the normal Editor and Player asset readers can share.

**Implementation.** Implement `crates/runtime/examples/generate_primitive_glbs.rs` using the four `MeshData` constructors. Write reproducible GLBs under `apps/editor-host/src-tauri/assets/primitives/`. Include positions, normals, UVs, triangle indices, bounds, and a neutral material. Use one embedded binary buffer and generate the files offline. Add round-trip coverage through the public glTF loader in `crates/runtime/tests/gltf_scene_api/decoding.rs`. Loader internals are covered separately by `crates/runtime/tests/unit/assets/gltf_loader/mesh.rs` and `crates/runtime/tests/unit/assets/gltf_loader/resources.rs`. Compare a regeneration against the checked-in bytes.

```rust
let mesh = match kind {
    PrimitiveKind::Plane => MeshData::plane(),
    PrimitiveKind::Cube => MeshData::cube(),
    PrimitiveKind::Sphere => MeshData::sphere(),
    PrimitiveKind::Capsule => MeshData::capsule(),
};
write_glb(mesh.vertices(), mesh.indices(), destination)?;
```

**Verify.** The production glTF loader opens each generated file. Validate finite bounds and normals, correct winding from the visible side of Plane, UVs, and deterministic regeneration. All dependency reads resolve within the GLB.

**Resources.** [Graphics and asset import](../../docs/graphics-and-assets.md) · [Engine asset pipeline](../adr/0006-asset-identity-and-import-pipeline.md).

### 167. Publish each primitive once per project

**Problem.** Repeated creation should reuse a project asset, and a project can already contain a user model with the same filename.

**Implementation.** Extend `crates/runtime/src/assets/project_assets.rs` with a bounded byte-source preparation path using its existing asset validation and publication helpers. Add a primitive operation to `apps/editor-host/src-tauri/src/commands/editor/asset_authoring.rs` and register its command in `apps/editor-host/src-tauri/src/lib.rs`. Choose versioned project-relative paths in the host. Verify a matching entry by bytes and catalog identity. For a conflicting existing path, resolve a safe distinct path with the existing collision policy or return a clear error. Serialize first-use publication with the existing reservation and stale-generation checks.

```rust
let asset = primitive_catalog.ensure(kind, project_context)?;
// The host derives the trusted bytes and the project-relative destination.
Ok(asset.id_and_path())
```

**Verify.** Two requests for Cube reuse one asset. A concurrent request and retry keep one catalog entry. A pre-existing user file keeps its bytes. Invalid or stale requests publish no partial file or catalog entry.

**Resources.** [Asset publication and dependencies](../adr/0006-asset-identity-and-import-pipeline.md) · `apps/editor-host/src-tauri/tests/unit/commands/editor/asset_authoring/contract_tests.rs`.

### 168. Spawn a primitive as an ordinary scene object

**Problem.** Asset import and object creation have separate lifetimes. Creating a primitive needs correct save, undo, and scene-switch behavior.

**Implementation.** Add the primitive entry call to `apps/editor/src/app/core/editor/editor.service.ts` and use `apps/editor/src/app/modules/editor/workspace/editor-workspace.ts` plus the existing model placement path in `apps/editor/src/app/modules/editor/workspace/assets-panel/assets-panel.ts`. Capture workspace and scene before asset preparation. After publication, recheck the same scene and obtain the authoritative revision before dispatching ordinary model placement. Group folder membership with creation when supported by the existing placement transaction. Reuse the same imported model in Props and Scene without growing the scene schema.

```ts
const target = workspace.captureScene();
const asset = await editor.ensurePrimitive(kind, target.generation);
if (!workspace.isCurrent(target)) return;
await workspace.placeModel(asset.path, placement, target);
```

**Verify.** One Undo removes the placed object and keeps the reusable model in Assets. Redo preserves the entity identity. A scene switch during preparation places no object in the new scene. If publication succeeds and placement fails, report that the model is available in Assets.

**Resources.** [Scene mutations and undo](../../designs/editor-viewport/runtime-and-rendering.md#9-scene-mutation-and-gesture-transactions) · [Documents and scene switching](../../docs/project-lifecycle.md#documents-switching-and-history).

### 169. Verify primitive assets through export and Play

**Problem.** A primitive has to survive reopening and export before the menu exposes it to authors.

**Implementation.** Add `compiler/tests/primitive_package.rs` using `compiler/src/preflight.rs` and `compiler/src/export.rs`. Stage a project with all four models across two scenes and a linked Prop. Extend a focused `apps/editor-host/src-tauri/tests/editor_workspace/prop_preview.rs` case for the normal mesh bounds and preview path. Use `apps/game/tests/startup_smoke.rs` as the entry point for testing the copied Player. Keep normal dependency validation for every generated GLB.

```rust
let staged = stage_prepared_project(&prepared, destination)?;
assert!(staged.path().join("assets/models/primitives/v1/cube.glb").is_file());
```

**Verify.** Reopen the scene and linked Prop, frame a primitive, start native Play, and launch the relocated export with Cargo off PATH. Removing a referenced primitive asset fails preflight with its project-relative path. A legacy imported GLB still stages and renders.

**Resources.** [Build and Player behavior](../../docs/project-lifecycle.md#build-and-playerheadless-behavior) · [Windows release walkthrough](windows-release-step-1.md#step-1-walkthrough).

### 170. Expose Plane, Cube, Sphere, and Capsule to authors

**Problem.** Users need the starter shapes where they create any other object.

**Implementation.** Enable **3D Object** entries in `apps/editor/src/app/core/editor/editor-object-menu.ts` after the host advertises primitive publication. Wire their creation result into Hierarchy selection and Inspector using `apps/editor/src/app/modules/editor/workspace/editor-workspace.ts` and `apps/editor/src/app/modules/editor/workspace/scene-entities-panel/scene-entities-panel.ts`. Use a deterministic starting position consistent with existing model placement. Keep floor collision in an explicit preset or Add Component action.

```ts
{ id: 'create_plane', group: 'objects', label: 'Plane', capability: 'create_primitive',
  action: {kind: 'primitive', primitive: 'plane'} }
{ id: 'create_capsule', group: 'objects', label: 'Capsule', capability: 'create_primitive',
  action: {kind: 'primitive', primitive: 'capsule'} }
```

**Verify.** Create all four from both entry points. Move, rename, select, group, undo, save, and reopen them. The Capsule row identifies the rendered shape. The Inspector accurately displays the collider actually attached.

**Resources.** [Scene view placement patterns](../../designs/editor-viewport/scene-view-interactions.md) · `apps/editor/src/app/modules/editor/workspace/scene-entities-panel/scene-entities-panel.spec.ts`.

## Build one Inspector and component picker

### 171. Address the selected object in its own document

**Problem.** The same visible name can refer to a scene object, a Prop source object, or a linked child.

**Implementation.** Introduce `apps/editor/src/app/modules/editor/workspace/inspector-panel/inspector-subject.ts` and adapt `apps/editor/src/app/modules/editor/workspace/inspector-panel/inspector-panel.ts` and `apps/editor/src/app/modules/editor/workspace/prop-workspace/prop-workspace.service.ts`. Keep stable object and document IDs in the address. Reuse `WorkspaceAssetScope` from `apps/editor/src/app/core/editor/editor.service.ts` for the editor-side generation, native workspace generation, and expected workspace revision. The command adapter passes the native generation to Rust. The UI checks the editor-side generation before applying an asynchronous result. Carry permissions and the scene or draft revision with the resolving session. Read existing `component_views` and legacy convenience fields from `apps/editor/src/app/core/editor/editor.models.ts`. Leave existing scene and Prop file formats readable.

```ts
type InspectorAddress =
    | {kind: 'scene'; scene: SceneId; object: EntityKey}
    | {kind: 'prop'; draftId: string; sourceObject: EntityKey}
    | {kind: 'linked'; scene: SceneId; root: EntityKey; address: PropAddress};

interface InspectorSubject {
    readonly address: InspectorAddress;
    readonly scope: WorkspaceAssetScope;
    readonly contentRevision: number;
}
```

**Verify.** Switch between two scenes with identically named objects, two Prop drafts, and a nested linked child. Edits reach the intended object or fail with a recoverable stale or read-only state.

**Resources.** [Object identity](../adr/0002-object-composition-and-components.md#base-object-identity-and-hierarchy) · [Source and instance lifecycle](../adr/0009-prefabs-and-instance-overrides.md).

### 172. Make the Inspector an object overview

**Problem.** Full editors mixed with transforms make it hard to see what a selected object contains.

**Implementation.** Refactor `apps/editor/src/app/modules/editor/workspace/inspector-panel/inspector-panel.html` and `apps/editor/src/app/modules/editor/workspace/components/scene-component-inspector.ts`. Put identity, enabled state when supported, transform, and a stable list of compact component cards in one order. Show current binding, enabled state, validation messages, and one primary action per card. Reuse the existing forms during migration. Preserve keyboard focus when a command updates snapshots and changes component order.

```html
<editor-component-card
    [component]="component"
    (openRequested)="openEditor(component.id)" />
```

**Verify.** Select a simple mesh, a player, and a linked Prop child. Name and transform stay in a fixed position. Keyboard users can reach every card, its text status, and its action.

**Resources.** [Workspace arrangement](../../designs/editor-viewport/scene-view-interactions.md#5-workspace-arrangement) · [Component definitions](../adr/0002-object-composition-and-components.md#strict-component-definitions).

### 173. Put every addable component in one searchable picker

**Problem.** Animator, Collider, and other addables currently enter the object through different UI paths.

**Implementation.** Add `apps/editor/src/app/modules/editor/workspace/components/add-component-catalog.ts` and use it in `apps/editor/src/app/modules/editor/workspace/components/component-editor.ts`. Read `crates/engine-core/src/command.rs` capabilities and existing component cardinality. Include existing Animator, Controls, and Collider authoring adapters with groups and eligibility. Show the reason when a component requires a different object or backend capability. Route legacy fields through their existing typed commands until an explicit migration is approved.

```ts
interface AddComponentChoice {
    readonly kind: ComponentKind;
    readonly group: 'rendering' | 'physics' | 'gameplay' | 'audio' | 'animation';
    readonly unavailableReason?: string;
}
```

**Verify.** Search for Animator, Collider, Audio Source, and Player Controller from the same picker. Enable only actions the current target supports. Existing singleton constraints and allowed multiple components behave consistently.

**Resources.** [Component definition registry](../adr/0002-object-composition-and-components.md#definition-registry-and-defaults) · `apps/editor/src/app/modules/editor/workspace/components/scene-component-inspector.ts`.

### 174. Unify attach, enable, disable, and remove

**Problem.** Every card needs consistent lifecycle controls and undo behavior.

**Implementation.** Route the controls in `apps/editor/src/app/modules/editor/workspace/components/component-editor.html` through `apps/editor/src/app/modules/editor/workspace/scene-editing.service.ts` and the equivalent Prop draft adapter. Reuse the native component commands in `crates/engine-core/src/command.rs`. Validate required references and ownership at commit. Surface reasons when a referenced component blocks removal. Preserve component identity when toggling and reload from the accepted revision.

```ts
await componentActions.apply(address, {kind: 'set-enabled', id: component.id, enabled: false});
```

**Verify.** Add a component, toggle it twice, remove it, then undo back through each edit. Rejected removal keeps the card and its error. Switching selection or Play while a draft is invalid uses the shared review.

**Resources.** [Component change notification](../adr/0001-editor-command-pipeline.md) · [Shared draft review](windows-release-step-1.md#1d-review-workspace-drafts-at-workflow-boundaries).

### 175. Use model and material pickers on the Mesh card

**Problem.** Raw model paths expose storage details and invite invalid asset references.

**Implementation.** Move editable model selection from `apps/editor/src/app/modules/editor/workspace/inspector-panel/inspector-panel.html` into a Mesh card backed by `apps/editor/src/app/modules/editor/workspace/components/component-editor.ts`. Reuse Assets selection and typed model commands from `apps/editor/src/app/modules/editor/workspace/assets-panel/assets-panel.ts`. Show the assigned model and material using resolved metadata. Keep existing material/edit commands and support missing references with an explicit replacement action.

```ts
const model = await pickAsset({kinds: ['model']});
if (model) await meshCard.replaceModel(address, componentId, model.path);
```

**Verify.** Swap a primitive model for an imported GLB, set its material through supported controls, and undo. Preserve transform and sibling components. Reopen a legacy entity, then assign a replacement if its model is missing.

**Resources.** [Asset identity and import](../adr/0006-asset-identity-and-import-pipeline.md) · [Graphics assets and materials](../../docs/graphics-and-assets.md).

### 176. Add Collider through the component picker

**Problem.** Collision editing should start from the same place as other object capabilities.

**Implementation.** Adapt `apps/editor/src/app/modules/editor/workspace/collider/collider-inspector.ts` and `apps/editor/src/app/modules/editor/workspace/inspector-panel/inspector-panel.html` to show a Collider card. Keep shape, local offset, bounds, blocking or sensor mode, and layer names in the existing editor. Link project collision rules to `apps/editor/src/app/modules/editor/workspace/physics-layers/physics-layers-dialog.ts`. Keep Capsule physics out of the picker until `crates/engine-core/src/collider.rs` and the contact, probe, controller, and overlay paths support the actual shape.

```ts
{ kind: 'collider', label: 'Collider', choices: ['box', 'sphere'] }
// Attach with the existing collider command and keep the model assignment separate.
```

**Verify.** Add and size a Box on a Plane so the collider top meets the visible surface. Play it as a floor, change it to a sensor, and confirm layer filtering. Attach a Box to a Capsule mesh and verify the Inspector describes the attached Box. Reload existing scene colliders without rewriting them.

**Resources.** [Collider authoring](../../docs/scene-editing.md#colliders-and-interaction-settings) · [Physics Layers milestones](authoring-roadmap-134-153.md).

### 177. Separate an object’s components from its role in a scene

**Problem.** A Camera or Player Controller can be attached to an object without being the active gameplay camera or player.

**Implementation.** Keep object options in `apps/editor/src/app/modules/editor/workspace/components/scene-component-inspector.ts` and `apps/editor/src/app/modules/editor/workspace/components/camera-component-draft.ts`. Show active Game Camera and assigned input as short role indicators. Route assignment actions to the existing Game Camera or Scene Information flows in `apps/editor/src/app/modules/editor/workspace/game-settings/camera-settings.html` and `apps/editor/src/app/modules/editor/workspace/scene-actions/scene-actions.html`. Apply the same distinction to Sky components and scene environment settings.

```ts
cameraCard = {
    attached: true,
    isGameCamera: scene.gameCameraKey === address.object,
    canAssign: capabilities.setGameCamera,
};
```

**Verify.** Create two cameras, designate one, and verify Editor Play and export use the assigned camera. Turn off an attached component and retain valid scene role handling. Editing the Scene view camera changes editor navigation only.

**Resources.** [Camera and input roles](../adr/0010-input-actions-and-possession.md) · [Editor and runtime view state](../../designs/editor-viewport/runtime-and-rendering.md#14-play-scene-view-and-standalone-behavior).

## Connect the specialist panes

### 178. Open specialist panes with the exact object and asset

**Problem.** Selecting a tab and opening a component editor are different actions today, and the destination can change while a draft review is open.

**Implementation.** Define the request in `apps/editor/src/app/core/editor/workspace/workspace-view.ts`. Route requests through `apps/editor/src/app/modules/editor/workspace/editor-workspace.ts`, `apps/editor/src/app/modules/editor/workspace/scene-viewport/workspace-view.service.ts`, and `apps/editor/src/app/modules/editor/workspace/workspace-draft-review.ts`. Revalidate generation, subject, component ID, and asset revision after any prompt. Keep the old view and focus on cancel or stale input. Transfer the native preview only after destination validation, and coordinate view review with the existing Play and project-exit paths.

```ts
type OpenPaneRequest = {pane: WorkspaceViewKind; subject: InspectorAddress;
    componentId?: string; assetId?: string};
const review = await reviews.reviewTransition(request);
if (review.accepted && stillResolves(request)) await panes.open(request);
```

**Verify.** Open each pane from a card. Cancel a dirty transition, fail a save, switch scene during review, and replace a project. The captured object opens only when it still belongs to the active document.

**Resources.** [Shared draft review](windows-release-step-1.md#1d-review-workspace-drafts-at-workflow-boundaries) · [Native and WebView composition](../../designs/editor-viewport/runtime-and-rendering.md#10-nativewebview-composition).

### 179. Make Animation the owner of clip and keyframe editing

**Problem.** Animation has a central workspace and a full inline component editor. The two entry points can open different targets.

**Implementation.** Keep clip binding and a short summary in `apps/editor/src/app/modules/editor/workspace/components/component-editor.html`. Move the inline editing tools into the existing `apps/editor/src/app/modules/editor/workspace/animation/animation-workspace.html` and `apps/editor/src/app/modules/editor/workspace/animation/animation-workspace.service.ts`. Reuse `apps/editor/src/app/modules/editor/workspace/animation/animation-workspace-target.ts` for Scene, Prop, and nested targets. Make **Open Animation** focus the bound clip and its timeline. Keep imported clip review, playback, scrub, keyframe edit, and Save reachable through the pane before retiring the inline form.

```ts
await openPane({pane: 'animation-workspace', subject,
    componentId: component.id, assetId: clip?.id});
```

**Verify.** From a Scene object and a nested Prop, open the correct clip, scrub, edit a key, save, and reopen it. Cancel an unsaved transition and verify the draft and current object stay selected.

**Resources.** [Animation workspace and draft milestones](editor-access-and-particles-154-162.md#154-show-animation-tools-as-soon-as-the-tab-opens) · [Graph asset scope](../adr/0007-graph-assets-and-execution.md).

### 180. Open Animator from its object card

**Problem.** Graph state editing and actor binding belong to different authorship scopes.

**Implementation.** Expose **Add Animator** through the shared catalog using `apps/editor/src/app/modules/editor/workspace/animator/animator-inspector.ts`. Keep controller asset and channel bindings on the card. Route **Open Animator** to `apps/editor/src/app/modules/editor/workspace/animator/animator-workspace.service.ts` with the selected actor and asset. Reuse `apps/editor/src/app/modules/editor/workspace/animator/animator-actor-binding.ts` for channel validation and fingerprint checks. Keep object assignments and graph edits in their existing document histories.

```ts
await openPane({pane: 'animation-controller', subject,
    componentId: animator.id, assetId: animator.controllerAssetId});
```

**Verify.** Two actors can reference the same Animator asset with different valid bindings. Graph changes save to the shared asset. Switching or undoing one actor’s binding leaves the other actor’s binding valid. Missing assets expose a repair action.

**Resources.** [Graphs and execution](../adr/0007-graph-assets-and-execution.md) · `apps/editor/src/app/modules/editor/workspace/animator/animator-workspace.spec.ts`.

### 181. Connect Player Controller, Controls Graph, and project inputs

**Problem.** The author needs to see how a player receives input and where that behavior is edited.

**Implementation.** Use `apps/editor/src/app/modules/editor/workspace/control-graph/control-graph-inspector.ts` and `apps/editor/src/app/modules/editor/workspace/control-graph/control-graph-workspace.service.ts` for graph assignments and editing. Use `apps/editor/src/app/modules/editor/workspace/game-input/input-actions-catalog.service.ts` for action asset selection. Keep input source, movement space, and local possession on the Player Controller card. Open **Controls Graph** with the assigned graph. Keep keyboard/controller layouts and their conditions in project Controls. Preserve the policy for who receives local input.

```ts
await openPane({pane: 'controls-graph', subject,
    componentId: player.id, assetId: binding.graphAssetId});
```

**Verify.** Assign an Input Actions asset and a Controls Graph. Edit the graph from the Player card, then Play and confirm action delivery. Changing a layout updates project settings. Reopening the card shows the saved object binding.

**Resources.** [Input actions and possession](../adr/0010-input-actions-and-possession.md) · [Conditional controls milestone design](../../designs/editor-viewport/conditional-controls-and-object-scripts.md).

### 182. Give Audio editing a focused workspace

**Problem.** Clip selection, audition, spatial behavior, and animation markers compete for space in the object Inspector.

**Implementation.** Add `apps/editor/src/app/modules/editor/workspace/audio-source/audio-workspace.ts` and register it with `apps/editor/src/app/core/editor/workspace/workspace-view.ts`. Reuse `apps/editor/src/app/modules/editor/workspace/audio-source/audio-source-inspector.ts`, `apps/editor/src/app/modules/editor/workspace/audio-source/audio-marker-fields.ts`, and `apps/editor/src/app/modules/editor/workspace/audio-source/audio-audition.service.ts`. Keep clip, gain, and on/off state in the compact card. Open the full pane for spatial settings, audition and marker routing. Resolve animation links by exact object, clip, and marker. Stop audition on context change, close, or Play.

```ts
await openAudio({subject, componentId: source.id, clipId: source.clipId});
audioAudition.stopFor(previousSubject);
```

**Verify.** Switch between two sources while auditioning and confirm only the current preview plays. Open the exact animation marker from a source, then save and reopen spatial settings. A linked source shows its actual editing permissions.

**Resources.** [Runtime and preview ownership](../../designs/editor-viewport/runtime-and-rendering.md#3-state-model) · `apps/editor/src/app/modules/editor/workspace/audio-source/audio-source.spec.ts`.

### 183. Route emitters into the existing Particle Designer

**Problem.** The Particle Designer already provides isolated authoring. Its entry points should behave the same across object creation and inspection.

**Implementation.** Make the Particle Emitter card in `apps/editor/src/app/modules/editor/workspace/components/component-editor.html` show the assigned preset and runtime summary. Route **Edit Particles** through `apps/editor/src/app/modules/editor/workspace/particle-designer/particle-designer-session.ts` and `apps/editor/src/app/modules/editor/workspace/particle-designer/particle-designer.ts`. Keep preview, draft review, and Apply in the Designer. Let the existing Create shortcut use the same route after the object is accepted by the engine.

```ts
await particleDesigner.open(subject.object, {componentId: emitter.id});
```

**Verify.** Select two emitters and switch between them. Save and cancel changes, then preview and export the saved emitter. Applicability for a linked or nested Prop follows its actual edit capability. Measure the Play viewport and exported Player with 0, 192, and 2,048 live particles. Record simulation, geometry preparation, and renderer wait times separately.

**Resources.** [Existing particle design and export](editor-access-and-particles-154-162.md) · `apps/headless/tests/fixtures/particle-workbench/project.json`.

### 184. Give scripts and behaviors one detailed editor

**Problem.** The object needs a clear behavior attachment, while schema fields, references, and conditions need room to edit.

**Implementation.** Add a focused editor in `apps/editor/src/app/modules/editor/workspace/components/behavior-workspace.ts`. Reuse `apps/editor/src/app/modules/editor/workspace/components/script-component-draft.ts`, `apps/editor/src/app/core/editor/object-scripts.models.ts`, and the existing script descriptors. Show assignment, status, and one **Edit Behavior** action on the card. Keep project action links and scene entity references explicit. Prop source fields use reference slots when the current contract requires them. Reuse the existing typed engine script runtime.

```ts
const target = subject.kind === 'prop' ? referenceSlot(slotId) : sceneEntity(entityKey);
await behaviors.bind(subject, scriptId, fieldId, target);
```

**Verify.** Bind an input-driven behavior in a scene and a typed reference slot in a Prop. A valid binding survives save and replay. Missing targets or unsupported configuration keep their errors and support repair.

**Resources.** [Typed scripts and behavior](../../designs/editor-viewport/conditional-controls-and-object-scripts.md) · `apps/headless/tests/conditional_authoring.rs`.

## Connect reusable Props and scene instances

### 185. Reuse the object Inspector inside Prop Workspace

**Problem.** Objects within a reusable Prop should feel familiar while retaining their own document and undo history.

**Implementation.** Connect the shared Inspector to `apps/editor/src/app/modules/editor/workspace/prop-workspace/prop-object-editor.ts`, `apps/editor/src/app/modules/editor/workspace/prop-workspace/prop-workspace.service.ts`, and `apps/editor/src/app/modules/editor/workspace/prop-workspace/prop-workspace.html`. Add a Prop source adapter that feeds the same identity, transform, and component cards with the source draft’s permissions. Expand component authoring in supported increments, starting with the existing audio and script support. Add native validation and a fixture before exposing any additional source component command.

```ts
switch (subject.kind) {
    case 'scene': return sceneEditing.applyComponent(subject, edit);
    case 'prop': return propDraft.applyComponent(subject, edit);
    case 'linked': return instanceOverrides.applySupported(subject, edit);
}
```

**Verify.** Select an object in a Prop and edit its transform and a supported component. Undo stays in the Prop. Leave the workspace and verify the Scene selection and scene history are still usable.

**Resources.** [Prop definitions and references](../adr/0009-prefabs-and-instance-overrides.md) · `apps/editor-host/src-tauri/tests/editor_workspace/prop_roundtrip.rs`.

### 186. Make nested Prop composition navigable

**Problem.** Editing a nested asset should retain the address of the child and show which document is active.

**Implementation.** Use the existing `PropAddress` in `apps/editor/src/app/core/editor/linked-prefabs.models.ts`. Build a stable breadcrumb and child-jump flow in `apps/editor/src/app/modules/editor/workspace/prop-workspace/nested-prop-editor.ts` and `apps/editor/src/app/modules/editor/workspace/prop-workspace/prop-workspace.service.ts`. Distinguish opening a child source from editing a placement record in the parent. Route nested authoring through existing guards in `apps/editor-host/src-tauri/src/prefab_sources/nested.rs`. Preserve stored placements and transforms.

```ts
await props.openNestedSource({placementPath, assetId, sourceRevision});
await props.returnTo(parentDraftId, selectedPlacement);
```

**Verify.** Open a three-level hierarchy, edit a child source and return to the parent. Matching names select the correct child. Cycles, stale pins and missing sources give a recoverable error with the current breadcrumb intact.

**Resources.** [Nesting and override lifecycle](../adr/0009-prefabs-and-instance-overrides.md#nesting-and-override-lifecycle) · `apps/editor-host/src-tauri/tests/editor_workspace/nested_prop.rs`.

### 187. Make linked-instance overrides explicit

**Problem.** A placed Prop can inherit properties and keep local edits. The Inspector needs to make that meaningful at the field level.

**Implementation.** Use `apps/editor/src/app/modules/editor/workspace/prefabs/linked-prefab-inspector.ts` and `apps/editor/src/app/modules/editor/workspace/prefabs/linked-prefab-inspector.html` to show inherited, overridden, and conflict states in the shared Inspector. Route **Edit Prop Source**, **Revert Override**, and **Review Update** to the existing source/instance operations in `apps/editor-host/src-tauri/src/prefab_sources/instances.rs` and its review module. Show only overrides allowed by the native property whitelist. Provide an edit-source action where a value comes from the reusable asset.

```ts
overrideRow = {
    address: propAddress,
    state: instanceOverrides.status(propAddress, propertyKind),
    canRevert: instanceOverrides.canRevert(propAddress, propertyKind),
};
```

**Verify.** Place two copies, override one and save. Update the shared source, review one instance, and undo that instance update. Correctly retain the other instance, source history, and unsupported-property rules.

**Resources.** [Prefab overrides](../adr/0009-prefabs-and-instance-overrides.md#nesting-and-override-lifecycle) · `apps/editor-host/src-tauri/tests/editor_workspace/prop_overrides.rs`.

### 188. Connect Assets, Hierarchy, and Prop Workspace in both directions

**Problem.** Authors need to find the reusable asset behind a placed object and get back to the original selection.

**Implementation.** Connect **Create Prop from Selection**, **Place Linked Prop**, **Edit Prop Source**, and **Show in Assets** across `apps/editor/src/app/modules/editor/workspace/assets-panel/assets-panel.ts`, `apps/editor/src/app/modules/editor/workspace/scene-entities-panel/scene-entities-panel.ts`, and `apps/editor/src/app/modules/editor/workspace/prop-workspace/prop-workspace.service.ts`. Carry asset ID and scene selection through the return route. Reuse copy and linked workflows. Retire a duplicate legacy entry only after its corresponding capabilities and draft guards are in the shared flow.

```ts
const prop = await props.createFromSelection(sceneAddress, selectedKeys);
await panes.open({pane: 'prop-workspace', subject, assetId: prop.id});
```

**Verify.** Create a reusable Prop from scene objects, place it as linked and as a copy, and return to the exact scene selection. Cancel creation with an existing asset name and verify the prior asset and scene are intact.

**Resources.** [Asset publication and dependency tracking](../adr/0006-asset-identity-and-import-pipeline.md) · [Prop sources and instances](../adr/0009-prefabs-and-instance-overrides.md).

## Complete the scene workflow and qualify it

### 189. Put scene and project settings in one discoverable place

**Problem.** Object components, scene lighting, and project physics rules have different scopes and currently share several unrelated entry points.

**Implementation.** Organize `apps/editor/src/app/modules/editor/workspace/scene-actions/scene-actions.html`, `apps/editor/src/app/modules/editor/workspace/scene-lights/scene-lights-dialog.ts`, and `apps/editor/src/app/modules/editor/workspace/physics-layers/physics-layers-dialog.ts` into named Scene and Project destinations. Use the existing scene lighting records in `crates/engine-core/src/editor/operations/scene_objects.rs`. Keep short lens or Sky options on object cards. Use explicit role commands for assigning the scene camera or environment. Existing light creation shortcuts open the appropriate scene-light context.

```ts
const placement = {target: 'scene-lighting', lightId};
await sceneSettings.open(placement);
```

**Verify.** Find Scene Lighting, active camera and environment from Scene settings, and collision layers and input layouts from Project settings. Make one change in each and verify Save Scene or Save All persists it in the intended document.

**Resources.** [Scene, project, and role ownership](../../docs/project-lifecycle.md) · [Existing Scene Lights workflow](editor-access-and-particles-154-162.md).

### 190. Build one playable workshop from a blank project

**Problem.** The refactor needs a complete author journey that shows whether the new entry points fit together.

**Implementation.** Add `apps/headless/tests/fixtures/primitive-workshop/project.json`, assets, and `route.replay.json`. Document a reproducible Editor sequence that creates a Plane floor, uses a Capsule mesh as the visible player, configures supported collision and Controls, places a reusable nested Prop, and animates an object that plays audio on an authored marker. Add one objective using a supported typed behavior. Use `apps/headless/tests/full_authoring_practice_yard.rs` as the pattern for source and package replays.

```ts
await create('plane');
await addComponent('collider', {shape: 'box'});
await openPane({pane: 'animation-workspace', subject: animatedObject});
```

**Verify.** Starting from blank, build the scene by Create, Add Component and card actions. Save, reopen, complete the objective, and restart. Replay the same packaged scene to check bindings, objective state, and visible object placement.

**Resources.** [Playable game creation workflow](../../designs/editor-viewport/game-creation-workflows.md) · `apps/headless/tests/full_authoring_practice_yard.rs`.

### 191. Verify keyboard access, focus, and responsiveness

**Problem.** A cleaner layout should also behave consistently with keyboard navigation, smaller windows, and large scenes.

**Implementation.** Extend `apps/editor/src/app/modules/editor/workspace/editor-workspace.editing.spec.ts`, `apps/editor/src/app/modules/editor/workspace/scene-entities-panel/scene-entities-panel.spec.ts`, and `apps/editor/src/app/modules/editor/workspace/workspace-tabs/workspace-tabs.ts`. Add checks for keyboard Create, component search, card navigation, focus restoration, and invalid drag destinations. Measure a representative large Hierarchy and object with many components against the baseline. Preserve scoped styles within the existing build budget and keep one native viewport/preview owner.

```ts
await keyboard.openCreate();
await keyboard.choose('Cube');
expect(activeInspectorSubject()).toEqual(createdObject);
```

**Verify.** Use mouse and keyboard at standard and compact window sizes. Verify a drop highlight does not shift the rows. Use the 12,000-row test in `apps/editor/src/app/modules/editor/workspace/scene-entities-panel/scene-entities-panel.spec.ts` to verify hierarchy projection rebuilds during rename, transform, and component edits. Use a scene of the same size to measure input-to-paint p95 in the desktop Editor. Record production style budgets and verify repeated pane switching retires old preview sessions.

**Resources.** [Interaction and responsiveness](../../designs/editor-viewport/scene-view-interactions.md) · [Performance and verification](../../docs/testing.md).

### 192. Qualify the connected workflow and retire superseded entry points

**Problem.** Removing duplicate UI is safe after the replacement routes and existing projects pass the same checks.

**Implementation.** Finish the scoped cleanup in `apps/editor/src/app/modules/editor/workspace/components/component-editor.html`, `apps/editor/src/app/modules/editor/workspace/prop-workspace/prop-workspace.html`, and `apps/editor/src/app/modules/editor/workspace/editor-workspace.ts`. Update the user walkthrough in `docs/windows-editor-quickstart.md`. Run compatibility cases for Cinder Oath, the full authoring practice yard, Conditional Door Lab, and Particle Workbench. Run the full aggregate baseline from `test_all.py`, including the native checks, with Cargo caches and temporary directories on D:. Use each test project's local target directory for Dashboard generation tests so their artifacts are in the staging tree. Build the Windows installer via `apps/editor-host/build-windows-release.ps1` and verify the packaged Editor UI and exported Player.

```powershell
python -B test_all.py --native
npm.cmd run check:production --prefix apps/editor
npm.cmd test --prefix docs-site
```

**Verify.** Every replacement card reaches its intended editor. Old projects open and export with preserved paths and links. Create a Blank project in the installed Editor, click and hold an object to drop it into a folder, then save, reopen, export, and launch the Player from an unrelated working directory. Check a clean Windows host using the documented release acceptance procedure and record which tests were native, automated, or manual.

**Resources.** [Windows Editor quickstart](../../docs/windows-editor-quickstart.md) · [Windows release refactors](windows-release-step-1.md) · [Testing and validation](../../docs/testing.md).

## Add the contextual Game Development Agent

Schedule this phase after Milestone 192. It uses the typed subject address from Milestone 171 and the scoped pane request from Milestone 178. Follow the [project-aware assistant and MCP proposal](../adr/0014-open-weight-assistant-and-mcp.md) for the shared native tool contract and separate inference worker. This first Agent profile combines live selection inspection and retrieval-augmented generation (RAG). Authors can ask about their selection, see references, and navigate to the object or component the answer addresses.

### 193. Switch the right drawer between Inspector and Agent

**Problem.** Authors need to ask about an object in the same space they use to inspect it. Switching tools should keep the object selected and the viewport usable.

**Implementation.** In `apps/editor/src/app/modules/editor/workspace/editor-workspace.ts` and `editor-workspace.html`, expose Inspector and Agent as two modes of one right drawer. Keep the switch, keyboard shortcut scope, splitter width, and focus restoration in the workspace shell. In `editor-workspace.scss`, reuse the existing Inspector sizing and the persisted width from `apps/editor/src/app/shared/services/layout-preferences.service.ts`. Route a switch away from a dirty editor through `apps/editor/src/app/modules/editor/workspace/workspace-draft-review.ts` and change the mode after the switch actually completes. Preserve the existing Runtime Inspector in Game view during Play and restore the authoring mode on return. Start with a shell for the future Agent view.

```ts
type RightDrawerMode = 'inspector' | 'agent';

readonly rightDrawerMode = signal<RightDrawerMode>('inspector');
readonly rightDrawerView = computed(() =>
  this.workspaceView.gameView() && this.editorService.playing()
    ? 'runtime-inspector'
    : this.rightDrawerMode(),
);
```

**Verify.** In `editor-workspace.editing.spec.ts` verify a switch with one viewport, one selected object, and the same drawer width. Verify focus via keyboard, Cancel with a dirty draft, and Play → Stop. Scene mutation history stays unchanged.

**Resources.** [Editor workspace](../../docs/editor-workspace.md) · [Native viewport composition](../adr/0012-native-viewport-composition.md) · Milestones 171 and 178.

### 194. Give the Agent the exact selected object or Prop

**Problem.** Display names repeat across scenes and nested Props. The Agent needs the same stable address as Inspector and the detailed editor it might open.

**Implementation.** Add `apps/editor/src/app/modules/editor/workspace/agent-drawer/agent-selection-context.ts`. Project the authoritative selection from `apps/editor/src/app/modules/editor/workspace/inspector-panel/inspector-subject.ts`; for a nested or linked Prop, reuse `apps/editor/src/app/core/editor/linked-prefabs.models.ts` and its complete `PropAddress`. Capture the resolved `InspectorSubject` from Milestone 171, including the address, both workspace generations, and the authored or draft revision. Include a display label. Check the editor-side generation before delivery and send the native generation and revisions when resolving current values. Include component ID on a request about a particular component. Bind this context to the request at submission. The drawer follows selection by default. A Pin control can retain the current object within the same project, with a visible label; clear the pin on project switch and mark removed entities unavailable.

```ts
interface AgentSelectionContext {
  readonly subject: InspectorSubject;
  readonly label: string;
  readonly componentId?: string;
}
```

**Verify.** Add `agent-selection-context.spec.ts` beside the adapter. Verify two same-named scene objects, two copies of a nested Prop, a Prop source draft, and an object removed while an answer is pending. Render each response with the label captured for that request. Clearing or changing selection does not redirect a response.

**Resources.** [Editor workspace](../../docs/editor-workspace.md) · [Authoring, Play, and editor state](../adr/0004-authoring-play-and-editor-state.md) · Milestones 171 and 185–187.

### 195. Resolve selected objects through a typed native read service

**Problem.** The Agent needs current values and supported behavior from the real editor session. Retrieval or a name lookup cannot resolve the live selection.

**Implementation.** Add `apps/editor-host/src-tauri/src/commands/editor/agent_context.rs` and declare the submodule in `apps/editor-host/src-tauri/src/commands/editor.rs`. Register the trusted editor command in `apps/editor-host/src-tauri/src/lib.rs`. Implement the shared Rust read service under `apps/editor-host/src-tauri/src/editor/agent_context.rs` and declare it in `apps/editor-host/src-tauri/src/editor.rs`. Use existing workspace and Prop resolvers to validate the native workspace generation, expected workspace revision, content revision, and address. Keep the read contract immutable so a query preserves selection and preview state. For an unsaved Prop draft, resolve an immutable draft snapshot tied to the current session and its revision. Return a bounded summary with typed component IDs and editor capabilities. Use authored state for this first integration. Map stale, missing, and unsupported targets to distinct response types.

```rust
let summary = workspace.inspect_authoring_subject(
    request.workspace_generation,
    request.revision,
    request.content_revision,
    &request.address,
    request.component_id.as_ref(),
)?;
```

**Verify.** Add `apps/editor-host/src-tauri/tests/editor_workspace/agent_context.rs` to the workspace tests. Verify valid scene, draft, and nested instance cases. Reject stale generations and revisions, mismatched components, and IDs from a different project. Verify that reads preserve revision, undo, saved scene data, selection, and active preview tokens.

**Resources.** [Editor command pipeline](../adr/0001-editor-command-pipeline.md) · [Rust–TypeScript bridge](../adr/0003-rust-typescript-bridge.md) · [Assistant and MCP proposal](../adr/0014-open-weight-assistant-and-mcp.md).

### 196. Define the Game Development Agent bundle and its tool contract

**Problem.** Model instructions, available tools, and retrieval sources need a shared definition so the drawer and future agent integrations expose the same capabilities.

**Implementation.** Add a runtime manifest at `agent-profiles/game-development-support/bundle.json` alongside the existing profile. Use `.agents/skills/game-development-support/SKILL.md` as the source for maintained workflow guidance. Start with the editor guidance profile and capability identifiers for selection inspection, asset lookup, and knowledge search. Generate the callable schemas from registered typed tools. Validate the manifest against that registry in a new `crates/authoring-agent/src/bundle.rs` and add the crate to `Cargo.toml`. Follow ADR-0014’s shared-tool boundary so the built-in client and future MCP adapter can call the same services. The first profile receives inspection grants only.

```json
{
  "id": "game-development",
  "version": 1,
  "profile": "editor-guidance",
  "tools": ["editor.inspect", "assets.query", "knowledge.search"],
  "corpora": ["engine-help", "active-project"]
}
```

**Verify.** Validate the bundle at load. Missing tools, unknown manifest versions, and tools outside the profile grant produce a clear diagnostic. Verify the published tool descriptions come from the registered schema and that ordinary editor startup works with the Agent disabled.

**Resources.** [Game development support skill](../../designs/editor-viewport/README.md) · [Assistant and MCP proposal](../adr/0014-open-weight-assistant-and-mcp.md) · [Authoring, Play, and editor state](../adr/0004-authoring-play-and-editor-state.md).

### 197. Build a versioned knowledge index

**Problem.** Retrieval needs a reliable set of help and project sources that can be refreshed independently of the model.

**Implementation.** Add `crates/authoring-agent/src/knowledge/corpus.rs` and `index.rs` with a manifest of approved sources. Start with version-matched engine help, public authoring contracts, and explicitly included project metadata such as Prop assets, script declarations, and gameplay settings. Split reference text by headings and store each chunk’s source ID, heading, content hash, and compatibility metadata. Keep one shared engine corpus per engine version and scope each project corpus by project identity. Refresh affected entries after saves and asset events, and remove obsolete entries. Keep live object state on the service from Milestone 195. Store the index in a project-approved location with development artifacts on D: for this checkout.

```rust
struct KnowledgeChunk {
    source: SourceId,
    heading: String,
    ordinal: u32,
    content_hash: ContentHash,
    compatibility: SourceCompatibility,
    text: String,
}
```

**Verify.** Add corpus fixtures in `crates/authoring-agent/tests/corpus.rs`. Verify deterministic chunk IDs, refresh on edit, removal on delete, and independence of two projects. Verify the allowlist omits credentials, generated build output, other projects, and sources resolving outside approved roots.

**Resources.** [Assistant and MCP proposal](../adr/0014-open-weight-assistant-and-mcp.md) · [Asset identity and import pipeline](../adr/0006-asset-identity-and-import-pipeline.md) · [Testing and validation](../../docs/testing.md).

### 198. Return cited results from exact and keyword lookup

**Problem.** Authors often use exact object and component terms, while engine help and project metadata can contain older versions of the same term.

**Implementation.** Add `crates/authoring-agent/src/knowledge/lexical.rs` and a bounded `knowledge.search` tool in `crates/authoring-agent/src/tools.rs`. Resolve exact current asset IDs with the scoped asset service. Search approved headings, identifiers, and text with a deterministic keyword ranker. Filter candidates by project grant and engine or schema compatibility before ranking. Return source references, matched headings, excerpts, and revision information. Keep exact live matches separate from the guide excerpts so an answer can cite each accurately.

```rust
let exact_asset = scoped_assets.find_exact(&query, &scope)?;
let reference_hits = lexical.search(&query, &scope, limit)?;
let results = KnowledgeResults { exact_asset, reference_hits };
```

**Verify.** Add `crates/authoring-agent/tests/lexical_search.rs`. Query a known asset ID and a plain phrase about enabling gravity. Verify compatible sources win, stale entries are excluded, an unknown ID gets an empty exact match, and an incomplete phrase can still return a relevant reference.

**Resources.** [Asset identity and import pipeline](../adr/0006-asset-identity-and-import-pipeline.md) · [Assistant and MCP proposal](../adr/0014-open-weight-assistant-and-mcp.md) · Milestone 197.

### 199. Add local vector search and combine it with keyword results

**Problem.** Authors may describe a behavior in their own words. Semantic retrieval should improve those searches while preserving exact lookup and offline access to the keyword index.

**Implementation.** Add `crates/authoring-agent/src/knowledge/embeddings.rs` and `hybrid.rs`. Define a replaceable local embedding provider and cache vectors by model identity and revision plus chunk content hash. Start with a bounded cosine ranking over compatible, project-scoped candidates. Combine reference rankings with reciprocal rank fusion, keep exact asset lookups from Milestone 198, and return lexical results while vectors are unavailable or being rebuilt. Re-embed changed chunks and invalidate vectors on provider or dimension changes without rebuilding the lexical index. Choose the provider and any later vector index after measuring latency, disk use, and retrieval quality on representative projects.

```rust
let lexical_hits = lexical.search(query, scope, candidate_limit)?;
let semantic_hits = vectors.search_if_ready(query, scope, candidate_limit)?;
let reference_hits = reciprocal_rank_fusion(
    &lexical_hits,
    &semantic_hits,
    result_limit,
);
```

**Verify.** Add `crates/authoring-agent/tests/hybrid_search.rs` with deterministic fake embeddings. Verify that a paraphrase finds the right guidance, an exact asset ID stays visible, model swaps preserve keyword results, and out-of-scope vectors never enter rankings. Record whether the ranked results improve a fixed question set.

**Resources.** [Assistant and MCP proposal](../adr/0014-open-weight-assistant-and-mcp.md) · Milestones 197–198.

### 200. Stream Agent answers in the drawer

**Problem.** The Agent needs a cancellable conversation flow so model or retrieval latency stays outside editor input, rendering, and simulation.

**Implementation.** Add `apps/editor/src/app/modules/editor/workspace/agent-drawer/agent-drawer.ts`, `agent-drawer.html`, and `agent-drawer.service.ts`. Mount the view in the slot from Milestone 193. Add `crates/authoring-agent/src/session.rs` and `apps/editor-host/src-tauri/src/commands/editor/agent_session.rs`, registered in `lib.rs`. Add `apps/authoring-agent/src/main.rs` as a separate local worker and register the app in `Cargo.toml`. Keep inference and retrieval in that worker. Use the typed service from Milestone 195 for active state and the same tool contract for a future authenticated MCP adapter. Pass an immutable request context, bounded tools and source results, and a cancellation token. Stream responses keyed by request ID. Cancel active streams and discard late delivery on project close, provide explicit Stop in the drawer, and expose keyword results when the configured model is unavailable. Keep model installation and model selection explicit. Extend `apps/editor-host/build-windows-release.ps1` to build the worker with the packaged target architecture and stage the worker, bundle manifest, and engine help under `release/agent/` for both portable and installed builds. Verify their copies alongside the Player resources. Resolve the installed worker and knowledge paths through the Editor resource root.

```ts
const request = {
  id: crypto.randomUUID(),
  question,
  selection: this.selectionForSubmission(),
};
this.agentDrawerService.start(request);
```

**Verify.** Add `agent-drawer.service.spec.ts` beside the service and use a fake streaming provider. Verify normal completion, Stop, project switch, changing selection mid-response, and an unavailable model. Verify the staged worker and help resolve under the portable layout. Smoke-test orbit, hierarchy selection, and Play while a request is pending.

**Resources.** [Assistant and MCP proposal](../adr/0014-open-weight-assistant-and-mcp.md) · [Native viewport composition](../adr/0012-native-viewport-composition.md) · Milestones 193–199.

### 201. Show sources and navigate to the addressed object

**Problem.** An answer should let authors check where a recommendation came from and open the exact object or component it discusses.

**Implementation.** Add `apps/editor/src/app/modules/editor/workspace/agent-drawer/agent-answer-actions.ts` and render citation and action rows in `agent-drawer.html`. Present citations from verified tool results. Wire Show Object and Open Component Editor to the selection resolver and the scoped pane request from Milestone 178. Revalidate the captured generation, address, and component ID on click and use the normal draft review. Keep suggested operations typed and allowlisted. Treat retrieved excerpts as reference data. When a source, object, or component has changed, show a refresh action and explain what can still be opened.

```ts
type AgentAnswerAction =
  | { kind: 'show-object'; target: AgentSelectionContext }
  | { kind: 'open-component-editor'; target: AgentSelectionContext;
      componentId: string };
```

**Verify.** Add `agent-answer-actions.spec.ts` alongside the adapter. Verify an answer about a nested Prop opens that exact instance and component even if another object is now selected. A stale, removed, or cross-project target reports its status. An instruction embedded in a retrieval excerpt cannot add actions or grant capabilities.

**Resources.** [Assistant and MCP proposal](../adr/0014-open-weight-assistant-and-mcp.md) · [Editor workspace](../../docs/editor-workspace.md) · Milestones 178, 187, and 194–195.

### 202. Verify the contextual Agent in the portable and installed Editor

**Problem.** The future Agent needs an end-to-end acceptance case that proves drawer swapping, exact selection, retrieval, and offline behavior together.

**Implementation.** Add `apps/headless/tests/fixtures/agent-context-workshop/project.json` with two scenes containing same-named objects, a nested linked Prop, and identifiable component IDs. Add `crates/authoring-agent/tests/authoring_workflow.rs` for the scripted selection and retrieval cases, and record the interactive package procedure in `docs/testing.md`. Exercise both package layouts from `apps/editor-host/build-windows-release.ps1` on D:. Launch each Editor from a working directory outside the installation and without the source checkout on PATH, and verify the bundled worker, manifest, and engine help resolve. Use a deterministic provider for CI and one explicitly configured local provider for packaged verification. Compare source relevance, response cancellation, indexing disk use, and editor responsiveness.

```rust
assert_eq!(answer.subject.address, selected_nested_address);
assert_eq!(answer.project, active_project_id);
assert_eq!(after_query.revision, before_query.revision);
assert_eq!(after_query.undo_depth, before_query.undo_depth);
```

**Verify.** Swap Inspector and Agent on a selected object, follow a cited answer into Animation, and repeat with a pinned nested Prop in a different scene. Confirm no query changes the project, no answers or index records leak across a project switch, and the Inspector and keyword help remain usable offline. Record packaged verification and performance alongside the guide.

**Resources.** [Testing and validation](../../docs/testing.md) · [Windows Editor quickstart](../../docs/windows-editor-quickstart.md) · [Assistant and MCP proposal](../adr/0014-open-weight-assistant-and-mcp.md) · Milestone 192.
