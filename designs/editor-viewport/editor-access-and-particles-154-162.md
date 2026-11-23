# Editor access and particles, Milestones 154–162

The Editor already has an Animation tab. It renders a large preview first and puts the animation tools below it. The new
layout gives those tools the main space immediately after opening the tab. The Objects menu provides one entry point for
common scene objects, lights and catalogued assets. Particle Designer is a separate window inside the Editor, with its
own preview and a draft saved through the scene's usual undo history.

| Where you start                                                                             | What opens                                                                                                                                                               |
|---------------------------------------------------------------------------------------------|--------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| **Animation** tab or an existing **Open in Animation** action                               | The Animation workspace, with clip tools at the top and a smaller isolated preview beside them on wider layouts. Choose **Authored Clips → Open Timeline** to edit keys. |
| **Objects → Props…** or **Models…**                                                         | The project's Assets browser, filtered to the relevant type and ready for the existing placement flow                                                                    |
| **Objects → Empty Object**, **Camera**, **Player Controller**, **Audio Source**, or **Sky** | A new scene object, using the normal create and undo flow                                                                                                                |
| **Objects → Point Light** or **Directional Light**                                          | A new entry in the scene's lights, whose properties can be managed through **Objects → Edit Lights…**                                                                    |
| **Objects → Particle Emitter** or **Edit Particles…** in the Inspector                      | Particle Designer with an isolated preview, presets, settings, and Apply                                                                                                 |

## 154. Show Animation tools as soon as the tab opens

**Problem.** The tab opens correctly but the oversized preview pushes the actual editing tools below the fold. In
`apps/editor/src/app/modules/editor/workspace/editor-workspace.html`, share the existing native viewport owner. In
`apps/editor/src/app/modules/editor/workspace/animation/animation-workspace-layout.scss`, lay out the Animation pane as
tools plus adjacent preview. Expose a compact preview on narrow layouts, below the first usable controls. Hide unrelated
surrounding scene panels while Animation is active.

```css
.animation-layout {
    grid-template-columns: minmax(0, 1fr) minmax(0, 0.75fr);
}
```

**Verify.** Both the tab and **Open in Animation** reveal controls from the top. Switching away restores the normal
Scene arrangement. Resize the workspace and check that the active tab and first controls are visible.

Resources [Workspace arrangement](scene-view-interactions.md#5-workspace-arrangement) · [Windows composition](runtime-and-rendering.md#10-nativewebview-composition).

## 155. Keep unfinished Animation edits safe when navigating

**Problem.** A dirty timeline can be removed when the workspace or clip source changes. Put a shared review on the
existing timeline draft in `apps/editor/src/app/modules/editor/workspace/animation/` and use it from tab changes,
Imported Clips, Play, and project/window exit. Keep the existing draft if the user keeps editing or applying fails.

```ts
if (await timeline.reviewNavigation()) {
    workspace.activate(nextView);
}
```

**Verify.** **Save and Close Timeline** saves the draft, **Discard and Close Timeline** drops it, and **Keep Editing**
stays in Animation. A rejected save or stale target must leave the draft visible. Clean navigation should require no
confirmation.

Resources [Draft transactions](runtime-and-rendering.md#9-scene-mutation-and-gesture-transactions) · [Project history](../../docs/project-lifecycle.md#documents-switching-and-history).

## 156. Add the Objects menu

**Problem.** Creating objects is scattered across Hierarchy, Assets, and individual settings. Add an **Objects**
dropdown next to the existing top menus in `components/editor-header/`. Follow the current catalog workflow for Props
and Models. Reuse existing camera, player, audio, and sky commands. Add native `create_empty` and `create_light`
commands in `crates/engine-core/src/editor/operations/`. Lights remain per-scene lighting records, and the menu should
give them an editing entry point.

```ts
{label: 'Props…', action: 'browse_props'}
{label: 'Point Light', action: 'create_point_light'}
```

**Verify.** Keyboard users can open the dropdown and return focus after selection. Each command respects edit mode and
revision ownership. Every new record is undoable. Props and models still use the existing asset placement path.

Resources [Asset placement](scene-view-interactions.md#27-asset-drag-and-drop-contract) · [Scene editing](../../docs/scene-editing.md).

## 157. Store a Particle Emitter on any scene object

**Problem.** Particles need persistent settings that survive saving, duplication, and Prop use. Extend the typed
component records in `crates/engine-core/src/components/`, `scene_format.rs`, and the linked Prop component rules. Add a
validated scene schema version and a typed `create_particle_emitter` command that selects a non-mesh object with an
enabled emitter. In Angular, expose an Inspector component record and the **Edit Particles…** action.

```ts
interface ParticleEmitterSettings {
    readonly rate_per_second: number;
    readonly lifetime_seconds: number;
    readonly speed: number;
    readonly spread_degrees: number;
    readonly gravity: number;
    readonly size: number;
    readonly color: readonly [number, number, number];
    readonly seed: number;
}
```

**Verify.** Round-trip a scene with an emitter, open old scenes, reject nonfinite/out-of-range settings, and undo create
or edit. Saved JSON has no preview or live-particle state. Schema 8 also covers Particle Emitters and their overrides
retained in linked Prop source records.

Resources [Object composition](../../docs/adr/0002-object-composition-and-components.md) · [Bridge contracts](../../docs/adr/0003-rust-typescript-bridge.md).

## 158. Simulate emitters at the fixed step

**Problem.** A preview or game must have stable emission, independent of how frames are grouped. Implement emission and
motion in `crates/engine-core/src/particles.rs` and advance it from `crates/engine-core/src/simulation_session.rs`. Use
the saved seed and launch from the emitter’s world position in a cone around local +Y. Parent and object rotations steer
the cone. Transform scale affects placement and leaves the launch cone and speed unchanged. Apply world vertical gravity
and expire particles at their configured lifetime. Existing particles stay in world space as the emitter moves. Publish
only committed state, clear on scene change or restart, and leave authored objects untouched.

```rust
if emitter_fraction >= 1.0 && live_particles < particle_budget {
    emit_at_world_transform(emitter, seed_and_sequence);
}
```

**Verify.** The same authored seed and input produce the same positions and counts with single and grouped fixed steps.
A paused or rejected step publishes no particles. Check that nonuniform and zero scale preserve the cone. Disabled
components never emit, and long-running scenes stay within budget.

Resources [Runtime scheduling](../../docs/adr/0005-runtime-lifecycle-and-scheduling.md) · [State ownership](runtime-and-rendering.md#3-state-model).

## 159. Render the same particles in the Editor and Player

**Problem.** A designer-only preview would leave the saved effect invisible during Play. Send the bounded presentation
snapshot through `crates/runtime/` to shared renderer code. Use the same camera-facing colored quads from
`apps/game/src/scene_rendering.rs` and the native Editor draw path. Particles should use scene depth and never enter
scene selection or collision.

```rust
draw_particles(camera, simulation.particle_snapshot());
```

**Verify.** A bright emitter appears and animates in Editor Play and a packaged Player. Scene geometry can hide
particles behind it. Stop and restart clear the live population. Export requires no external sprite or model for the
starter effects.

Resources [Native rendering](runtime-and-rendering.md#1-architectural-decision) · [Player/export behavior](../../docs/project-lifecycle.md#build-and-playerheadless-behavior).

## 160. Give Particle Designer its own window

**Problem.** Editing an effect by changing Inspector numbers leaves the result hard to see. Add a floating, expandable
Editor dialog in `apps/editor/src/app/modules/editor/workspace/particle-designer/`, using the shared dialog's focus and
native viewport suppression. Put the animated preview and play/pause/restart on one side and the preset picker and
settings on the other, with a compact single-column mode. Start with **Embers**, **Fountain**, and **Sparks**.

```ts
const previewState = createPreview(draft.seed);
```

**Verify.** Each preset visibly changes the effect. The preview never edits the scene. The first editable controls, the
preview controls, and Apply remain available at a small window size.

Resources [Modal behavior](runtime-and-rendering.md#102-modal-behavior) · [Accessibility and discoverability](scene-view-interactions.md#35-accessibility-and-discoverability).

## 161. Make Apply a single, predictable scene edit

**Problem.** A designer window can outlive the object or scene it opened from. Bind the draft to its workspace
generation, scene, object, component, and original settings. Recheck the target in `particle-designer` and apply one
revisioned `replace_component` through the editor command queue. Preserve the draft on a conflict and provide Apply,
Discard, and Keep Editing for close review.

```ts
if (currentComponentMatchesDraftTarget()) {
    await replaceComponentAtCurrentRevision();
}
```

**Verify.** Cancel leaves authored settings alone. Apply and Undo change one component once. A project switch, object
deletion, or concurrent edit cannot apply the draft to another object.

Resources [Bridge contracts](../../docs/adr/0003-rust-typescript-bridge.md) · [Edit and recovery behavior](scene-view-interactions.md#37-failure-and-recovery-matrix).

## 162. Verify the complete workflow

**Problem.** The Designer, Editor navigation, simulation, and Player each have a different failure mode. Add focused
frontend and native tests where the old tests do not cover the new behavior. Exercise this path on a saved scene.

```text
Objects → Particle Emitter → Particle Designer → Fountain → Apply → Close
→ Play → Stop → Undo → Play → Stop → Save All → Export → Player
```

**Verify.** Run the editor tests and production build, native simulation and rendering checks, and the documentation
site. Record whether the Windows Player launched and whether the visual checks were automated or observed directly.

Resources [Testing guide](../../docs/testing.md) · [Build and Player](../../docs/project-lifecycle.md#build-and-playerheadless-behavior).

## Try the included sample

Copy the folder `apps/headless/tests/fixtures/particle-workbench/`. Open its `project.json` through **File → Open
Project…**. Press **Play** to see Embers and Fountain. Stop and select an emitter in the Hierarchy, then open **Edit
Particles…** to change it. The sample's README includes the full edit, undo, and export walkthrough.
