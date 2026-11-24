# Vulkan scene editor design

Design status: researched on 6 November 2026; the first native authoring milestone
is implemented. The [implementation guide](../../docs/viewport-implementation.md)
records the shipped scope, contracts, verification and remaining boundaries.
This design remains the broader roadmap; requirements below are not all completed.

The [2026-11-20 building-blocks review](../../history/reviews/building-blocks-review.md) records the completed playable slice,
module-aware role configuration, Collider Inspector and sensor/filter authoring. Trigger lifecycle events remain a
teaching proposal; the researched chapters below must not be read as proof that every proposed feature is available.

The next editor milestone is a usable level-building workspace: navigate a scene,
place an asset, select it, transform it, undo a mistake, and play the result.
The scene viewport should feel immediate while the surrounding editor supports
complex Inspectors, asset browsers, graphs, and project workflows.

The recommended architecture keeps Angular and Tauri for application chrome and
Rust with Vulkan for the scene, spatial tools, and interactive feedback.
Camera navigation, grid presentation, and tool preferences belong to the editor.
Authored entities, gameplay cameras, and project settings belong to game data.

## Read the design

| Chapter | Purpose |
| --- | --- |
| This overview | Product decisions, visual structure, priorities, and delivery boundaries. |
| [Scene view interactions](scene-view-interactions.md) | Navigation, grid, selection, transform tools, asset placement, and input behavior. |
| [Runtime and rendering](runtime-and-rendering.md) | State ownership, native rendering, bridge contracts, grid/gizmo implementation, and responsiveness. |
| [Game creation workflows](game-creation-workflows.md) | Concrete game-building tasks, feature dependencies, and support-agent responsibilities. |
| [Object authoring roadmap, Milestones 163–202](../../history/roadmaps/object-authoring-roadmap-163-202.md) | Create, Inspector and specialist panes, Prop authoring, and the future contextual Agent. |

The interactive preview supplied with this design demonstrates the proposed
layout and gestures. It is a local concept simulation, not a Vulkan renderer
or evidence that the proposed production tools have been implemented.

Existing implementation details below refer to the combined working tree.
They are distinct from committed features and from proposed design requirements.

## Product outcome

The author should be able to block out a small courtyard game without editing
entity positions in a text file or manually guessing where a model will appear.

The first successful session should include these actions:

1. Open the project and recognize the active scene.
2. See a world grid and understand which direction is up.
3. Orbit around an object without modifying the game camera.
4. Pan and zoom to the area where a prop belongs.
5. Drag a model from Assets and see a placement preview.
6. Drop it onto a surface or the construction plane.
7. Move, rotate, and scale it with a visible handle.
8. Type an exact position in Inspector when precision matters.
9. Undo the whole drag with one action.
10. Play the scene and return to the same authoring viewpoint.

This is the first product milestone. General graph editors, terrain authoring,
animation production, and arbitrary editor plugins extend that foundation later.

## What the Unity research establishes

Unity separates the Scene Camera from cameras attached to game objects.
Its Scene view offers orientation controls and perspective/orthographic views.
Its transform tools support handles, numeric Inspector editing, and configurable
pivot position and local/global orientation. Its grid controls distinguish
visual presentation from snapping operations.
These are interaction references, rather than an instruction to reproduce
Unity's proprietary implementation or every feature in its editor.
[Unity Scene Camera reference](https://docs.unity.com/en-us/engine/6000.0/manual/unity-editor/editor-windows-views-reference/using-the-scene-view/scene-view-camera),
[Unity Scene view navigation](https://docs.unity.com/en-us/engine/6000.0/manual/unity-editor/editor-windows-views-reference/using-the-scene-view/scene-view-navigation),
[Unity positioning tools](https://docs.unity.com/en-us/engine/6000.0/manual/working-with-scenes/scenes-manage-gameobjects/positioning-game-objects),
[Unity Grid and Snap overlay](https://docs.unity.com/en-us/engine/6000.0/manual/working-with-scenes/scenes-manage-gameobjects/grid-snapping/grid-and-snap-overlay).

Our design adopts the familiar spatial workflow and chooses a smaller delivery
scope suited to this engine. The detailed chapters label engine-specific choices
and performance targets as proposals.

## Current engine baseline

| Area | Present in the inspected working tree | Remaining viewport work |
| --- | --- | --- |
| Native surface | One Vulkan child surface embedded in the desktop editor. | Navigation and tool ownership inside that surface. |
| Scene rendering | Native worker, camera uniforms, scene synchronization, invalidation. | Editor grid, tool handles, selection outlines, drop ghosts. |
| Workspace | Scene/Game views, Inspector, entities, Assets, frequent actions. | Hierarchy interactions, viewport tool strip, orientation controls. |
| Selection | Rust selection/picking commands and selectable entity rows. | Bind native click/marquee gestures and selection feedback. |
| Transforms | Local Inspector editing with typed commands and transactions. | Spatial handles, constrained drags, and pointer feedback. |
| Camera commands | Authored camera operations exist in the scene command model. | Separate editor navigation state before using them as viewport controls. |
| Asset placement | Catalogued models can be added at the origin. | Surface-aware placement previews and cross-boundary dragging. |
| Persistence | Scene/project saving and retained scene drafts. | Editor-only viewport preferences and per-scene view bookmarks. |
| Play | Existing session mode and Scene/Game presentation. | Complete focused gameplay input and safe Scene inspection during Play. |
| Export | Staged project output and explicit standalone build/run flow. | Desktop and another-machine acceptance evidence. |

The Inspector and project workflow added immediately before this design remain
useful. The design connects those capabilities to spatial authoring rather than
replacing their backend semantics.

## Primary decisions

### Editor navigation is not a scene edit

Orbit, pan, dolly, fly speed, projection, and frame-selection change an editor
view state. They do not modify the authored scene camera, dirty a scene, create
undo entries, or change a packaged game.

An explicit action such as Align Game Camera to View performs the authored
mutation. It is visibly named, undoable, and unavailable when authoring is locked.

### Vulkan owns spatial feedback

The world grid, transform handles, hover highlights, selection outlines,
orientation widget, marquee, and model-placement ghost are rendered by the
native editor viewport.

Angular owns the adjacent tool strip, Inspector controls, hierarchy, Assets,
menus, and accessible alternatives. This respects the existing Windows native
child surface boundary instead of assuming HTML can float above it.

### The pointer stays responsive

Native camera movement should not wait for an Angular change-detection pass or
a serialized scene snapshot. Camera changes invalidate the native view directly.

Authored edits still go through Rust validation and history. Continuous gestures
coalesce replaceable preview values; press, commit, cancel, and capture-loss
boundaries retain their order.

### One drag creates one undo entry

Dragging a handle or placing an asset has a start state, a preview state, and a
single completion boundary. Escape or capture loss restores the starting state.
An abandoned gesture never leaves a grouped transaction open.

### Familiar controls have explicit precedence

Q selects the hand/navigation tool; W, E, and R select Move, Rotate, and Scale.
RMB navigation owns movement keys only while the viewport has its navigation
capture. Text fields always retain editing keys and shortcuts.

### Surface placement has a predictable fallback

A model dragged over geometry uses the current placement policy to resolve a
valid surface. A miss falls back to the active construction plane when possible.
A parallel ray or invalid surface shows a rejected placement state rather than
creating an entity at an arbitrary distant point.

### Visual grid density is independent of snapping

Zoom may change the density of rendered grid lines. It must not silently change
the author's selected move increment. Both the visible grid spacing and the snap
increment have readable labels.

### Game data remains independent of the editor

The runtime player does not include hierarchy layout, tool preferences, grid
visibility, editor selection, or viewport navigation state. Shared core game
logic and scene composition retain the architecture already chosen for the engine.

## Visual composition

### Application chrome

Use a restrained dark grey product palette consistent with the current editor.
Keep a clear hierarchy between the native title/menu area, project controls,
workspace tabs, viewport tools, and content panes.

The application title identifies the project. The scene selector identifies the
active scene and its dirty state. Play controls occupy a stable, recognizable
location rather than moving when optional tools appear.

File, Edit, View, Scene, Tools, and Help are proposed top-level menu groups.
The implementation may initially keep the current File menu while additional
commands are introduced through their actual feature work.

Do not present inactive graph builders as finished tools. Animation Controller
and Shader Graph acquire real tabs when their document types are implemented.

### Hierarchy

Rename the spatial object list to Hierarchy when parent/child presentation and
the associated interaction behavior are introduced. Until then, Scene Entities
accurately describes the existing flat list.

The proposed Hierarchy has a scene root, expandable parent rows, selection,
keyboard navigation, and per-editor visibility/pickability controls.
It should not duplicate project-folder navigation, which belongs in Assets.

Rows display a semantic entity name first. Model paths belong in details or
Inspector, avoiding a second long line for every entity in a large scene.

Picking an object in the viewport reveals its hierarchy row. Choosing a hierarchy
row updates the native selection outline and Inspector without moving the camera.
Double-click or Frame Selection is a separate navigation action.

### Viewport tools

Place the persistent tool controls outside the native child bounds:

- Hand, Move, Rotate, and Scale.
- Local/World handle orientation.
- Pivot/Center handle placement.
- Grid visibility and construction plane.
- Snap enable state and increment.
- Perspective/Orthographic mode.
- Frame Selection and View Options.

The orientation widget inside Vulkan shows X, Y, and Z labels in addition to
axis colors. It is not the sole way to change views: accessible external view
controls expose the same operations.

Toolbars should wrap into a deliberate secondary row at small desktop widths.
They must not overlay Inspector content or reduce the native surface to a
negative/zero size during a resize.

### Scene content

The initial viewport contains a quiet background, a horizon where applicable,
the ground grid, axis references, and actual scene geometry.

The selection outline uses one consistent accent. Tool axes retain conventional
X/Y/Z cues with visible labels, handle shapes, and hover treatment.

The grid fades with distance and at grazing angles. Minor lines remain softer
than major lines; the origin axes remain recognizable without dominating props.

Avoid heavy vignettes, fake post-processing, decorative statistics, or permanent
callouts across the scene. Every overlay should help an author make a spatial
decision or understand the active operation.

### Inspector

Keep the existing local Position, Rotation, and Scale controls as the exact-value
alternative to a drag. Rotation remains degrees in the UI and radians at the
Rust contract boundary.

Show the selected entity's name, stable identity, model reference, and available
component sections. Future collider, behavior, material, and gameplay settings
must use genuine backend capability rather than decorative forms.

When multiple objects are selected, show mixed values deliberately. Do not display
the first object's values as if they applied identically to the whole selection.

### Assets

Keep folder navigation and content in the bottom drawer, with a broad, usable
resize target and a persistent collapse control.

Model entries support drag-to-place and an Add to Scene fallback. Scene entries
support Open Scene and a visible active marker. Later material/prefab/texture
entries use distinct, typed drop behavior.

A drag begins from the actual asset entry. It carries a project-scoped asset
identity, not an unvalidated arbitrary executable path or a display label alone.

### Status and help

Show a concise contextual gesture hint near the viewport edge or adjacent status
line. During a transform, display its axis constraint and current displacement.

Use status messages for operation completion and alerts for errors. Do not
announce every pointer move through accessibility live regions.

The help surface explains navigation, precision editing, snapping, cancellation,
and the difference between Scene and Game. It is available through the keyboard.

## State ownership map

| State | Owner | Persistence | Undo |
| --- | --- | --- | --- |
| Entity transform and parent | Core scene document | Scene JSON | Yes |
| Authored game camera | Core scene document | Scene JSON | Yes |
| Shared behavior definitions | Project/core data | Project JSON | Yes where supported |
| Play simulation state | Runtime session | Transient session | Authoring history stays separate |
| Editor camera and orbit pivot | Editor viewport session | Optional per-project editor preferences | No |
| Grid appearance and visible spacing | Editor viewport preferences | Editor settings | No |
| Snapping policy and increments | Editor tool settings | Editor preferences or explicit existing snap contract during migration | No scene-history pollution |
| Selection | Editor session | Session/editor preference only | Defined by editing transaction semantics |
| Hover and placement ghost | Active interaction | Never | No |
| Transform gesture baseline | Gesture transaction | Never directly | One committed result |
| Hierarchy expansion and pane widths | Editor layout preferences | Editor settings | No |

Selection restoration during Undo remains consistent with the existing Rust
history contract. This does not imply that every click creates an undo entry.

## Input ownership map

| Context | Input owner | Required behavior |
| --- | --- | --- |
| Inspector text/number field | Focused Angular control | Typing, selection, arrows, and field commit/cancel. |
| Native viewport idle | Native viewport interaction router | Hover, selection, tools, and navigation entry. |
| Captured native camera gesture | Native navigation state | Pointer delta, fly keys, release, and focus loss. |
| Captured transform gesture | Native tool state with Rust transaction | Constrained preview, commit, or rollback. |
| Asset drag crossing WebView/native boundary | Shared editor drag coordinator | One drag identity and authoritative placement lifecycle. |
| Modal application dialog | Dialog | Suspend/cancel viewport capture and prevent hidden gameplay input. |
| Game preview with gameplay focus | Runtime input routing | Gameplay actions; Escape releases focus before editor shortcuts. |

Ownership transitions are explicit. Leaving a native child window is not proof
that a drag ended; losing capture or focus is a separate event.

## Responsiveness goals

These are initial engineering targets to measure, not existing benchmark claims.

| Interaction | Initial proposed target | Measurement |
| --- | --- | --- |
| Editor camera feedback | Visible on the next available rendered frame. | Native input timestamp to presented camera revision. |
| Hover/handle feedback | No synchronous GPU readback on the input path. | Input age and pick result age. |
| Transform preview | Bounded queue with latest-value coalescing. | Pending preview count and presented edit revision. |
| Idle scene | No continuous full render loop solely for a static grid. | Worker wakeups and frame submissions while idle. |
| Asset drag | Cached lightweight ghost before final model readiness when necessary. | Drag-enter to first valid preview. |
| Large hierarchy | Virtualized rows when profiling justifies the threshold. | Scroll/input latency and DOM node count. |

Use a reproducible representative scene and report median, p95, and p99 latency.
Do not use average FPS alone to decide whether the editor feels responsive.

## Implementation order

### Milestone A: navigate and understand space

1. Add editor-only camera state and an explicit native navigation router.
2. Separate camera/view revisions from authored scene revisions.
3. Implement orbit, pan, dolly, keyboard alternatives, and Frame Selection.
4. Add perspective/orthographic projection and orientation views.
5. Draw a depth-correct adaptive grid and visible axis references.

The acceptance scenario is opening an existing project, exploring its scene,
and confirming that the document remains clean and its undo history unchanged.

### Milestone B: select and manipulate

1. Bind viewport picking to the shared editor selection.
2. Add hover and selected-object feedback.
3. Implement Move handles, plane constraints, and snapping.
4. Add Rotate and Scale with Local/World and Pivot/Center semantics.
5. Make every gesture commit once and recover on Escape or capture loss.

The acceptance scenario is placing an object precisely, undoing once, and seeing
the original authored transform restored in both Inspector and Vulkan.

### Milestone C: assemble a level

1. Introduce project-scoped asset drag sessions.
2. Draw placement ghosts and resolve surface/construction-plane hits.
3. Commit a dropped model once and select its created entity roots.
4. Add hierarchy parenting with explicit keep-world behavior.
5. Complete keyboard placement alternatives and recoverable asset errors.

The acceptance scenario is blocking out a small courtyard with repeated props
and consistent scale, then saving and reopening the result.

### Milestone D: prove a game workflow

1. Connect authored player/gameplay settings through real supported components.
2. Complete focus-safe Game input and preserve the Scene viewpoint across Play.
3. Add the minimum authoring UI for the chosen game's objective and scene flow.
4. Export the saved project and run its standalone player.
5. Measure the larger authoring session and fix demonstrated bottlenecks.

The exact gameplay UI depends on which runtime contracts are already supported.
The support-agent workflow chapter identifies those dependencies rather than
silently expanding this viewport milestone into a general engine rewrite.

## Design boundaries

The design does not claim production implementation of a new grid, navigation
router, spatial handles, hierarchy drag/reparent UI, or surface placement.
Those are the next implementation features.

The interactive preview uses local simulated scene data. Its mouse gestures,
outlines, object placement, and Inspector changes are useful for reviewing the
interaction design, not measuring Vulkan performance or validating native DPI.

Complex physics authoring, animation state graphs, shader graphs, terrain,
networked collaboration, and a general plugin SDK remain later feature areas.
Their extension points are described where they affect present responsibilities.

## Support agent

The Game Development Support profile is maintained in
`agent-profiles/game-development-support/SKILL.md` with Codex UI metadata beside it.
It connects viewport decisions to the first finishable game, reviews the smallest
useful next feature, and explains how to use real engine capabilities.

It should be consulted for authoring-flow decisions and gameplay acceptance,
while implementation agents retain responsibility for their code and checks.
It does not autonomously commit, publish, modify documentation, or schedule work.

## Related engine references

- [Current editor workspace](../../docs/editor-workspace.md).
- [Editor command architecture](../../history/adr/0001-editor-command-pipeline.md).
- [Object composition and inspector subjects](../../history/adr/0002-object-composition-and-components.md).
- [Rust and TypeScript contracts](../../history/adr/0003-rust-typescript-bridge.md).
- [Scene data and persistence](../../docs/scene-persistence.md).
- [Project lifecycle](../../docs/project-lifecycle.md).

These references describe their own inspected snapshots. The present design is
the governing proposal for the new viewport work, not a retroactive declaration
that all earlier roadmap features are complete.
