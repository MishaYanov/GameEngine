# Game creation workflows and support agent

Status: proposed product design. The capability map reflects the working tree inspected on 2026-11-15.
The later [Milestone 4–6 review](../../docs/milestones-4-6-review.md) records implemented component Inspector,
linked prefab and runtime lifecycle workflows; proposed capabilities below retain their historical design scope.
Audience: Engine Developer.
This chapter defines the game a usable viewport should help someone finish.
It complements the interaction and rendering chapters rather than specifying a second editor architecture.
Examples are proposed authoring flows unless explicitly marked as present in source.
An available Rust API does not mean its complete authoring interface exists.
No performance target in this chapter is a measured result.

## 1. Product outcome

The immediate outcome is a small game created, edited, played, saved, and exported with this engine.
The viewport earns its features by making that loop clearer or faster.
A beautiful static scene alone does not prove that the game creation loop works.
A feature should remove a demonstrated interruption in that loop.
The first complete game is also a practical acceptance fixture for the editor.
Use that fixture repeatedly instead of inventing a new unrelated demo for each subsystem.
Keep the game small enough that one person can understand its full authored structure.
Keep representative complexity in reusable objects, retained scene edits, and input routing.
Do not expand the milestone into an open-world renderer, complete visual scripting system, or marketplace.

Unity's level-design guidance treats greyboxing as arranging simple geometry to explore layout before final art.
Its ProBuilder tooling supports rapid prototyping and level construction.
The relevant lesson is to shorten the route from an idea to a playable space.
It does not require this engine to reproduce ProBuilder's mesh editor before making a game.
[Unity level-design guidance](https://unity.com/blog/games/e-book-for-level-designers), [ProBuilder overview](https://docs.unity3d.com/Packages/com.unity.probuilder@6.0/manual/index.html).

## 2. The reference game: Courtyard Run

The bundled Courtyard Run is a short two-scene challenge. The player moves a capsule to a yellow key sphere,
returns to a green exit and enters Finish. A blue finish marker completes the run. Key pickup, the exit and
completion use proximity. Restart returns the player to the Courtyard spawn with fresh progress. The layouts
use reusable capsule, cube and sphere glTF props. The separate Robot Demo scene remains in the catalog.
[Run the reference game](../../docs/apps/game/README.md#demo-progress-and-new-game).

### 2.1 Game brief

| Item                   | Reference fixture                                                      |
|------------------------|------------------------------------------------------------------------|
| Working title          | Courtyard Run                                                          |
| Player goal            | Acquire a key, reach the courtyard exit and approach the finish marker |
| Primary action         | Move through a readable space                                          |
| Restart                | Restart Run in the editor or F10 in the native Player                  |
| Completion state       | A visible finish state with a restart action                           |
| Scene count            | Two small scenes                                                       |
| Art requirement        | Capsule, cube and sphere props with role-specific colors               |
| Required authoring     | Place, align, duplicate, inspect, save, and play entities              |
| Required runtime proof | Objective, transition, finish, and restart in the player               |
| Deliberately deferred  | Combat, inventory UI, networking, terrain painting, and cinematics     |

### 2.2 Playable route

1. Spawn in Courtyard and check the locked exit to the left.
2. Move to the key on the right to collect it and unlock the exit.
3. Return to the exit to load Finish, then release movement before heading to the marker.
4. Approach the finish marker on the right and observe completion and frozen movement.
5. Restart to restore the Courtyard spawn and starting progress.

### 2.3 Why this fixture exercises the editor

The opening tests measurements, grid snapping, and camera framing.
The landmark tests selection among overlapping geometry.
The repeated modules test duplication, alignment, and parent-child transforms.
The key tests a visible runtime state change that is not an authored edit.
The exit tests stable role references and scene identifiers.
The second scene tests retained authoring drafts and startup-scene configuration.
The finish state tests runtime progress ownership.
The restart tests the difference between restarting a scene and restarting a run.
The export tests that the authored game does not depend on editor-only geometry.

## 3. Current capability map

The following records current source capabilities. Record native acceptance separately.
The main wire definitions are in `apps/editor/src/app/core/editor/editor.models.ts`.
The configured starter is in `apps/game/project.json`.
Native application-specific progress is demonstrated in `crates/cinder-oath-gameplay/src/lib.rs`.

| Capability                           | Inspected state                                                              | Consequence for the next work                                |
|--------------------------------------|------------------------------------------------------------------------------|--------------------------------------------------------------|
| Authoring project creation           | Angular and host flow present                                                | Start from a real project folder                             |
| Scene catalog                        | Project data and editor UI present                                           | Use stable scene IDs                                         |
| Retained scene edits                 | Rust workspace support present                                               | Switching scenes should preserve drafts                      |
| Entity name and transform            | Inspector and commands present                                               | Numeric editing is the fallback for every gizmo              |
| Entity parent key                    | Virtual Hierarchy, drag/explicit keep-world reparent and typed commands       | Unsupported shear/singular conversions fail explicitly       |
| Model catalog                        | Explicit discovery and selection present                                     | Existing project models can be placed                        |
| Models from external folders         | No complete authoring flow established here                                  | Do not claim arbitrary file import is solved                 |
| Duplicate and delete                 | Toolbar and Rust commands present                                            | Use undoable structural operations                           |
| Grid visualization                   | Native analytic grid and editor-only controls implemented                     | Display spacing remains independent from authoring snap      |
| Pointer transform gizmos             | Move/Rotate/Scale, grouped preview/commit/cancel implemented                   | Share Rust history and validation with Inspector             |
| Local and world transforms           | Command support present                                                      | Presentation and mixed selection rules need design           |
| Materials and colliders              | Collider Inspector, sensor/filter editing and selected Scene outlines present | General material/light/component authoring remains open      |
| Prefab data operations               | APIs exist                                                                   | Linked overrides and a complete prefab UI are future work    |
| Controlled movement and spin         | Configured behavior types present                                            | Reuse these before creating a scripting framework            |
| Gameplay roles                       | Module catalog supplies default/per-scene role descriptors to Gameplay Roles  | Generic projects do not invent Courtyard roles               |
| Typed game progress                  | Key pickup, exit and completion with status and run reset                    | Observe objective and score during Play                      |
| Scene transitions                    | Authored exit requests `finish` in editor, Player and headless Play          | Observe the runtime scene separately from authored selection |
| Animation and shader workspace kinds | Type definitions exist                                                       | Do not display them as finished editors                      |
| Standalone export                    | Staging, shared module preflight, compilation, retry and launch flow present | Correct failed role/scene diagnostics before compiling       |

### 3.1 Authored roles and module objectives

`CampaignProgress` contains score, key ownership, exit reached and completion.
`CampaignFactory` composes configured behavior with application-specific progress logic.
Gameplay Roles uses the selected module's role descriptors and writes persistent scene bindings. Courtyard advertises
Player/Key/Exit and Player/Finish in its finish scene. The module uses them for
the key, exit and finish proximity checks. The editor shows status and Restart Run in Game.
[Role configuration](../../docs/project-lifecycle.md#gameplay-role-bindings).

### 3.2 Editor and player must select the same gameplay module

The bundled demo selects `courtyard_run` through the manifest. The Tauri editor,
packaged player, and headless runner resolve that selection through the same native
module. Generic projects use configured movement and spin. Read the
[manifest contract](../../docs/project-lifecycle.md#gameplay-module-selection).

The module runs the reference game's objective, exit and finish rules. Desktop acceptance
exercises acquire, transition, finish and restart in editor Play and the packaged player.

## 4. Authoring data and runtime state

The authoring project describes initial scenes, assets, and behavior configuration.
An editor session adds selection, tools, camera preferences, drafts, and history.
A play session instantiates runtime state from authored data.
A game run can retain typed progress across scene transitions.
A packaged player executes the authored game without editor overlays.
These lifetimes should be visible in the interface when they affect the user's decision.
They should not require users to understand transport details or Rust ownership.

### 4.1 Ownership table

| Information | Owner | Saved in scene files? | Reset rule |
| --- | --- | --- | --- |
| Entity transform at authoring time | Scene document | Yes | Reload/discard or undo |
| Editor navigation pose | Editor viewport state | No | User preference or view reset |
| Game camera pose | Authored scene camera or game module | Yes when authored | New runtime session |
| Selected entities | Editor session | No | Scene/project transition rules |
| Active transform tool | Editor workspace | No | Workspace preference |
| Temporary drag preview | Current gesture | No | Drop, cancel, or focus loss |
| Runtime player position | Simulation world | No | Scene/run restart |
| Courtyard key ownership | Typed game progress | No authoring write | Defined run restart |
| Build input snapshot | Staged export | Separate output | New export preparation |

### 4.2 Product language

Use `Scene` for a spatial authoring document.
Use `Game` for the running player's camera output.
Use `Hierarchy` once the panel actually exposes parent-child structure.
Use `Scene Entities` for the current flat entity list until that change is implemented.
Use `Inspector` for properties of the current selection.
Use `Assets` for project-owned source resources and reusable authored assets.
Use `Project Settings` for settings shared by the game.
Use `Editor Preferences` for controls that only affect this person's editing environment.
Use `Export Game` for the operation that writes a standalone playable folder.
Use `Run Exported Game` for launching that separate executable.
Avoid presenting a backend type name as a product category without a user-facing purpose.

## 5. Workflow A: create a project and understand the scene

The user opens New Project from the File menu or launch screen.
They choose a name and a destination folder.
Creation writes the authoring project and starter assets.
Creation does not compile or launch a player.
The editor opens the new manifest after successful creation.
An existing dirty project is replaced only through the existing reviewed-revision/discard flow.
If opening fails, the created project path remains available for retry.
The interface must not ask the user to create the same folder again.

### 5.1 First visible scene

The viewport starts in Scene mode.
Show a quiet ground grid and orientation aid.
Frame the starter content instead of displaying an empty horizon by accident.
The active scene name remains readable above the viewport.
The Hierarchy or Scene Entities panel contains recognizable names.
The Inspector begins with a useful selection hint.
The Assets drawer exposes the project's scenes and model catalog.
The primary action bar includes Save All, history actions, and Play.
Secondary creation and diagnostic controls stay out of the first-view center.

### 5.2 Orientation checklist

- Can the user identify the ground plane?
- Can they tell which object is selected?
- Can they frame an object that is outside the current view?
- Can they distinguish the editor camera from the game camera?
- Can they return to a known view without changing scene data?
- Can they find the active scene and its saved state?
- Can they start Play and clearly stop it?
- Can they reopen the project without relying on a repository sample path?

### 5.3 Useful empty state

An empty scene still shows a grid, orientation, and creation entry points.
It should not look like a failed renderer.
Display a compact hint such as `Drag a model from Assets, or add a blockout shape`.
Hide that hint once it obstructs useful content.
A native renderer failure is a distinct error state with a retry or diagnostic route.
A missing project is a distinct state with Create and Open actions.
Do not combine all three conditions into one generic blank panel.

## 6. Workflow B: block out a playable route

Start by choosing the player footprint and typical movement speed.
Treat one scene unit as a documented project convention for the reference game.
Do not claim the engine currently enforces meters in its transform contract.
For Courtyard Run, adopt a one-unit block module while assessing the first layout.
Show this convention in the sample's authoring notes or game settings.
Changing the display grid should not rescale existing entities.
Changing a snap increment should not move anything until the next edit requests snapping.

### 6.1 Minimum blockout kit

| Module | Use | Initial authoring need |
| --- | --- | --- |
| Floor tile | Establish walkable footprint | Place and duplicate |
| Wall segment | Shape a route | Move, rotate, snap |
| Door opening | Check character clearance | Compare dimensions |
| Ramp | Exercise vertical layout | Inspect local/world transforms |
| Pillar | Create occlusion and landmarks | Duplicate consistently |
| Goal marker | Make the route legible | Distinct material or icon |
| Hazard marker | Communicate a rule | Separate appearance from collision |

### 6.2 Geometry scope

The first blockout kit can consist of ordinary imported reusable models.
A simple built-in cube or plane is a useful later creation tool.
Do not require vertex editing, UV editing, boolean modeling, or procedural terrain for this milestone.
If primitives are added, their size and pivot conventions must be consistent with imported assets.
Give a new primitive a sensible name and select it immediately.
One creation gesture should produce one undoable scene operation.
Deleting a blockout object should not delete its source model asset.
Replacing its model should not silently remove its game role or collision settings.

### 6.3 Placement sequence

1. Choose a floor tile from the blockout kit.
2. Place it on the visible grid plane.
3. Read the position and scale in Inspector.
4. Duplicate it into a short path.
5. Add walls on each side of one section.
6. Use top view to align the route.
7. Return to perspective and check the route at player height.
8. Save the scene.
9. Play through the route.
10. Revise the geometry based on the movement result.

### 6.4 Measurement behavior

Display selection dimensions as an optional overlay rather than permanently covering the object.
Report dimensions in the chosen project convention.
Distinguish local scale values from world-space bounding dimensions.
A scaled parent can change a child's world size without changing its local scale.
The measurement overlay should explain that distinction through labels.
Avoid rounding a displayed value so aggressively that adjacent modules appear aligned when they are not.
Numeric Inspector editing remains available for exact placement.
Grid snapping remains an aid, not the only path to precision.

### 6.5 Layout questions for the support agent

Is the intended route visible from the starting point?
Can the player pass through the narrowest opening at the chosen footprint?
Does the game camera show enough of the next decision?
Can the user distinguish a decorative object from an interactive landmark?
Does the second scene reuse what the first scene already taught?
Is a confusing result caused by layout, input, camera, collision, or unclear feedback?
Fix the responsible layer before adding another abstraction.

## 7. Workflow C: place and arrange reusable assets

Assets should feel like resources the user can place, inspect, and reuse.
The current catalog can enumerate project-owned scenes and models.
The current Add to Scene action places a model at the scene origin.
Viewport drag placement is proposed work with a preview and an explicit drop commit.
Arbitrary external-file ingestion remains separate from dragging an already cataloged asset.
The first drag implementation should use stable asset identifiers or validated relative paths.
It should not copy a large asset payload for every pointer move.

### 7.1 Asset-to-scene drag

Dragging begins from an identified model entry.
The preview follows a valid placement surface or the selected construction plane.
Show the preview with a distinct visual treatment.
The preview is not yet a normal selectable authored entity.
Show an invalid cursor and a brief reason when no valid drop target exists.
On drop, submit one placement operation with the final transform.
Select the created instance after the backend accepts it.
Canceling the drag leaves the document and undo history unchanged.
Keep Add to Scene as the keyboard-accessible fallback.

### 7.2 Placement options

| Option | Proposed first behavior |
| --- | --- |
| Grid plane | Use the current visible construction plane |
| Surface placement | Opt-in surface hit with explicit alignment policy |
| Snapping | Apply the selected translation increment |
| Rotation | Preserve asset default; do not guess a surface orientation silently |
| Scale | Preserve the asset's authored instance default |
| Parent | Use the scene root unless a specific target was chosen |
| Multi-primitive model | Preserve the imported model structure promised by the importer |
| Repeated placement | A later tool mode, not an accidental sticky drag state |

### 7.3 Asset identity and source editing

A placed entity references a project asset rather than embedding a filesystem absolute path.
Renaming an entity changes its scene label, not the model file name.
Deleting an entity removes the instance, not the source resource.
Replacing a source model may affect multiple instances and should be presented accordingly.
An import refresh should not reorder the user's hierarchy arbitrarily.
Missing sources should have a clear placeholder and path information.
Do not erase the entity's transform because its model is temporarily unavailable.
Keep source-file operations distinct from instance placement operations.

### 7.4 Prefabs are more than duplicate

Unity prefabs store configured objects as reusable assets and create instances from those templates.
Its system also includes instance overrides and specialized editing flows.
The engine's existing prefab serialization APIs are useful foundations, but do not establish those complete semantics.
The first reusable-object feature should explicitly choose whether it is a copied template or a linked instance.
Do not use the same icon and name for both without explaining the difference.
[Unity prefab documentation](https://docs.unity.com/en-us/engine/6000.0/manual/working-with-gameobjects/prefabs).

### 7.5 First template milestone

Support saving a selected group as a reusable asset when its references are valid.
Support placing another copy and preserving the group's internal relationships.
Use new instance entity keys.
Preserve explicit role bindings only when their scope and conflict rules permit it.
Make invalid external references visible before saving the template.
Call the operation `Save as Template` if linked update semantics are not yet implemented.
Reserve `Apply Overrides` and `Revert Overrides` for a later linked-instance feature.

## 8. Workflow D: organize a scene without losing spatial intent

Parent-child relationships need a visible hierarchy when scenes grow beyond a few entities.
The current flat panel should evolve into a real Hierarchy rather than gaining a decorative tree arrow.
The tree reflects actual parent relationships from the authoritative scene document.
Search should find an entity without requiring every ancestor to be manually expanded.
Selection must stay synchronized between hierarchy, viewport, and Inspector.
The tree should remain usable when the selected entity has many descendants.

Unity's Hierarchy represents scene objects and supports creating and organizing them.
Its parent-child structure is a useful reference for a spatial authoring workflow.
Our first implementation should focus on accurate relationships and predictable reparenting.
It need not inherit every multi-scene or prefab interaction at once.
[Unity Hierarchy documentation](https://docs.unity.com/en-us/engine/6000.0/manual/unity-editor/editor-windows-views-reference/hierarchy-window/hierarchy).

### 8.1 Proposed scene groups

| Group | Suggested contents | Runtime meaning |
| --- | --- | --- |
| Environment | Floor, walls, landmarks | Organizational until components say otherwise |
| Gameplay | Player, key, exit, hazards | Explicit roles and behavior configuration |
| Presentation | Camera and lights | Authored game presentation |
| Debug | Temporary authoring aids | Excluded from shipping only through explicit rules |

### 8.2 Reparenting contract

Dragging an entity onto another entity proposes a parent relationship.
Dragging between rows proposes an ordering operation only if ordering is actually supported.
Do not disguise one operation as the other.
Keep world transform by default for a spatial reparenting gesture.
Offer local-transform preservation through an explicit alternate action.
Reject self-parenting and descendant cycles before committing.
Show an invalid drop indication without corrupting the current tree.
One completed reparent gesture creates one history entry.
Canceling restores the original hierarchy and transforms.

### 8.3 Large-tree behavior

Keep selection state keyed by persistent entity keys.
Do not base identity on row indexes or visible names.
Use virtualization when measurements justify it for the targeted scene sizes.
An expanded tree should not force the side pane beyond its configured width limit.
Scrolling a tree should not orbit the viewport behind it.
Dragging near a tree edge may auto-scroll the tree, with an explicit speed limit.
Keyboard reparenting or a parent picker should provide an alternative to pointer precision.
The Inspector should expose the parent reference when that helps resolve a spatial mistake.

## 9. Workflow E: configure the player and game camera

The reference game needs a defined player role, not merely the first renderable entity.
Bind behavior to stable entity keys or validated authored role references.
The project currently supports shared controlled-movement behavior definitions.
Reuse that route for a first player configuration interface.
Do not add a second movement implementation solely for the editor preview.
The runtime and exported player should consume the same movement semantics.

### 9.1 Player authoring fields

| Field | Purpose | Proposed validation |
| --- | --- | --- |
| Controlled entity | Identify the player instance | Existing key in the target scene |
| Movement definition | Reuse shared behavior settings | Valid behavior definition ID |
| Speed | Tune traversal | Finite value within the behavior's supported domain |
| Movement space | Explain local or world movement | Explicit supported enum |
| Spawn transform | Define a readable start | Valid authored transform |
| Collision shape | Approximate the occupied space | Valid collider configuration |
| Input profile | Map player actions to inputs | Explicit profile and target selection |

### 9.2 Camera distinction

Orbiting Scene view does not change the game's authored camera.
Framing an entity is an editor navigation action.
Use a separate explicit action to align the game camera with the current Scene view.
That action changes authored data and participates in undo.
Show a game-camera frustum or preview when enabled.
Hide game-camera guides during a clean Game view unless requested for debugging.
Provide a direct way to select the game camera's authored settings.
Do not make the viewport's current perspective an implicit packaging input.

### 9.3 Input focus

Scene navigation shortcuts only apply when the Scene viewport owns the appropriate focus.
Text input takes precedence over tool shortcuts.
Game movement only applies to the intended play target.
Changing focus to Inspector must not leave a movement key held.
Stopping Play must release runtime input and pointer capture.
Opening a system dialog must not continue a captured drag behind the dialog.
The support agent should diagnose input ownership before recommending a new input framework.

## 10. Workflow F: collision and interaction

Rendering a wall and assigning a collision shape are distinct tasks.
A collision debug outline should be an optional editor overlay.
The current collider DTOs include box and sphere shapes.
The current Inspector authors those shapes, offsets, dimensions, sensor flags and unsigned layer/mask bits.
The optional Colliders Scene overlay shows selected effective shapes with dashed sensor and continuous blocking
outlines. Both participants' layer/mask directions must match; sensors overlap without blocking motion.
This is implemented overlap classification. Feature 1.4 Enter/Stay/Exit delivery remains unimplemented.
[Current review and limits](../../docs/building-blocks-review.md#collider-inspector-and-sensorfilter-authoring).
Collision query or debug-shape support is not proof of a complete character controller.
The reference game must define which interactions require collision response and which require overlap detection.
A visible goal marker can initially use an explicit trigger rule rather than a general physics simulation.
Avoid claiming gravity, rigid bodies, slopes, or continuous collision detection until implemented and verified.

### 10.1 First interaction set

- Block movement at an intended solid boundary only if response semantics exist.
- Detect entry into a key region once per acquisition.
- Detect an eligible player entering an exit region.
- Require the key state before the exit can complete.
- Produce feedback when the exit is unavailable.
- Keep visual appearance and interaction state synchronized through runtime state.
- Restore initial runtime interaction state on a defined restart.

### 10.2 Authoring feedback

Use a collision overlay to show the effective world-space shape.
Distinguish an enabled collider from a hidden or unsupported shape.
Changing scale should update the preview according to the collider's documented transform semantics.
Do not infer a precise collision mesh from a complex rendered model unless the importer supplies one.
A trigger region needs a different label or style from a solid obstacle.
Color alone should not carry that difference.
The game camera's clean view should not contain debug collision geometry in the exported player.

## 11. Workflow G: objective, scene transition, finish, restart

Game progress belongs to the running game session.
The authored scene defines initial data and references.
The reference game needs an explicit progression contract before adding a visual objective graph.
Use the existing typed progress and game-session foundations where they fit.
Do not serialize arbitrary runtime score updates into the scene document.
Do not make editor selection the source of the player's identity.

### 11.1 Proposed progress model

| State | Meaning | Reset scope |
| --- | --- | --- |
| Has key | First objective satisfied | Full run restart |
| Current checkpoint | Current recovery location | Policy defined by the game |
| Completed | Finish condition reached | Full run restart |
| Score | Optional count or reward | Full run restart unless explicitly retained |

### 11.2 Transition contract

An eligible exit requests a transition to a stable scene ID.
The game session prepares the destination before replacing the active runtime scene.
An invalid destination leaves the current scene playable and reports an actionable error.
Progress retention follows the game-session policy.
Editor-only navigation state does not become game progress.
The runtime scene switch should be observable in Game view and in the standalone player.
The editor should not silently change the authored startup scene when the player traverses an exit.

### 11.3 Restart choices

`Restart Scene` restores the current scene's initial runtime state using the game's progress-retention policy.
`Restart Run` creates the defined new game run and clears the corresponding progress.
`Stop Play` returns to authored editor state.
These are different operations and should not share one ambiguous `Reset` label.
The first release can expose only Restart Run if that covers the game's needs.
Do not add an unused checkpoint system just to support a menu item.
The finish screen should make the supported restart operation obvious.

## 12. Workflow H: inspect components, scripts, and graphs

The Inspector should eventually present editable capabilities rather than a growing list of unrelated fields.
The present Inspector edits name, local transform and collider shape/interaction settings. Module/role/input
configuration is exposed through separate workspace controls. Material, light, camera and behavior property authoring
can build on typed commands as separate features.
Support reusable property controls for numeric vectors, references, enums, and validation.
Keep each property's label aligned with the runtime meaning.
Show read-only values as read-only instead of rendering inert controls that resemble editors.

### 12.1 Component composition

An entity's presence in a scene does not require every possible component.
Only show applicable sections.
Adding or removing a capability should be an explicit undoable action.
Shared behavior settings belong to project-level definitions when reused across scenes.
Per-instance overrides require explicit semantics before the UI offers them.
Refer to the existing object-composition ADR when changing the schema.
An accepted design direction is not evidence that every component type is implemented.

### 12.2 Script experience

A custom gameplay module needs a clear source location, configuration interface, and error boundary.
The first support workflow can use native Rust modules already supported by the engine.
That is different from hot-reloadable arbitrary user scripts.
Do not promise immediate script recompilation without a measured and implemented reload path.
Preserve the authored scene when a module fails to prepare.
Show module errors near the user's operation as well as any existing diagnostics surface.
Keep script compilation and scene editing responsiveness as separately measured paths.

### 12.3 Graph experience

Animation controllers and shader graphs need their own workspace view kinds.
Their type definitions should not cause empty tabs to appear in a shipping workflow.
Open a graph only when the asset type has a working editor.
The graph document has its own selection and history context.
Switching back to Scene restores the scene's navigation and tool state.
Graph compilation feedback should identify the responsible node and operation.
Do not put graph implementation details into ordinary scene creation dialogs.

## 13. Workflow I: play and return to authoring

The user presses Play from a valid authored project.
Finish or cancel the current Inspector or viewport gesture before entering Play.
Create runtime state from the authored documents.
Make the active mode visually clear without making text difficult to read.
Route game input to the running view.
Keep Stop reachable through the pointer and keyboard.
Stopping returns to authored state and releases captured input.
The user's Scene navigation pose should remain useful on return.

Unity's Game view displays output from application cameras.
Its Play mode runs the project for inspection, and temporary Play changes reset when Play ends.
This supports a clear distinction between authoring and running the game.
Our design should keep that distinction while using this engine's own session and progress contracts.
[Unity Game view and Play mode](https://docs.unity.com/en-us/engine/6000.0/manual/unity-editor/editor-windows-views-reference/game-view).

### 13.1 Play acceptance route

1. Save the authored player transform.
2. Start Play.
3. Move the player away from the spawn.
4. Acquire the key.
5. Transition scenes.
6. Complete the run.
7. Restart the run.
8. Stop Play.
9. Confirm the authored player transform is unchanged.
10. Continue editing without recreating the viewport window.

## 14. Workflow J: export a game that runs on its own

Save All persists dirty scene documents and project settings.
Export validates a saved project and prepares an output snapshot.
Staging freezes the authoring assets before background player compilation.
After staging acknowledges success, the user can continue editing and saving.
The completed output includes a player executable, manifest, and required assets.
Run Exported Game launches that output explicitly.
The built game does not need the Angular editor or its layout preferences.
The output still requires its supported operating system, graphics driver, and runtime prerequisites.

### 14.1 Export checks from the user's perspective

- The output belongs to the project they intended to export.
- Unsaved edits are clearly identified before export preparation.
- The selected destination is visible after the operation.
- A failed build reports whether a staged output was retained.
- Retry uses the intended staged inputs rather than silently exporting newer edits.
- A later export does not overwrite an unrelated folder.
- Run launches the output shown in the interface.
- The first release check includes execution outside the repository.

### 14.2 Required practical evidence

A successful Cargo process proves compilation, not playable behavior.
A copied executable proves installation, not driver compatibility.
A unit test using a fixture executable proves a packaging boundary, not Vulkan presentation.
A native desktop smoke check should cover the editor's focus, resize, input, and viewport behavior.
A separate standalone check should cover objectives, transitions, finish, and restart.
A supported second-machine check is a release criterion rather than an assumed property.
Keep those verification levels separate in progress reports.

## 15. Proposed next implementation sequence

This is a viewport-centered sequence for the next feature work.
It assumes the existing selection, numeric Inspector, and export workflow remains available.
Each task has a user-visible outcome and a prerequisite.
Tasks are compact review boundaries, not promises that they are equal in duration.
Complete the needed functional slice before requesting broad optimization work.

### Step 1: separate editor navigation state

Prerequisite: inspect the current camera and command ownership.
Outcome: Scene navigation can move without dirtying the authored scene.
Store the editor pose separately from the authored game camera.
Retain a stable per-scene navigation identity.
Keep Frame Selection working through the new state owner.
Acceptance: orbit and frame do not change a saved scene's authored camera.

### Step 2: make an empty viewport useful

Prerequisite: an editor camera with a valid projection.
Outcome: the user sees ground, orientation, and a useful start view.
Render an editor-only grid and origin axes.
Provide clear empty-scene guidance.
Keep rendering errors distinct from empty content.
Acceptance: a new empty scene can be understood without placing a model first.

### Step 3: add camera navigation gestures

Prerequisite: separated editor camera state.
Outcome: orbit, pan, and dolly respond predictably.
Handle capture, focus loss, and Escape consistently.
Offer visible controls for users without a middle mouse button.
Honor the input ownership rules of the interaction chapter.
Acceptance: navigating never accidentally transforms a selected object.

### Step 4: add orthographic inspection

Prerequisite: camera navigation and projection state.
Outcome: top, front, and side views support precise layout.
Expose a readable projection state and return to perspective.
Keep the construction plane coherent with the chosen view.
Preserve the visible target when changing projections.
Acceptance: a floor module can be aligned in top view and inspected in perspective.

### Step 5: expose grid and snap settings

Prerequisite: grid rendering and existing snap command semantics.
Outcome: the user controls visibility, plane, and increments.
Keep display spacing separate from the applied transform increment.
Offer useful defaults for the one-unit blockout kit.
Show a clear snapping state during a gesture.
Acceptance: disabling the grid does not unexpectedly disable numeric precision.

### Step 6: connect viewport selection

Prerequisite: camera coordinates and existing pick commands.
Outcome: clicking an entity selects the same entity as the side panel.
Respect tool hit-test priority before scene selection.
Keep selection changes out of transform history unless the domain explicitly requires otherwise.
Provide a useful fallback for occluded or tiny entities through the hierarchy.
Acceptance: selection survives renaming and does not depend on row order.

### Step 7: add selection feedback

Prerequisite: authoritative selection and renderer overlay ownership.
Outcome: selected entities are visible against varied scene materials.
Distinguish hover, primary selection, and group membership.
Use shape and state cues in addition to color.
Keep selection guides out of the player's clean output.
Acceptance: the selected robot remains identifiable over the ground grid.

### Step 8: implement the move tool

Prerequisite: selection, overlay hit testing, and grouped transform edits.
Outcome: axis and plane handles move an entity with live feedback.
Use the same local/world transform semantics as Inspector.
Commit one history entry per completed drag.
Cancel restores the starting transform.
Acceptance: a wall can be moved one grid interval and undone once.

### Step 9: implement the rotate tool

Prerequisite: the shared transform gesture lifecycle.
Outcome: rotation rings support clear constrained rotation.
Use degrees in the user interface and radians at the existing command boundary.
Keep local and world axes explicit.
Apply configured angular snapping only when requested.
Acceptance: a wall turns by a quarter rotation without translating its chosen pivot.

### Step 10: implement the scale tool

Prerequisite: transform gestures and defined pivot behavior.
Outcome: axis and uniform scaling are predictable.
Preserve backend validation and document zero/negative scale behavior.
Do not introduce an arbitrary UI-only minimum incompatible with valid authored data.
Keep dimensions readable through Inspector.
Acceptance: a floor tile can become a platform and return through one undo.

### Step 11: add group selection and pivots

Prerequisite: single-entity tools are stable.
Outcome: multiple entities can be arranged together.
Define primary entity, group center, and local-axis basis.
Avoid applying an ancestor transform twice to selected descendants.
Show mixed Inspector values without inventing a common value.
Acceptance: moving a selected parent and child does not double the child's displacement.

### Step 12: preview model placement

Prerequisite: the catalog, construction plane, and surface queries.
Outcome: dragging a model shows where it will be placed.
Keep the preview outside authored entity state until drop.
Support canceling across the native/WebView boundary.
Retain Add to Scene as a complete fallback.
Acceptance: a canceled drag changes neither document nor history.

### Step 13: commit model drops

Prerequisite: a valid placement preview and importer contract.
Outcome: drop creates a selected model instance at the final transform.
Submit a single placement intent rather than one spawn per pointer update.
Preserve importer structure and asset references.
Report failures without leaving a broken preview gesture.
Acceptance: one drop produces one undoable creation operation.

### Step 14: expose a small blockout kit

Prerequisite: reliable placement and transforms.
Outcome: a route can be assembled without external modeling work.
Use reusable models first or add only the primitive support the fixture needs.
Document pivots and scale conventions.
Provide readable initial entity names.
Acceptance: the courtyard route can be built from the supplied kit.

### Step 15: expose a real hierarchy

Prerequisite: authoritative parent references and stable selection.
Outcome: nested scene structures can be inspected and selected.
Support expand, collapse, search, and keyboard navigation.
Preserve the current side-pane size constraints.
Keep the flat-panel naming until this structure is actually available.
Acceptance: a deeply nested selected entity can be located and framed.

### Step 16: support reparenting

Prerequisite: a real hierarchy and existing reparent semantics.
Outcome: groups can be reorganized without accidental spatial movement.
Default to preserving world transform for spatial authoring.
Reject cycles and unsupported targets before commit.
Provide a non-drag parent picker.
Acceptance: reparenting a landmark preserves its visible world pose.

### Step 17: make gameplay roles inspectable

The Gameplay Roles controls implement assignment and clearing for Player, Key, Exit and Finish.

Prerequisite: the reference game's stable role configuration.
Outcome: the user can see which entity is the player, key, and exit.
Reuse existing shared behavior definitions where possible.
Avoid a general scripting framework before the game needs it.
Show missing references as actionable validation.
Acceptance: changing the player instance does not require searching unrelated source files.

### Step 18: finish the reference game's progress loop

The shared module implements the route, Game status and Restart Run. The Player uses its title and F10.

Prerequisite: roles, movement, and a defined interaction rule.
Outcome: acquire, transition, finish, and restart work consistently.
Use typed game progress and explicit scene IDs.
Keep runtime progress separate from authoring saves.
Implement only the objective UI or module configuration that this game requires.
Acceptance: the complete loop works in Play and in the standalone player.

### Step 19: expose focused diagnostics

Prerequisite: a playable loop and observable viewport events.
Outcome: the team can locate actual responsiveness problems.
Measure input-to-present separately from compilation and file loading.
Use representative scene fixtures rather than one empty viewport.
Label target budgets as targets until measured.
Acceptance: a slow gesture can be attributed to input, commands, rendering, or loading.

### Step 20: complete the first game delivery check

Prerequisite: the full reference loop and a saved project.
Outcome: someone can create, edit, play, export, run, finish, and restart.
Check persistence after closing and reopening the project.
Check the output outside the repository on a supported machine.
Record remaining limitations with evidence.
Acceptance: the next roadmap is driven by observed game-creation friction.

## 16. Practical acceptance scenarios

These scenarios describe behavior to verify after the corresponding feature exists.
They do not assert that the current application has passed native acceptance.
Use the same scene data across repeat runs where possible.

| Scenario | Expected result | Evidence to retain |
| --- | --- | --- |
| Create a project | Manifest and assets open in editor | Actual created path and reopened project |
| Navigate an empty scene | Grid and orientation remain understandable | Native interaction observation |
| Dolly toward a small prop | Focus remains controllable near the target | Camera pose and visible result |
| Switch to top view | The construction plane and axes are clear | View/projection state |
| Move with snapping | Final transform matches the requested increment | Authoritative transform and rendered frame |
| Cancel a transform | Starting transform returns with no committed edit | History and scene data |
| Select an occluded entity in Hierarchy | Inspector and viewport agree | Stable selected key |
| Drag a model from Assets | One instance appears at the previewed placement | Creation command and resulting entity |
| Cancel a model drag | No authored instance remains | Document and history unchanged |
| Reparent a group | Defined pose policy is preserved | Parent keys and world matrices |
| Edit two scenes | Switching retains both drafts | Dirty-state and saved-file evidence |
| Start and stop Play | Authored scene remains unchanged | Before/after authored document |
| Complete a game run | Objective and transition are consistent | Runtime progress and active scene |
| Restart the run | Defined initial game state returns | New progress/session state |
| Save while export compiles | Staged export remains consistent | Staged hashes or copied data comparison |
| Run exported game | The shown output launches explicitly | Output path and actual native execution |

## 17. Responsiveness as a game-creation requirement

Judge feedback through what the user is trying to accomplish.
Camera movement needs immediate visible response.
A drag needs a stable preview and a visible final commit.
The Inspector must not apply old values after a newer gesture has finished.
A heavy model import needs progress without freezing unrelated navigation.
A script compile needs an honest state and a retained authoring document on failure.
An export needs a consistent snapshot without occupying the editing queue throughout compilation.
Treat these as distinct paths with different instrumentation.

### 17.1 Measurement fixtures

Use a small scene for correctness and a representative larger scene for sustained interaction.
Include repeated models, nested transforms, and overlapping selectable content.
Include a few heavier assets to expose load behavior.
Keep the fixture deterministic enough to compare before and after an optimization.
Count entities, visible instances, triangles, and active behaviors separately.
Do not describe one entity count as a universal performance guarantee.
Record hardware, viewport size, projection, build mode, and rendering settings with results.
Separate cold opening, warm opening, active editing, and Play measurements.

### 17.2 Optimization decision

First locate the layer responsible for the observed delay.
Prefer reducing unnecessary full-scene work before replacing the UI framework.
Prefer coalescing stale gesture work before raising command-queue limits.
Prefer validating a focused cache before adding a new authoring file format.
Retest the same user task after the change.
Keep an optimization only when it improves the measured path without harming correctness.
Do not delay the reference game's completion for unrelated broad infrastructure work.

## 18. The game development support agent

The support agent's purpose is to help turn engine capability into a finishable game.
It also helps ensure that viewport implementation serves real authoring tasks.
It is not an automatic publisher, committer, or substitute for user decisions about the game.
Its reusable profile is supplied as the `game-development-support` skill.
The active collaboration agent can be asked to review later feature choices during this work.
The saved skill provides the persistent guidance for future sessions.

### 18.1 Inputs worth collecting

- The current playable goal and the next user-visible obstacle.
- The project and scene being edited.
- Whether the problem occurs in Scene, Play, or the standalone player.
- The authoritative data or command involved.
- The shortest reproduction the user can describe or the agent can inspect.
- Existing measured evidence, clearly separated from expectations.
- The implementation scope the user has actually authorized.

### 18.2 Decisions it should improve

Choose the smallest feature that advances a complete creation or play loop.
Distinguish a camera problem from a transform problem.
Distinguish a model import issue from an entity placement issue.
Distinguish authoring state from runtime state and game progress.
Prefer existing typed commands and reusable components when they fit.
Recommend new schema or infrastructure only when a concrete capability requires it.
Identify what should remain game-specific instead of entering engine-core.
Use current primary documentation for uncertain external engine comparisons.

### 18.3 Output style

Lead with the user-visible result.
Explain which current capability is reused and which new capability is needed.
Use a compact sequence scaled to the feature instead of a fixed arbitrary step count.
When teaching, explain one action, its location, and its observable result at a time.
When implementing, preserve the user's work and complete the authorized slice.
When reviewing, distinguish inspected source, automated checks, native interaction, and measured performance.
Suggest a commit name when requested, but do not create commits without explicit authorization.
Update maintained documentation only when requested under the repository's documentation rules.

### 18.4 Useful invocation examples

`Use $game-development-support to turn the courtyard route into the next playable milestone.`
`Use $game-development-support to review whether this viewport feature helps level creation.`
`Use $game-development-support to diagnose why Scene feels responsive but Play input lags.`
`Use $game-development-support to teach me how to author the key and exit roles.`
`Use $game-development-support to compare a native module with a future graph authoring flow.`
`Use $game-development-support to prepare the first game export acceptance checklist.`

## 19. Source and interpretation notes

The Unity references establish selected interaction and workflow concepts, not equivalence between products.
The proposed game, milestone order, UI language, and agent workflow are original recommendations for this engine.
Current source observations refer to the working tree inspected during this design task.
Reinspect relevant code before implementing a later phase.
No claim of feature completion should be inferred from a proposed workflow.

- [Unity level-design guidance](https://unity.com/blog/games/e-book-for-level-designers): greyboxing and playable layout exploration.
- [ProBuilder overview](https://docs.unity3d.com/Packages/com.unity.probuilder@6.0/manual/index.html): prototype geometry and level construction tools.
- [Unity grid snapping](https://docs.unity.com/en-us/engine/6000.0/manual/working-with-scenes/scenes-manage-gameobjects/grid-snapping): grid customization and alignment concepts.
- [Unity Hierarchy](https://docs.unity.com/en-us/engine/6000.0/manual/unity-editor/editor-windows-views-reference/hierarchy-window/hierarchy): scene object organization.
- [Unity prefabs](https://docs.unity.com/en-us/engine/6000.0/manual/working-with-gameobjects/prefabs): reusable configured object assets and related editing topics.
- [Unity Game view](https://docs.unity.com/en-us/engine/6000.0/manual/unity-editor/editor-windows-views-reference/game-view): application-camera output and temporary Play state.

## 20. Design completion criteria

The chapter is useful when an implementer can explain the user's next authoring task.
The interaction chapter supplies the exact viewport gestures and state transitions.
The rendering chapter supplies the ownership and presentation boundaries.
The reference game supplies a concrete reason to implement those mechanisms.
The support profile preserves these priorities without hard-coding one game's rules into every task.
The final proof is a completed small game created with the editor and run outside it.
