# Scene view interaction design

Status: proposed product and interaction specification.
Research date: 2026-11-06.
Audience: Engine Developer.
This chapter specifies the editable Vulkan viewport and its surrounding controls.
It does not claim that the proposed interactions are already implemented.
All default values below are initial product decisions that require usability validation.
The interactive mockup demonstrates the intended experience; it is not a GPU benchmark.

## 1. What the Scene view is for

The Scene view is the developer's workspace for arranging and understanding a game world.
The Game view shows the world through the game's camera and input rules.
The editor camera is a tool for the developer and must remain independent of the game camera.
An artist must be able to inspect a room without changing where the player starts looking.
A designer must be able to place repeated props without manually typing every transform.
A programmer must be able to locate a selected entity and inspect its state without losing context.
Every visible control should support one of those tasks.

The first useful workflow is a small playable room:

1. Open its scene.
2. Orbit, pan, and zoom to understand the room.
3. Drop a model onto the construction plane.
4. Move, rotate, and scale it with visible handles.
5. Save, play the room, stop, and continue editing.

Unity describes its Scene view as a place to visualize and manipulate the world.
That role is the reference model for this design.
[Unity Scene view reference](https://docs.unity.com/en-us/engine/6000.0/manual/unity-editor/editor-windows-views-reference/using-the-scene-view).

## 2. Research findings and adoption decisions

The following observations concern Unity 6.0 documentation, not undocumented Unity internals.
The adoption column describes this engine's proposed behavior.
The implementation may differ while preserving a familiar user experience.

| Area | Observed Unity pattern | Proposed adoption |
| --- | --- | --- |
| Navigation | Scene camera supports pan, orbit, zoom, flythrough, and framing. | Offer these as distinct, predictable gestures. |
| Orientation | An axis widget changes viewing direction and projection. | Keep a compact native orientation widget. |
| Transform tools | Move, Rotate, and Scale have direct shortcuts. | Start with Q/W/E/R and visible tool buttons. |
| Handle placement | Pivot and Center control handle position. | Make the current group pivot explicit. |
| Handle basis | Local and world-oriented handles serve different edits. | Show Local or World next to the active tool. |
| Grid | Visual grid settings and snapping settings are configurable. | Separate display density from edit increments. |
| Selection | Scene and Hierarchy selection operate on the same objects. | Use one authoritative selection of entity keys. |
| Dense scenes | Overlapping objects can be chosen from a pick list. | Add a named overlap picker after basic picking. |
| Overlays | Tool panels can be shown, hidden, and arranged. | Begin with fixed safe zones, then add layout presets. |
| Game distinction | Development view settings do not change the built game. | Persist editor preferences outside scene JSON. |

The navigation observations follow Unity's documented controls.
[Unity Scene view navigation](https://docs.unity.com/en-us/engine/6000.0/manual/unity-editor/editor-windows-views-reference/using-the-scene-view/scene-view-navigation).
The transform observations follow Unity's positioning guide.
[Unity Position GameObjects](https://docs.unity.com/en-us/engine/6000.0/manual/working-with-scenes/scenes-manage-gameobjects/positioning-game-objects).
The selection observations follow the picking reference.
[Unity Select GameObjects](https://docs.unity.com/en-us/engine/6000.0/manual/working-with-scenes/scenes-manage-gameobjects/scene-picking/select-game-objects).
The display-state distinction follows the view options reference.
[Unity Scene view draw modes and view options](https://docs.unity.com/en-us/engine/6000.0/manual/working-with-scenes/scenes-manage-gameobjects/overlays/view-modes).
Unity's overlay guidance supports contextual, user-controlled tooling.
The fixed safe zones here are a first implementation choice for this native-child architecture.
[Unity overlay design guidance](https://www.foundations.unity.com/patterns/overlays).

## 3. Product priorities

1. A gesture produces immediate visual feedback.
2. An edit has a predictable result and can be undone.
3. Camera movement never changes authored gameplay data.
4. Mouse and keyboard operations remain understandable in large scenes.
5. The interface stays quiet until a tool needs more information.
6. Custom tools can extend the workspace without replacing its input ownership.
7. The editor remains usable when assets are loading or a build is compiling.

Visual polish means clear hierarchy, readable controls, stable spacing, and restrained feedback.
It does not mean making the viewport resemble a dashboard.
The world should occupy most of the available center region.
Frequent tools belong near that region without obscuring the selected object.
Long-running work should expose progress without taking over the pointer.

## 4. State ownership

Four kinds of state have different lifetimes.
Conflating them would create hidden saves, polluted undo history, and confusing Play behavior.

| State | Examples | Owner | Persistence |
| --- | --- | --- | --- |
| Authored scene | Entity transforms, parent links, game camera, lights | Rust scene model | Scene JSON |
| Authored project | Startup scene, scene catalog, shared behavior settings | Rust project model | Project JSON |
| Editor view | Orbit pivot, distance, projection, grid display, tool | Rust viewport session | Editor preferences |
| Gesture preview | Drag origin, hit handle, candidate placement, marquee | Active input controller | None |

Angular displays accepted authored state and editor settings.
The native viewport consumes high-frequency pointer input close to rendering.
The command pipeline remains the only route for accepted authored edits.
The editor camera should not enter scene undo history.
Changing grid opacity should not mark a scene dirty.
Moving a prop should mark its scene dirty.
Changing the authored game camera should remain an explicit scene edit.

Existing orbit, pan, and dolly commands currently mutate scene camera data.
Their names alone do not make them suitable for editor-only navigation.
Introduce the editor view-state boundary before wiring navigation gestures to those commands.

## 5. Workspace arrangement

The center defaults to Scene when an authoring project opens.
Game remains the neighboring center tab.
Animation and shader documents are future document types with their own tools.
Switching document type must not silently reinterpret a scene gesture.

The Scene workspace has these regions:

- A compact document header identifies the active scene and unsaved state.
- A tool strip contains Select/View, Move, Rotate, and Scale.
- A settings strip exposes Local/World, Pivot/Center, and snapping.
- The Vulkan canvas occupies the remaining center area.
- The orientation widget sits within a native canvas corner.
- A short contextual hint occupies a native canvas edge when useful.
- Hierarchy and Inspector remain outside native viewport bounds.
- Assets remains the resizable bottom drawer.

The native window must not extend beneath DOM buttons.
A toolbar click must never become a scene pick through transparent chrome.
Keep contextual text away from the last pointer position.
No persistent panel should cover the gizmo by default.

## 6. Native overlays and Windows composition

The current viewport is a native child surface within an Angular/Tauri editor.
Ordinary DOM elements cannot be assumed to draw above that child surface.
Use Vulkan for grid lines, selection outlines, transform handles, marquee, and drag ghosts.
Use Angular for surrounding toolbars, property forms, menus, and asset catalogs.
Use an explicit native-compatible path for any menu that overlaps the canvas.

A context menu may initially open in a reserved edge strip outside the canvas.
A later native overlay layer may support menus directly under the pointer.
Do not ship a visually correct browser mockup as proof of Windows composition.
When a native modal opens, finish or cancel the active gesture before suppressing the viewport.
Restoring the viewport must restore focus intentionally.
Never infer focus from the surface merely becoming visible again.

Each overlay exposes a stable identifier.
Each interactive overlay publishes its hit region in native viewport coordinates.
Hit testing prioritizes overlay controls before world geometry.
Noninteractive labels do not intercept world input.

## 7. Default navigation map

The default profile is designed for a three-button mouse on Windows.
Every navigation action also has a visible menu or button entry.
A two-button profile and remapping are supported preferences.

| Gesture | Context | Result |
| --- | --- | --- |
| Middle-button drag | Scene canvas | Pan in the camera plane |
| Alt + left-button drag | Perspective or unlocked orthographic view | Orbit around the view pivot |
| Wheel up/down | Scene canvas | Dolly closer/farther, or change orthographic size |
| Alt + right-button drag | Scene canvas | Continuous dolly |
| Right-button drag | Perspective | Look around in flythrough mode |
| Right button + WASD | Perspective flythrough | Move horizontally relative to view |
| Right button + Q/E | Perspective flythrough | Move down/up |
| Shift during flythrough | Flythrough | Increase movement speed |
| F | Scene canvas with selection | Frame selection |
| Home | Scene canvas | Frame all eligible visible entities |
| Plus / Minus | Scene canvas | Step zoom in/out |
| Escape | Any active gesture | Cancel the current gesture |

Right-button flythrough consumes W/E/Q before transform-tool shortcuts.
Releasing the right button exits flythrough.
Movement keys must be cleared on focus loss.
The engine must not continue moving after an Alt-Tab or modal dialog.

## 8. Keyboard dispatch priorities

Use one ordered dispatcher rather than independent listeners in every panel.
The dispatcher asks the focused surface and active gesture which action owns an event.
Events are handled once.

Highest to lowest priority:

1. Operating-system and native modal interaction.
2. Active text or numeric field editing.
3. Active pointer gesture cancellation or completion.
4. Active flythrough movement.
5. Context-specific scene tools.
6. Focused panel navigation.
7. Application commands.

Typing W into an entity name must not activate Move.
Typing Delete into a text field must not delete an entity.
Undo in an active text field first follows that field's editing rules.
Once the field commits, scene Undo acts on the authored command history.
Tool shortcuts require Scene canvas focus or a deliberate global tool context.
Shortcut help shows the currently effective binding.
Remapping rejects conflicting bindings within the same active context.
Conflicts across exclusive contexts are allowed and explained.

## 9. Editor camera model

The editor view stores an orbit pivot and a positive distance from that pivot.
It also stores orientation, projection type, orthographic size, and navigation speed.
Camera settings belong to the view, not to an entity.
The proposed baseline is right-handed coordinates, positive Y up, and an XZ ground plane.
The starter authoring convention treats one world unit as one metre.
That is a proposed project convention, not a current runtime-enforced physical unit.
The initial pose looks toward the scene origin from above and to one side.
If a project has a useful bounding box, first-open framing uses it.
Otherwise, the view presents the origin grid and a neutral empty state.

Camera motion is stable under different frame rates.
Integrate flythrough using elapsed time rather than pointer-event count.
Treat pointer deltas as input samples rather than absolute camera transforms.
Keep the navigation speed independent of render frame count.
Clamp angular singularities or use a stable orientation representation.
Prevent accidental upside-down orbit in the default profile.
Offer unrestricted orbit as an advanced preference.

Camera state is saved per project, scene, and workspace view.
The preference key uses stable scene identity rather than the scene filename alone.
A renamed scene retains its view.
A different project with identical scene keys receives a different view context.

Unity separately documents Scene Camera settings that do not change camera components.
[Unity Scene Camera reference](https://docs.unity.com/en-us/engine/6000.0/manual/unity-editor/editor-windows-views-reference/using-the-scene-view/scene-view-camera).

## 10. Orbit behavior

An orbit begins with a pivot chosen before the first movement sample.
Use the current explicit view pivot.
Framing a selection updates that pivot to the framed bounds center.
Orbiting must not change entity selection.

Horizontal pointer motion rotates around the configured world-up direction.
Vertical pointer motion changes elevation around the pivot.
The distance to the pivot stays constant during an ordinary orbit.
The selected entity does not need to exist for orbit to work.
When no meaningful pivot exists, use the last valid view pivot.

The orbit cursor is visible at gesture start.
Captured movement can continue outside the viewport.
The application releases capture on completion, cancellation, focus loss, or view destruction.
Escape restores the camera pose from gesture start.
Undo does not replay editor-camera navigation.

If the pivot is behind the camera after an external view change, reestablish a valid pivot.
Do not snap the view to the world origin without an explanation.
An orientation-axis click ends an orbit before starting its view transition.

## 11. Pan behavior

Panning moves the editor camera and view pivot together.
The apparent world point under the pointer should remain approximately under it.
Sensitivity scales with view depth in perspective.
Sensitivity scales with orthographic size in orthographic mode.

Panning parallel to the screen is the default.
A separate Ground Pan command may move along the active construction plane.
Do not make a modifier silently switch pan planes halfway through a gesture.
If a modifier changes mode, display the new mode before the next accepted movement.

The pan gesture does not select the object beneath its start position.
It does not start a marquee.
It does not dirty the authored scene.
Large pointer jumps from a DPI transition are rejected rather than applied as world movement.
Pan speed has a bounded preference range.
Reset View restores a usable pose when accumulated navigation becomes confusing.

## 12. Dolly, zoom buttons, and cursor anchoring

Perspective zoom is primarily a dolly rather than a field-of-view change.
Keeping field of view stable makes size and spatial relationships easier to understand.
Orthographic zoom changes the orthographic view size.
The projection label always indicates which interpretation is active.

Wheel input uses a multiplicative distance or size change.
Equal wheel steps should feel similar when inspecting a small prop or a large room.
Smooth wheel deltas are accumulated without forcing each sample into a full wheel notch.
Plus and Minus buttons apply equivalent bounded steps.
A Reset Zoom action frames the current selection or visible scene.

The preferred anchor is the world hit under the cursor.
When there is no geometry hit, intersect the active construction plane.
When the plane is parallel to the view, use the view pivot.
Do not block zoom while waiting for a new GPU picking readback.
A recent valid anchor may be reused for the current gesture.

The minimum distance depends on scene scale and clip limits.
The maximum distance is finite and visible in advanced camera settings.
Reaching a limit produces a subtle boundary indication.
Repeated zooming must never produce NaN, infinity, or an inverted projection.

## 13. Flythrough mode

Flythrough is a temporary navigation state.
It starts when the right-button gesture exceeds the look threshold or a movement key is used.
A plain right click can still open the scene context menu after release.
Do not open that menu after a flythrough gesture.

Mouse movement changes yaw and pitch.
WASD moves relative to the view's horizontal basis.
Q and E move along the configured world-up direction.
Shift provides a temporary speed multiplier.
The wheel adjusts flythrough movement speed while this mode owns the pointer.
The speed indicator appears only while adjusting or actively flying.

The default flythrough has no gameplay collision.
It must pass through walls to inspect interiors.
A future collision-aware camera is an optional navigation mode.
That option must not create physics state in the authored scene.
Releasing the right button clears all movement input.
Losing application focus clears every held key and releases capture.
Entering Play ends flythrough before gameplay input becomes active.

## 14. Orientation and projection widget

The orientation widget shows named positive and negative axes.
Axis colors are supplemented with X, Y, and Z labels.
Clicking an axis moves to a named orthographic view.
The supported names are Top, Bottom, Front, Back, Left, and Right.
The meaning of Front follows the engine's documented world convention.
Do not copy Unity's axis assumptions without checking this renderer's convention.

Clicking the center toggles Perspective and Orthographic.
A text control provides the same operation.
The current mode remains visible even when no entity is selected.
Reset View restores the default elevated perspective.
Rotation Lock prevents orbit while allowing pan and zoom.
The lock has an explicit visual and accessible state.

Projection changes preserve the apparent size of the pivot region where possible.
Axis transitions preserve the current pivot.
Reduced-motion preference disables animated camera transitions.
The transition can be interrupted by a new navigation gesture.
The final view state, not intermediate animation samples, is persisted.

## 15. Grid purpose and visual hierarchy

The grid is a construction aid, not an entity.
The initial grid lies on the engine's horizontal ground plane.
It has minor lines, stronger major lines, and labeled origin axes.
The background is dark neutral gray.
The grid has enough contrast to judge alignment without competing with objects.

Minor-line density adapts to camera scale.
Major lines remain visually stable while minor levels blend.
Distant lines fade rather than accumulating into a bright horizon band.
The grid is depth-aware so solid objects can occlude it.
An optional X-ray Grid mode is clearly identified as a view preference.
The origin axes remain distinguishable from transform gizmos.

The grid is visible in a new empty scene.
It stays available when no meshes have loaded.
A grid toggle never changes snapping by itself.
A snapping toggle never forces the grid visible.
The status strip exposes the actual placement increment when snapping is active.

Unity documents configurable grid appearance and separate snapping operations.
[Unity Scene view grid snapping](https://docs.unity.com/en-us/engine/6000.0/manual/working-with-scenes/scenes-manage-gameobjects/grid-snapping).

## 16. Construction plane and grid controls

The initial plane can be changed to XY, XZ, or YZ.
Display plane labels use the engine's axis convention.
The plane can be positioned at Origin or At Selection.
At Selection copies a position once.
It does not become an implicit constraint following future selection movement.

Grid opacity is a view preference.
Minor-line density is an automatic visual setting with an optional manual override.
Placement spacing is an authored-edit preference with an explicit numeric value.
Those two values must never be confused.
Zooming out may display a ten-unit visual grid while the placement increment remains one unit.
The interface then shows both values when their difference matters.

Reset Grid restores plane, position, opacity, and display density.
Reset Grid does not move entities.
Align Selection to Grid is a separate undoable command.
Its action label includes the affected axes.
Changing plane during a live drag is blocked until that drag ends.

Unity's Grid and Snap overlay exposes grid plane, visibility, opacity, and increments.
This design adopts the separation while choosing explicit per-project editor preferences.
[Unity Grid and Snap overlay](https://docs.unity.com/en-us/engine/6000.0/manual/working-with-scenes/scenes-manage-gameobjects/grid-snapping/grid-and-snap-overlay).

## 17. Snapping semantics

The initial increments are one world unit for translation and fifteen degrees for rotation.
The initial scale increment is one tenth.
These are product defaults, not claims about the engine's unit system.
Use the proposed metre convention in starter projects and label generic units for imported projects without one.

Absolute grid snapping rounds a world-space target to grid coordinates.
Increment snapping quantizes the delta from the gesture's starting transform.
Expose these as different modes.
Do not move an already offset object onto an absolute grid when the user asked for increments.
Local incremental snapping uses the handle basis captured at gesture start.
Absolute snapping initially supports world-aligned construction planes.

For a drag, compute each result from the original transform.
Do not repeatedly round the previous preview.
This prevents accumulated error and allows accurate reversal.
The snap modifier temporarily changes the current gesture's snapping mode.
A native hint states the effective mode while the modifier is held.
Releasing the modifier restores the persistent setting without a position jump caused by accumulation.

Rotation values are displayed in degrees and converted to backend radians.
Scale snapping preserves axis signs unless the user explicitly crosses a sign boundary.
Surface and vertex snapping are later capabilities with separate availability indicators.

## 18. Selection and active entity

There is one authoritative entity-key selection shared by canvas, Hierarchy, and Inspector.
A selection has an ordered set and an explicit active entity.
The active entity supplies the Pivot/Local reference when needed.
Show an active marker in the Hierarchy and a stronger selected outline.
Do not rely on selection order that is invisible to the developer.

| Input | Result |
| --- | --- |
| Click visible eligible entity | Replace selection and make it active |
| Click empty canvas | Clear selection |
| Ctrl + click entity | Toggle membership |
| Shift + click entity | Add without removing existing members |
| Shift + click selected entity | Make it active |
| Escape with no gesture | Clear selection |
| Hierarchy row click | Select the same entity key |
| F after selection | Frame accepted selected bounds |

Selection changes do not dirty the scene.
An entity being loaded may be selected through Hierarchy even without render geometry.
Selecting it exposes loading state and any available transform.
Deleting an entity removes its key from selection.
Undo restores selection consistently with the accepted history operation.

Unity supports single and multiple selection in both Scene view and Hierarchy.
It also distinguishes an active object for operations requiring one reference.
Our explicit active marker is an intentional usability improvement.
[Unity Select GameObjects](https://docs.unity.com/en-us/engine/6000.0/manual/working-with-scenes/scenes-manage-gameobjects/scene-picking/select-game-objects).

## 19. Picking and dense-scene behavior

The first picking implementation may use existing mesh bounds.
The UI must not imply triangle-accurate picking if it only tests bounding boxes.
Document the approximation in developer diagnostics rather than an ordinary confirmation dialog.
Progress toward accurate visible-surface picking when actual scenes demonstrate the need.

Pick results include the view generation and scene revision used for the query.
A stale result must not replace selection in a different scene.
Hover information is advisory.
Accepted selection must resolve against the current authoritative document.
An entity deleted while a pick is in flight is ignored.

Prefer the nearest eligible visible object by default.
Selection outlines do not become pickable geometry.
Grid lines never become a selection candidate.
Gizmo handles have their own priority before object picking.
Hidden or unpickable entities are excluded according to editor state.
An overlap picker lists eligible candidates by name and hierarchy path.
Choosing a candidate is an ordinary selection change.

Unity's selection piercing menu addresses overlapping geometry.
This engine should expose a comparable named choice without requiring repeated blind clicks.
[Unity selection piercing menu](https://docs.unity.com/en-us/engine/6000.0/manual/working-with-scenes/scenes-manage-gameobjects/scene-picking/selection-piercing-menu).

## 20. Marquee selection

Dragging empty canvas with the selection tool starts a marquee after a small movement threshold.
The initial threshold is four CSS pixels, converted using the current DPI scale.
A click below that threshold remains a click.
The marquee remains a native overlay above the rendered world.
Dragging from an entity body in Select mode also becomes a marquee once the threshold is crossed.
Moving an entity initially requires a transform handle or Inspector edit, preventing accidental body drags.

Default marquee selects eligible objects intersecting the rectangle.
A preference may require full containment.
The mode is displayed in the gesture hint.
Ctrl toggles the hit set against the starting selection.
Shift adds the hit set.
No modifier replaces the selection with the hit set.

Compute the final result from the selection captured at gesture start.
Do not repeatedly toggle the same entity on every movement sample.
Large result sets use bounded selection rules from the backend.
If a limit is reached, show how many were selected and why the remainder was excluded.
Escape restores the starting selection.
Focus loss cancels the marquee.
The completed marquee produces one accepted selection update.

## 21. Visibility and pickability

Visibility and pickability are editor state, not synonyms for gameplay activation.
A hidden editor object can still exist and behave in the game.
An unpickable editor object can remain visible.
Use distinct eye and pick-lock controls with accessible names.

Visibility and pickability can be inherited by a hierarchy subtree.
The row indicates when a parent supplies the effective restriction.
A temporary Reveal Selection action helps locate an intentionally hidden object.
That action does not change authored rendering flags.
Isolate Selection temporarily hides other editor geometry.
Exiting isolation restores the prior visibility set.

A visible status badge indicates that isolation or hidden-object filtering is active.
Reset View does not silently clear visibility preferences.
Provide a dedicated Show All action.
Persist these preferences per project and scene.
Do not include them in standalone game exports.

## 22. Transform tool model

Q selects the View/Select tool.
W selects Move.
E selects Rotate.
R selects Scale.
Each tool is also available as a labeled button and a menu action.
The active tool remains visibly selected.

The tool controller has a stable identity separate from the selected entity.
Changing selection does not reset the tool.
An unavailable tool remains visibly unavailable with a concise reason.
Do not advertise Rect Transform until the engine has a real 2D/UI layout model.
Do not advertise a combined Transform tool until its hit regions are reliable.

Move supports axis and plane handles.
Rotate supports axis rings and a screen-facing ring.
Scale supports axis handles and uniform scale.
All tools share selection, pivot, basis, snapping, and transaction rules.
Tool-specific code supplies geometry and target-transform calculation.
A custom tool must not bypass the shared gesture lifecycle.

## 23. Gizmo appearance and hit testing

Gizmos maintain approximately constant screen size as camera distance changes.
The initial nominal size is ninety-six CSS pixels.
The pickable region is wider than the rendered line.
A narrow visual line must not require pixel-perfect pointing.

Axis colors are paired with X/Y/Z labels or recognizable arrow directions.
Hovered handles brighten and show a short axis label.
The selected handle remains distinct throughout a drag.
Handles behind geometry may remain visible with subdued styling.
Their picking behavior follows the declared occlusion preference.

When handles overlap in screen space, prefer the nearest screen-space handle segment.
Use stable tie-breaking so tiny pointer movement does not flicker between axes.
Axis handles nearly aligned with the camera become visually deemphasized.
Plane handles remain available when their projected area is usable.
Do not begin a constraint whose geometry is numerically undefined.
Offer another handle or an Inspector field instead.

The gizmo disappears when selection is empty.
It is disabled during Play authoring restrictions.
Loading geometry does not prevent transform editing when a valid entity transform exists.

## 24. Pivot, center, local, and world

Pivot mode uses the active entity's authored pivot.
Center mode uses the combined bounds of eligible selected roots.
When render bounds are unavailable, use transformed entity origins as a fallback.
The fallback is stable for the duration of the gesture.

World basis aligns handles to the engine's world axes.
Local basis aligns handles to the active entity's orientation.
The chosen basis is captured at drag start.
Do not rotate the basis continuously during the same rotation gesture.
Changing the basis or pivot mode is blocked while a transform drag is active.

For multiple selected entities, transform the selected roots of the selection.
A selected child of a selected parent must not receive the same world delta twice.
Preserve descendant local transforms unless the operation explicitly targets them.
A multi-selection operation uses one group pivot and one command transaction.
Each entity still receives a valid local authored transform after parent conversion.

Nonuniformly scaled parents can make world-space operations ambiguous.
The implementation must detect unsupported decomposition instead of silently corrupting transforms.
The Inspector should report the limitation on the affected selection.
Undo must restore the exact original authored values.

## 25. Transform gesture transaction

A transform drag begins only after a valid handle is hit.
Capture selection, active entity, scene identity, editor view, pivot, basis, and original transforms.
Open the grouped edit before the first authored preview command.
Retain the original data until the gesture resolves.

Each pointer movement calculates a target from the captured original transforms.
The newest unsent target replaces older unsent targets.
At most one transform request is in flight per gesture.
Do not queue hundreds of intermediate mouse samples.
The native viewport may draw a local gesture preview while awaiting acceptance.
Rejected previews must never become the saved document.

Pointer release flushes the latest target and commits one history entry.
Escape drops unsent previews and restores the original transforms.
Focus loss cancels rather than leaving a hidden active transaction.
Scene replacement cancels the gesture before installing the new scene.
Save, Play, Duplicate, Delete, and scene switching settle the active edit first.
A failed rollback remains visible and can be retried.
Further authored gestures are blocked while that rollback is unresolved.

## 26. Numeric editing and fine adjustments

The Inspector remains a first-class alternative to pointer handles.
Position and scale use finite numeric values.
Rotation displays degrees while respecting backend radians.
An actively edited field keeps incomplete text locally.
Blank text and partial signs are not sent as invalid numeric transforms.

One continuous field edit produces one undo action.
Enter commits.
Escape restores the value at field focus.
Blur commits the latest valid edit according to the existing property-control behavior.
Selecting another entity settles the active field before selection changes.

Modifier-based fine adjustment is optional for the first release.
If added, Shift during a transform drag applies a clearly displayed precision factor.
It must not conflict with a simultaneously advertised snapping modifier.
Do not overload Shift with three unrelated meanings in one tool state.
Values derived from repeated pointer movement use the original transform as their baseline.
Numeric input can represent zero or negative scale if the backend allows it.
Pointer scale handles should avoid accidental singular scale without pretending the value is forbidden.

## 27. Asset drag and drop contract

Dragging a model from Assets begins an internal asset drag.
The drag payload contains a stable asset reference and source workspace identity.
A filename alone is insufficient to establish which project owns the asset.
The payload is not an authored entity until the drop is accepted.

Crossing into the native child viewport requires an explicit host bridge.
Do not depend on DOM dragover events firing above a native Vulkan window.
The host owns native pointer location and drag-target validation.
Angular owns the source catalog selection and accessible alternative actions.
Both sides share a drag-session identifier.

The first visible result is a translucent placement ghost.
If geometry is not ready, show a bounded placeholder with the asset's name.
Loading should not block ordinary camera navigation before the drag enters placement mode.
The ghost is editor-only and excluded from picking and game exports.
Canceling the drag leaves no entity, no history entry, and no dirty state.

## 28. Placement targeting

Default model placement intersects the active construction plane.
The ghost origin follows that intersection.
When snap is enabled, apply the actual placement increment.
The ghost displays the accepted target rather than an unrelated screen-space thumbnail.

Surface Placement is a separate mode.
It uses eligible scene surface hits and an explicit orientation rule.
The initial orientation rule preserves the asset's rotation.
Align to Surface Normal is an additional explicit toggle.
Model bounds may supply an optional rest-on-surface offset.
The offset must be consistent between preview and accepted entity creation.

If no valid plane or surface hit exists, the ghost shows a blocked state.
Dropping there does not create an entity at an arbitrary distant position.
Show the reason in the gesture hint.
The user can change view or choose Add to Scene as an alternative.
Do not silently switch to the world origin when the pointer is outside the viewport.
On drop, validate the workspace, scene generation, asset availability, and target once more.

## 29. Completing and canceling placement

An accepted drop instantiates the model as one atomic authored operation.
Imported model hierarchies retain their internal relationships.
The resulting root or roots become selected.
The active entity is chosen deterministically.
The Inspector shows the accepted transform.
One Undo removes the whole drop operation.

If loading finishes after cancellation, the asset cache may retain useful data.
It must not create the canceled entity.
A drop request for a closed project is discarded.
A request for a deleted or replaced asset reports a local error.
Do not retry a rejected drop automatically against a different scene.

Holding a repeat-placement modifier may keep the asset ready for another drop.
Each completed placement creates its own undo entry.
The repeating mode has a visible badge and an explicit exit action.
Escape exits it before clearing selection.
Repeat placement is a later enhancement after a single drop is reliable.

## 30. Hierarchy drag and drop

Dragging a Hierarchy row moves or reparents existing entities.
It is a different operation from dragging an asset to create entities.
The drag indicator must distinguish inside-parent placement from sibling ordering.
If sibling ordering has no authored meaning yet, do not display an unsupported reorder affordance.

Reparent defaults to preserving the world transform.
The resulting local transform is calculated and validated in Rust.
Dropping onto the current parent is a no-op.
Dropping an entity onto itself is rejected.
Dropping an ancestor into its descendant is rejected.
Locked or otherwise unavailable targets show a blocked indicator before release.

Dragging multiple selected roots reparents them as one operation.
Already selected descendants are not independently reparented twice.
The history entry states the operation and target parent.
Undo restores original parents, local transforms, and selection.
The keyboard alternative is a Reparent action with a searchable destination list.

## 31. Context actions and selection tools

A plain context click provides actions relevant to the current selection.
The initial actions are Frame, Duplicate, Delete, and Copy Entity Key.
Unavailable actions remain disabled with a concise explanation.
The menu does not invoke a destructive action merely because it opened under the pointer.

Delete is undoable and affects the authored scene.
Duplicate follows existing backend entity-key and hierarchy rules.
Frame changes only the editor view.
Copy Entity Key does not change selection or dirty state.
An overlap picker is a dedicated action when several candidates occupy the hit location.

The context target is captured when the menu opens.
If selection or scene identity changes before activation, revalidate the chosen command.
Close the menu on scene replacement or Play transition.
A menu never holds an unfinished transform transaction.
Keyboard users open the same action list from the focused entity or Scene view.

## 32. Scene, Game, and Play transitions

Scene is the authoring surface.
Game is the preview surface governed by gameplay input.
Play first settles an active authoring field or gesture.
The accepted authored document becomes the simulation starting point.
The simulation must not mutate the authoring baseline.

The Game view receives gameplay input only after deliberate focus.
Its focus state is visible.
Escape releases gameplay input capture before acting on authoring selection.
Stop clears held gameplay input and restores edit mode.
The previous editor camera and Scene selection remain available.

The current implementation may use one persistent native viewport for Scene and Game.
That is a rendering-resource choice, not a reason to conflate their state.
If switching to Scene during Play pauses or hides simulation rendering today, label that limitation.
A later dual-view arrangement needs an explicit simulation scheduling policy.
Do not imply simultaneous Scene/Game operation until it is implemented and measured.
Grid, gizmos, placement ghosts, and selection outlines are absent from standalone output.

## 33. Input-state machine

The pointer controller has one active state at a time.
States carry their own start snapshot and cleanup action.
The initial states are Idle, Hover, ClickCandidate, Marquee, Orbit, Pan, Dolly, Flythrough, TransformDrag, AssetPlacement, and ModalSuspended.

| Current state | Event | Next state | Required action |
| --- | --- | --- | --- |
| Idle | Pointer enters | Hover | Update candidate hit |
| Hover | Left down on handle | TransformDrag | Capture and begin transaction |
| Hover | Left down on entity | ClickCandidate | Capture click candidate |
| ClickCandidate | Release below threshold | Hover | Accept selection |
| ClickCandidate | Empty-space drag exceeds threshold | Marquee | Capture initial selection |
| ClickCandidate | Entity-body drag exceeds threshold | Marquee | Suppress the pending single pick |
| Hover | Navigation chord | Orbit/Pan/Dolly | Capture view pose |
| Hover | Right-button look | Flythrough | Capture and clear movement baseline |
| Hover | Valid internal drag enters | AssetPlacement | Create ghost only |
| TransformDrag | Release | Hover | Flush and commit |
| AssetPlacement | Valid drop | Hover | Commit one placement |
| Any gesture | Escape | Hover | Restore its captured baseline |
| Any gesture | Focus lost | ModalSuspended | Cancel, clear keys, release capture |
| ModalSuspended | Focus restored | Idle | Require a fresh gesture |

A gesture cannot transition directly into another authored gesture without resolving its transaction.
Every state has a timeout or ownership cleanup path for a destroyed viewport.
The controller ignores late movement samples from a previous gesture identifier.

## 34. DPI, resizing, and dock changes

Native input positions and extents are physical pixels in the existing bridge.
Visual sizing and drag thresholds are specified in CSS pixels.
The input boundary converts once using the viewport's current scale factor.
Do not apply device-pixel-ratio conversion both in Angular and Rust.

Each pointer sample identifies the view-bounds generation used to interpret it.
Resizing invalidates stale picking projections.
Moving a dock refreshes native bounds even if its dimensions do not change.
A minimized viewport has no interactive extent.
Never divide by zero while computing normalized pointer coordinates.

Changing monitor DPI during a gesture cancels the gesture safely.
The new DPI becomes active for the next gesture.
This is preferable to a sudden world-space jump.
The cursor returns to a valid position after capture ends.
Splitters remain outside the native interaction region or use coordinated native hit testing.

## 35. Accessibility and discoverability

Every pointer-only action has a menu, button, or numeric-field alternative.
The Scene canvas has a concise accessible name and a focus indicator.
Tool buttons expose their names, shortcuts, selected state, and disabled reason.
Grid and snap toggles expose separate pressed states.
Axis information uses labels as well as color.

The Hierarchy provides the nonvisual path to entity selection.
The Inspector provides the non-drag path to transform editing.
Add to Scene provides the non-drag path to model placement.
Frame Selection provides the recovery path after navigation becomes disorienting.
Reset View restores a known useful view without altering game data.

Announce accepted selection changes and completed operations.
Do not announce every hover or camera movement sample.
Error messages name the failed action and an available recovery.
Reduced motion disables view interpolation and nonessential transitions.
High-contrast mode preserves selected-handle and locked-state distinctions.
Tooltips supplement labels; they do not contain the only explanation of a critical state.

## 36. User-facing feedback

Feedback has three layers.
Persistent chrome shows scene, tool, basis, pivot, projection, and dirty state.
Context hints show the current gesture and useful modifier.
Operation feedback reports accepted changes or failures.

Examples of useful hints:

- “Orbit · release Alt + Left Mouse to finish.”
- “Move X · snap 1 unit · Esc cancels.”
- “Place robot.glb · construction plane XZ.”
- “No valid placement surface.”
- “3 entities selected · active: DoorFrame.”
- “Editing is unavailable during Play.”

The hint must match the controller's actual state.
Do not show a snapping hint merely because a modifier key is down in a text field.
Transient success messages do not shift the canvas bounds.
Long error text belongs in a stable status region or Inspector error area.
An unresolved transaction error remains visible until resolved or deliberately dismissed.

## 37. Failure and recovery matrix

| Failure | Visible behavior | State outcome |
| --- | --- | --- |
| Stale pick | No selection jump | Current selection retained |
| Missing model geometry | Placeholder and loading/error label | Entity remains inspectable |
| Invalid numeric transform | Field-level error | Last valid authored state retained |
| Preview rejected | Error and reverted preview | Gesture canceled |
| Rollback rejected | Retry Discard action | Further authored edits blocked |
| Asset removed during drag | Blocked ghost and error | No entity created |
| Scene changes during drop | Drop canceled | New scene untouched |
| Device lost | Renderer error and recovery action | CPU document retained |
| Viewport resized to zero | Canvas interaction suspended | No invalid projection update |
| Focus lost during flythrough | Movement stops | No stuck keys |
| Window closed during drag | Capture and transaction cleanup | No hidden gesture owner |
| Unsupported parent decomposition | Action rejected with target information | Original transforms retained |

Recovery never silently reloads scene files from disk.
The accepted in-memory document remains the authoring authority until an explicit reload.
A render failure must not discard unsaved scene edits.
An import failure must not leave half of a hierarchy in the authored scene.

## 38. Acceptance walkthrough: place a modular room

Open a room scene containing a floor and a player start.
The Scene view shows the grid and retains its last useful editor pose.
Select a wall model in Assets.
Drag it into the canvas.
Observe a ghost aligned to the construction plane.
Enable translation snapping at one unit.
Move the ghost across several grid cells.
Drop it and observe the new wall selected in Hierarchy and Inspector.
Use Move to align the wall with the floor edge.
Use Rotate with fifteen-degree increments to turn it ninety degrees.
Duplicate the wall and place a second segment.
Undo once and observe only the second operation revert.
Save All.
Play and confirm that gameplay uses the authored game camera rather than the editor camera.
Stop and return to the same authoring pose.

Acceptance requires stable visual feedback throughout.
There must be no hidden placement at the origin after an invalid drop.
There must be no scene dirty mark caused only by orbiting the room.

## 39. Acceptance walkthrough: inspect a dense scene

Open a scene with overlapping props and a nested hierarchy.
Select one prop through Hierarchy.
Frame it with F.
Orbit around it without changing its transform.
Use the overlap picker at a crowded screen location.
Choose a named entity and observe the matching Hierarchy row.
Mark a large background object unpickable.
Confirm it remains visible but no longer wins ordinary picks.
Marquee a group of smaller props.
Change the active entity without discarding the group.
Move the group in World mode.
Switch to Local mode after the drag finishes.
Undo and observe exact original local transforms restored.
Clear the selection without resetting the editor camera.

This walkthrough exposes selection, pivot, and hierarchy defects more effectively than isolated icon demos.
Use it before adding more transform-tool variants.

## 40. Delivery sequence

Deliver a coherent user-visible increment at each stage.
Avoid adding every control before its behavior exists.

1. Separate editor-camera state from authored game-camera state.
2. Render an adaptive construction grid and origin axes.
3. Wire pan, orbit, dolly, zoom buttons, and Frame Selection to editor view state.
4. Add native canvas picking with shared Hierarchy/Inspector selection.
5. Add Move handles with one transaction per drag.
6. Add Rotate and Scale using the same gesture lifecycle.
7. Add Local/World, Pivot/Center, and explicit snap settings.
8. Bridge Assets dragging into native placement with a cancellable ghost.
9. Add marquee, overlap picking, visibility, and pickability.
10. Add overlay layout presets and custom-tool extension points after the core loop is usable.

Each stage is accepted through an actual scene-authoring workflow.
Browser interaction prototypes support discussion of layout and gestures.
Native Windows, Vulkan, focus, DPI, and drag behavior require their own implementation evidence.
Performance targets are hypotheses until measured on representative scenes.
The design is complete enough to guide implementation; it is not evidence that the editor already satisfies it.
