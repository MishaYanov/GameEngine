# Vulkan editor viewport: runtime and rendering design

Status: proposed implementation design, researched on 2026-11-06.
Audience: Engine Developer.
This chapter specifies how the Scene view should work inside the existing Angular/Tauri/Vulkan editor.
The chapter describes future work unless a subsection explicitly says **Current source**.
It does not claim that grid rendering, native transform handles, or viewport drag and drop are implemented.
The interactive mockup accompanying the design is a behavior model, not a Vulkan performance demonstration.

## 1. Architectural decision

Keep Angular responsible for editor chrome, inspectors, catalogs, and accessible alternatives.
Keep the native child window responsible for scene pixels, immediate navigation, and visual handles.
Keep Rust responsible for scene mutation semantics, persistent identity, undo history, and validation.
Introduce editor-only view state rather than routing navigation through the authored game camera.
Use one visible native surface for the first delivery.
Make the contracts capable of identifying that surface without implementing a docking framework first.
Treat performance figures below as acceptance targets to measure, not existing benchmark results.

### 1.1 The first user-visible result

- Opening a scene shows a quiet, stable grid beneath scene geometry.
- A user can orbit, pan, dolly, frame selection, and switch orthographic views.
- Navigating the editor leaves the scene's saved game-camera settings unchanged.
- Clicking geometry selects its entity and updates the existing Inspector.
- A transform handle starts one reversible edit gesture.
- Dragging a catalog model over the viewport shows an editor-only placement ghost.
- Releasing the model creates authored entities in one undoable operation.
- Pressing Escape removes the ghost or cancels the current transform.
- Losing focus releases capture and stops movement.
- Play uses the game's camera and hides authoring-only visual aids by default.

### 1.2 Boundaries that make this possible

| Layer | Owns | Does not own |
| --- | --- | --- |
| Angular workspace | Tool choice, property UI, asset catalog, keyboard alternatives | GPU resource lifetime or scene mutation rules |
| Native viewport controller | Input capture, immediate camera state, hovered handle, presentation requests | Saved transforms or game simulation behavior |
| Core editor session | Authoritative entities, selection, transactions, history | DOM layout or native cursor APIs |
| Renderer | Grid, scene geometry, selection decoration, handles, placement ghost | File dialogs or editor shortcut policy |
| Asset jobs | Validated loading and import preparation | Deciding that a cancelled drag should create entities |
| Standalone runtime | Game camera and gameplay input | Editor navigation preferences or editor-only overlays |

## 2. Current source and implementation gaps

The inspected tree includes staged, unstaged, and untracked work after commit `c3a3e95`.
The Git documentation baseline also names that commit; it is not evidence that every guide matches this tree.
The current source inventory below takes precedence over older milestone descriptions.
No production source is changed by this chapter.

### 2.1 Current Angular ownership

- `EngineViewportDirective` attaches a native child to a measured DOM aperture.
- Its source is `apps/editor/src/app/core/editor/engine-viewport.directive.ts`.
- It sends CSS logical client bounds, rather than multiplying them by `devicePixelRatio`.
- It observes resize, clipping, visibility, and layout changes.
- It reserves the single embedded viewport through `EditorService.reserveViewport()`.
- It forwards `editor://viewport-input` through `engineViewportInput`.
- `SceneViewport` forwards those events through its `viewportInput` output.
- Its source is `apps/editor/src/app/modules/editor/workspace/scene-viewport/scene-viewport.ts`.
- `SceneViewport` currently hides the child surface when a native HTML modal is detected.
- `EditorWorkspace` mounts one persistent `SceneViewport` element.
- `EditorWorkspace` currently does not bind that element's `viewportInput` output to tools.
- `SceneEditingService` already owns field drafts, coalescing, commit barriers, and cancellation.
- Its source is `apps/editor/src/app/modules/editor/workspace/scene-editing.service.ts`.
- That service is a useful reference for edit lifetimes, not the owner of every native pointer sample.

### 2.2 Current native ownership

- `ViewportHost` lives in `apps/editor-host/src-tauri/src/viewport.rs`.
- `ViewportHost::attach()` creates or reuses the native viewport worker.
- `ViewportHost::bounds()` updates the child aperture.
- `ViewportHost::invalidate()` wakes rendering after an accepted editor operation.
- `Control` carries latest bounds, visibility, invalidation, and stop state.
- `Worker::window_event()` receives winit pointer, wheel, keyboard, focus, and resize events.
- Pointer movement is currently coalesced before forwarding to Angular.
- `Worker::about_to_wait()` requests redraw only when work is needed.
- Play ticks are gated by surface activity and the session's `needs_tick()` result.
- `Worker::draw()` captures scene metadata and changes under the workspace mutex.
- GPU waits, uploads, acquisition, and presentation happen after releasing that mutex.
- Native input currently supplies physical child-window coordinates.
- The current worker does not implement this chapter's navigation/tool state machine.

### 2.3 Current camera behavior

- `Worker::draw()` obtains Edit-mode camera data from the authored scene document.
- In Play, it obtains the rendered world's camera.
- Core `frame_selection`, `orbit_camera`, `pan_camera`, and `dolly_camera` commands exist.
- Their capability descriptors identify them as undoable editor commands.
- `operations::apply()` changes `SceneData.camera` for those camera operations.
- Consequently, using these commands as ordinary Scene-view navigation changes authored data.
- The new Scene view must avoid that consequence.
- Existing authored-camera commands remain useful for an explicit camera-authoring operation.
- Renaming their meaning silently would break callers and historical expectations.

### 2.4 Current renderer foundation

- `WindowRenderer` lives in `crates/renderer/src/window_renderer.rs`.
- `WindowRenderer::prepare()` rebuilds size-dependent targets when required.
- `WindowRenderer::draw()` currently accepts a `CameraUniform` and render objects.
- `VulkanFrame` records rendering in `crates/renderer/src/vulkan/frame.rs`.
- The current depth clear value is `1.0`.
- The material pipelines use `CompareOp::LESS` for depth-tested scene geometry.
- `CameraUniform::perspective()` uses glam's right-handed Vulkan projection helper.
- Its source is `crates/renderer/src/vulkan/camera.rs`.
- `VulkanMaterialPipelines` already includes an overlay variant.
- That variant disables depth testing and depth writes for flagged objects.
- `RenderObject::screen_space()` and `RenderObject::unlit()` provide existing flags.
- Those facilities are a foundation; they do not implement grid, gizmo picking, or selection outlines.
- The initial viewport work should preserve existing scene/material pipeline compatibility.

## 3. State model

Use three different state lifetimes so that view navigation does not become content editing.
Use explicit names in public contracts; avoid the ambiguous field name `camera` without context.
Keep renderer-derived caches out of saved editor preferences.
Keep local preference files out of exported game content.

### 3.1 Authored scene state

- Entity identities, hierarchy, transforms, models, lights, and game-camera configuration are authored data.
- Scene JSON remains the source format for this milestone.
- These values participate in validation, dirty state, saving, packaging, and history.
- A scene edit uses the existing expected-revision contract.
- A failed scene edit does not retry blindly against a newer revision.
- Explicit game-camera authoring also participates in this contract.
- The Scene view cannot silently write its orbit position into the game camera.

### 3.2 Editor view preferences

- Perspective or orthographic projection belongs to the editor view.
- Orbit pivot and distance belong to the editor view.
- Editor camera orientation belongs to the editor view.
- Orthographic height belongs to the editor view.
- Grid visibility, plane, major-line ratio, and visual density belong to the editor view.
- Tool choice, pivot mode, and local/world orientation belong to the editor view.
- These settings can be restored per project and scene.
- They should not make a scene dirty or add entries to Undo.
- Their persistence uses a debounced preference write after navigation settles.
- A corrupt preference record falls back to a sensible frame-all view.
- Preferences use a versioned record so future fields can be migrated safely.

### 3.3 Transient interaction state

- Hovered entity and hovered handle are transient.
- Pointer capture owner and initial pointer position are transient.
- An in-progress placement ghost is transient.
- A pending asynchronous pick is transient.
- An in-progress numeric entry displayed by a handle is transient.
- Raw key-down state is transient.
- None of these values survives project replacement, worker destruction, or application restart.
- Restoring a stale capture state from disk would be a defect.

### 3.4 Proposed native view record

```rust
// Proposed, not an existing public API.
struct EditorViewState {
    viewport_id: ViewportId,
    workspace_generation: u64,
    scene_id: SceneId,
    view_revision: u64,
    camera: EditorCamera,
    grid: GridSettings,
    tool: ToolKind,
    orientation: ToolOrientation,
    pivot_mode: PivotMode,
    interaction: InteractionState,
}

struct EditorCamera {
    pivot: DVec3,
    orientation: DQuat,
    distance: f64,
    projection: EditorProjection,
    near: f32,
    far: f32,
}

enum EditorProjection {
    Perspective { vertical_fov_radians: f32 },
    Orthographic { vertical_height: f64 },
}
```

The record uses double precision for editor navigation calculations.
It does not change the existing scene transform format to double precision.
The renderer receives bounded, validated matrices converted to its supported GPU representation.
Coordinate rebasing remains an explicit later feature if measured scene scale requires it.
Do not promise arbitrary planet-scale editing from this state record alone.

## 4. Navigation processing

Navigation must not require a full scene snapshot round trip per pointer movement.
Process immediate camera deltas beside the native worker that owns the input surface.
Publish a small resulting view-state update to Angular when useful for labels or controls.
Angular toolbar changes use the same native view controller through typed requests.
Input policy remains explicit and configurable rather than hardcoded inside Vulkan rendering.

### 4.1 Common navigation path

1. The native window receives a pointer or wheel event.
2. The input controller resolves the active gesture and modifier policy.
3. The controller validates the active workspace and view generation.
4. It updates `EditorViewState` without mutating the scene document.
5. It increments `view_revision` and marks the surface dirty.
6. The worker renders the newest view state at the next useful frame boundary.
7. A compact view-state notification updates Angular controls after coalescing.
8. A preference write is scheduled when the gesture becomes idle.

### 4.2 Orbit

- Orbit around the stored pivot rather than recomputing a pivot from every hovered object.
- Begin with the selection's bounds center when using Frame Selection.
- Keep the world-up convention consistent with the engine's Y-up scene data.
- Clamp pitch before the singular pole for the first implementation.
- Treat free roll as an optional later preference, not an accidental quaternion side effect.
- Apply pointer deltas in logical navigation units after explicit DPI normalization.
- Clamp extreme single-event deltas after focus changes or pointer restoration.
- Preserve an uninterrupted rotation when an object passes behind the camera.
- Do not recenter orbit because selection changes during a gesture.

### 4.3 Pan

- Translate pivot and camera together along camera-right and camera-up.
- For perspective, scale movement by distance and field of view.
- For orthographic, scale movement by orthographic height.
- A one-pixel motion must have a predictable relationship to the displayed scene scale.
- Pan should neither rotate the view nor alter game-camera data.
- A pan gesture that begins on empty space must not create a selection rectangle.
- Crossing the child edge keeps the active capture policy until release or cancellation.

### 4.4 Dolly and zoom

- Perspective wheel input changes orbit distance by a multiplicative factor.
- Orthographic wheel input changes vertical view height by a multiplicative factor.
- Use bounded positive distances rather than allowing the camera to cross its pivot by accident.
- Distinguish line-wheel and pixel-wheel events before applying sensitivity.
- Accumulate small touchpad deltas instead of rounding each event to a notch.
- The visible plus/minus controls issue the same normalized navigation action.
- Modifier-assisted fine adjustment changes sensitivity, not the final unit system.
- Zoom-to-cursor can follow after ordinary pivot-based dolly is consistent.
- Zoom-to-cursor must preserve the cursor's ground-plane anchor in orthographic mode.
- Perspective zoom-to-cursor needs an explicit surface or plane anchor when no object is hit.
- A miss uses the orbit-pivot plane, not an arbitrary enormous ray distance.

### 4.5 Frame Selection

- Compute the union of world-space bounds for selected root entities and their visible descendants.
- Fall back to a small bounds box for entities without renderable meshes.
- Fit both viewport dimensions, not just vertical field of view.
- Add a modest framing margin so handles are not clipped at the edge.
- Perspective framing updates pivot and distance.
- Orthographic framing updates pivot and vertical height.
- Preserve orientation unless the user explicitly requests a named axis view.
- With no selection, offer Frame All through a separate command.
- Do not overload the existing authored `frame_selection` command silently.
- Introduce a view-targeted framing operation and migrate the toolbar intentionally.

### 4.6 Projection switching

- Named Top, Front, Back, Left, Right, and Bottom views use known editor orientations.
- Switching to orthographic preserves apparent scale near the orbit pivot.
- Switching back to perspective derives an equivalent distance from that apparent scale.
- Store the last useful free perspective orientation for returning from a named view.
- Avoid saving a temporary Top view as the game's authored camera.
- Axis labels remain readable at high DPI and do not rely on color alone.
- Projection labels in Angular reflect acknowledged native state rather than guessed local toggles.

## 5. Grid rendering

The first grid is an editor visualization, not a mesh saved into the scene.
It is visible in Scene view and omitted from standalone game rendering by default.
Grid settings control visibility independently of snapping settings.
Turning the grid off must not silently disable numeric snapping.

### 5.1 Recommended rendering approach

- Render an analytic plane grid in a dedicated editor pipeline.
- Reconstruct a world ray from inverse view-projection and fragment coordinates.
- Intersect the ray with the active work plane.
- Evaluate periodic line distances in that plane's two basis coordinates.
- Use fragment derivatives to keep visible line thickness stable.
- Blend minor and major lines with a quiet axis treatment.
- Reject fragments with no valid forward plane intersection.
- Fade lines before numerical noise dominates at a near-parallel horizon.
- Keep line meshes as a diagnostic fallback, not the default unbounded-grid representation.
- Do not allocate or regenerate thousands of world-space line vertices while zooming.

The derivative operation used by this proposal is grounded in the GLSL definition of `fwidth`; the line style and transition policy are original design choices. [Khronos GLSL specification](https://registry.khronos.org/OpenGL/specs/gl/GLSLangSpec.4.60.html).

### 5.2 Work planes

- Perspective Scene view defaults to the XZ ground plane at Y = 0.
- Top and Bottom orthographic views use the XZ plane.
- Front and Back orthographic views use the XY plane.
- Left and Right orthographic views use the YZ plane.
- A user can choose a custom work-plane offset.
- A custom plane is an editor preference until used to author an entity transform.
- Its basis must be orthonormal and finite before reaching the shader.
- A plane offset cannot be inferred from whichever model was most recently imported.

### 5.3 Adaptive spacing

- Begin with a configurable base spacing of one scene unit.
- Adopt one scene unit as one metre for starter-project authoring guidance, as a proposed convention rather than an existing enforced engine unit.
- Keep imported-asset scale conversion explicit so that convention does not silently rescale existing scenes.
- The initial major-line ratio is ten minor cells per major cell.
- Choose a visual subdivision level from projected spacing near the view pivot.
- Keep useful lines approximately tens of physical pixels apart, rather than subpixel dense.
- Blend between adjacent subdivision levels while zooming.
- Use hysteresis around level transitions to avoid flicker during tiny camera motions.
- Show the effective displayed grid spacing in the viewport status strip.
- Keep the configured snap increment visible separately.
- Visual spacing may grow to ten or one hundred units without changing snap increments.
- Close-range visual spacing may shrink to tenths or hundredths of a unit.
- Clamp the exponent used by the adaptive algorithm to an explicit supported range.
- Treat an invalid spacing as a configuration error and restore a default.

### 5.4 Depth behavior

- Draw scene geometry with its existing depth convention.
- Draw the grid with depth testing enabled and depth writes disabled.
- Write the plane intersection's projected depth, rather than the fullscreen triangle's depth.
- Use the same projection matrix and depth mapping as scene geometry.
- A foreground object should hide the grid behind its visible surface.
- A model lying on the work plane should not show severe z-fighting through its faces.
- Prefer a small documented depth bias or a carefully ordered floor treatment when needed.
- Avoid changing the entire renderer to reverse Z during the first grid milestone.
- Reverse Z can be evaluated separately with camera, frustum, picking, and shader tests together.
- A grid shader may not assume an OpenGL -1 to 1 depth range.

Vulkan's viewport depth mapping and shader depth output must agree; this is an API requirement, not a styling choice. [Vulkan depth guide](https://docs.vulkan.org/guide/latest/depth.html).

### 5.5 Shader sketch

```glsl
// Proposed algorithm sketch; names and bindings are not a committed shader ABI.
vec3 nearPoint = unproject(fragmentNdc.xy, 0.0, inverseViewProjection);
vec3 farPoint = unproject(fragmentNdc.xy, 1.0, inverseViewProjection);
vec3 ray = farPoint - nearPoint;
float denominator = dot(planeNormal, ray);
bool validDenominator = abs(denominator) > planeEpsilon;
float t = validDenominator
    ? dot(planeNormal, planeOrigin - nearPoint) / denominator
    : -1.0;
vec3 world = nearPoint + t * ray;
vec2 planeUv = vec2(dot(world - planeOrigin, planeU),
                    dot(world - planeOrigin, planeV));
vec2 cell = planeUv / displayedSpacing;
vec2 derivative = max(fwidth(cell), vec2(derivativeEpsilon));
vec2 edgeDistance = abs(fract(cell - 0.5) - 0.5) / derivative;
float line = 1.0 - clamp(min(edgeDistance.x, edgeDistance.y), 0.0, 1.0);
// Compute derivatives before fragment-varying discard decisions.
if (!validDenominator || t < 0.0 || t > 1.0) discard;
vec4 clip = viewProjection * vec4(world, 1.0);
gl_FragDepth = clip.z / clip.w;
outColor = vec4(gridColor, line * distanceFade(world));
```

The exact reconstruction must be tested against the selected glam projection convention.
The sketch does not cover an infinite far plane or reverse-Z variant.
It deliberately avoids treating the shader as ready-to-paste production code.
Derivative calculations must not be placed only inside divergent per-fragment branches.
Finite checks and near-horizon behavior require GPU tests on supported drivers.

### 5.6 Precision and scale

- Keep camera/pivot math in double precision on the CPU where practical.
- Convert the work-plane origin near the camera to stable GPU coordinates.
- Distinguish a visual grid-origin adjustment from rebasing authored entity coordinates.
- Very large authored translations remain limited by existing f32 transform storage.
- Do not conceal that limit by displaying more decimal places in the Inspector.
- Include scenes at small, ordinary, and large coordinate magnitudes in visual acceptance.
- The initial grid should fail quiet: fade away rather than emit NaNs or bright streaks.
- A nearly edge-on work plane is allowed to become visually sparse.

## 6. Editor overlay rendering

Editor visuals must be passed as editor data rather than inserted into the authored entity catalog.
The selection outline, transform handle, placement ghost, and orientation widget share this principle.
The existing overlay pipeline is reusable where its depth behavior is appropriate.
It is not sufficient for every editor pass because a grid needs depth testing without depth writes.

### 6.1 Proposed frame order

1. Prepare swapchain and size-dependent resources.
2. Capture the newest stable scene packet and view state.
3. Draw ordinary opaque scene geometry.
4. Draw the work-plane grid against scene depth.
5. Draw depth-tested selection decoration.
6. Draw translucent placement preview with an explicit supported blend policy.
7. Draw visible transform handles.
8. Draw optional occluded-handle hints with lower visual emphasis.
9. Draw screen-space orientation and viewport annotations.
10. Present without a synchronous picking readback.

### 6.2 Renderer contract

```rust
// Proposed editor-capable draw request.
struct ViewDrawRequest<'a> {
    camera: &'a ViewCameraUniform,
    scene_objects: &'a [RenderObject<'a>],
    editor: Option<&'a EditorOverlayPacket>,
}

struct EditorOverlayPacket {
    grid: Option<GridDraw>,
    selection: SelectionDraw,
    gizmo: Option<GizmoDraw>,
    placement: Option<PlacementDraw>,
    orientation_widget: OrientationWidgetDraw,
}
```

`editor: None` keeps the ordinary runtime path straightforward.
The renderer consumes validated packets; it does not issue scene commands.
Packet data must not keep a workspace mutex locked through GPU submission.
Packet resources retire only after their last GPU use has completed.
Do not allocate a new Vulkan pipeline for every selected object or pointer sample.

### 6.3 Resource ownership

- Shared handle meshes belong to the editor renderer's lifetime.
- Grid pipeline resources belong to the renderer and attachment format.
- Per-frame handle transforms belong to frame scratch data.
- Pick readback buffers belong to bounded frame slots.
- Selection keys belong to editor state; the renderer receives resolved draw references.
- A placement preview may hold an imported asset reference while it is visible.
- Cancelling placement releases that preview reference after safe frame retirement.
- Worker teardown retains existing resource ordering: dependants before device ownership.
- Device-idle waits are reserved for operations that actually require broad retirement.

Fences and pipelining are the appropriate foundation for asynchronous readback; a per-hover device-idle wait would undermine responsive editing. [Khronos synchronization sample](https://docs.vulkan.org/samples/latest/samples/performance/wait_idle/README.html).

## 7. Transform handle geometry and hit testing

Use native drawing and native hit testing for handles over the native scene surface.
Send their semantic editing intent to the authoritative Rust editor session.
Do not position an HTML handle over a child HWND and assume CSS stacking will make it usable.

### 7.1 Screen-size policy

- Keep the principal handle length near a stable logical-pixel size.
- Convert that size to physical pixels using the active window scale factor.
- In perspective, derive world scale from camera distance, field of view, and viewport height.
- In orthographic, derive world scale from orthographic height and viewport height.
- Clamp size near the camera so the handle cannot engulf the entire viewport.
- Hide or simplify handles whose pivot is behind the near plane.
- Keep visible geometry smaller than its pointer hit region.
- Use a target region that remains practical at high DPI and with ordinary motor imprecision.
- Provide numeric property controls as a precise alternative.

### 7.2 Hit priority

1. An active captured gesture continues to own input.
2. An orientation-widget hit owns its explicitly reserved region.
3. A visible active-tool handle hit has priority over scene geometry.
4. A plane or center handle follows the tool's documented tie-breaking policy.
5. Scene selection is considered after tool handles miss.
6. Empty-space interaction follows navigation or marquee policy.

### 7.3 Translation constraints

- Axis movement projects a drag onto one world or local basis vector.
- Plane movement projects onto the selected pair of basis vectors.
- Screen-plane movement uses a plane through the pivot facing the editor camera.
- Derive every sample from the gesture's original transforms and current total drag.
- Avoid accumulating f32 deltas into already-previewed transforms.
- Near-parallel rays use a stable fallback plane chosen at gesture start.
- Do not switch fallback planes halfway through a drag without an explicit continuity rule.
- Snap the constrained result using the configured snap increment.
- Display an offset readout along the constrained axes.

### 7.4 Rotation and scale constraints

- Rotation rings use a plane normal matching their chosen axis.
- Unwrap angular motion across the -pi/pi boundary.
- Derive rotations from the original orientation rather than summing rounded Euler components.
- Convert to the engine's supported transform representation at the mutation boundary.
- Nonuniform scale acts on the documented local basis for the initial implementation.
- World-oriented nonuniform scaling under rotated parents can require shear.
- If a requested result is not representable by the engine's TRS model, reject it clearly.
- Zero and negative scale are currently accepted by the backend; UI policy must be explicit.
- Preserve numeric authoring compatibility rather than silently changing backend scale validity.
- Mirrored or singular transforms need visual and picking acceptance cases.

### 7.5 Multi-selection

- Select multiple keys in authoritative editor state.
- Compute a documented shared pivot, using active entity or selection center.
- Operate on selected roots so a selected child is not transformed twice with its parent.
- Use the existing hierarchy-aware transform semantics where they match the tool operation.
- Keep the original selection stable for the lifetime of a captured gesture.
- An external selection change cancels the gesture before starting another target set.
- The Inspector can show mixed values while handles still operate on the whole selection.
- A multi-selection drag produces one history entry.

## 8. Picking strategy

Start with CPU ray picking using existing model bounds and the editor view camera.
Keep the visual and picking camera matrices identical for the selected view revision.
Do not require GPU ID picking before the first usable editor viewport ships.
Add an optional GPU path when triangle accuracy or measured CPU cost justifies it.

### 8.1 Current CPU path

- Core `pick_selection` already validates bounded pixel coordinates and extent.
- It currently constructs its ray from the authored scene camera.
- It intersects transformed model bounds rather than providing exact per-triangle visible-surface picking.
- Singular world matrices are skipped because they cannot be inverted.
- The current command can modify selection without mutating authored transforms.
- This implementation is a starting point, not a reason to keep the wrong camera source.
- Refactor ray construction into a reusable helper that accepts a validated view ray or matrix.
- Preserve scene identity and expected-revision validation around the selection result.

### 8.2 Proposed CPU request

```rust
// Proposed view-targeted selection request.
struct PickRequest {
    viewport_id: ViewportId,
    workspace_generation: u64,
    scene_id: SceneId,
    view_revision: u64,
    authored_revision: u64,
    pixel: [f64; 2],
    extent: [u32; 2],
    operation: SelectionOperation,
}
```

The native host resolves the matching camera snapshot and constructs a ray.
Do not trust a WebView-supplied object key as proof that a geometry hit occurred.
Selection commands can still originate from the hierarchy through their separate known-key path.
Reject a pick request whose viewport extent is zero or whose coordinates are outside the aperture.
When camera or scene revisions have changed, discard or recompute the pick deliberately.
Never apply an old asynchronous hit to a newly opened project with the same entity names.

### 8.3 GPU ID picking, later phase

- Render stable per-draw IDs into an integer attachment when a pick is requested.
- Keep zero as background and map nonzero IDs through that frame's entity-key table.
- Match material culling, alpha policy, instance expansion, and depth rules to visible rendering.
- Copy only the needed pixel or small region to a bounded readback slot.
- Observe completion through the appropriate frame fence.
- Apply host-memory invalidation where required by the memory type.
- Associate the result with workspace generation, scene, view revision, and request sequence.
- Ignore results from superseded hover requests.
- Avoid blocking the render thread until a hover result is available.
- Keep CPU picking available when the optional ID attachment cannot be created.
- Marquee selection may initially use projected bounds rather than a large GPU readback.

GPU-to-host visibility and completion require explicit synchronization; a mapped pointer alone is not a completed pick result. [Vulkan synchronization examples](https://docs.vulkan.org/guide/latest/synchronization_examples.html).

## 9. Scene mutation and gesture transactions

The semantic transaction belongs to the Rust editor, not to a canvas-local undo stack.
Reuse existing begin, transform, commit, and cancel commands for the first gesture implementation.
A later transient-preview layer may reduce scene cloning cost if measurements identify it as necessary.
Do not introduce that extra mutation system before the current path has a working measured baseline.

### 9.1 First implementation

1. Resolve the handle and target selection at pointer-down.
2. Capture original transforms and the initial pointer ray.
3. Send one `begin_transaction` command.
4. Compute constrained total-drag transforms from the originals.
5. Keep at most one transform request in flight and one latest pending request.
6. Apply accepted transforms through the existing authoritative editor commands.
7. Let normal native invalidation redraw accepted scene changes during the transaction.
8. On release, flush the latest pending transform and commit once.
9. On Escape, drop pending transforms and cancel the owned transaction.
10. Release pointer capture after commit or cancellation has entered a safe terminal state.

### 9.2 Ownership and failure rules

- A gesture owns only the transaction token it opened.
- A late failure response may follow a successful native begin; inspect recovered authoritative state.
- Do not clear ownership before determining whether that transaction needs cancellation.
- A failed rollback blocks new authoring gestures and offers a visible retry action.
- Do not send a second field or handle edit into a failed previous transaction.
- Save, Play, scene switching, and project replacement require the gesture to settle first.
- A commit barrier reserves authoring input until the following operation has captured its prerequisites.
- Navigation can continue while an unrelated background build compiles already-staged files.
- A scene switch increments view context and invalidates pending input/results from the prior scene.
- Workspace destruction cancels previews and attempts transaction rollback through the surviving host lifetime.

### 9.3 Proposed preview optimization boundary

- A render-only preview transform map may become useful for very large selections.
- Such a map must be explicitly owned by a gesture token and excluded from save/export data.
- Picking must either see the preview or be suspended until the gesture ends.
- The final authoritative transform command must validate the complete selected-root result.
- Cancellation simply removes the preview map only if no authored edits were applied.
- Mixed preview and authored mutation ownership must not become ambiguous.
- The first implementation should not ship both paths unless measurements justify the complexity.

## 10. Native/WebView composition

The editor currently embeds a native child rather than rendering Vulkan into a DOM texture.
That choice provides direct native rendering but imposes a real composition boundary.
The design must respect that boundary instead of relying on CSS `z-index` guesses.

### 10.1 Ownership of visible controls

- Keep primary tool buttons in the Angular toolbar outside the child aperture.
- Keep Inspector and Assets in ordinary Angular panes.
- Draw the in-viewport orientation widget and transform handles natively.
- Draw placement ghosts and marquee rectangles natively.
- Put detailed tool settings in Angular panels outside the child aperture.
- Reserve an explicit native overlay region if a small in-viewport tool strip is required later.
- Do not create a separate native window for every handle or label.
- Use the same editor palette and logical sizing tokens across both drawing systems.
- A temporary DOM popover overlapping the child must use a tested suppression or placement policy.

Windows child windows have their own visible/update regions and sibling clipping rules; DOM stacking is not a substitute for native composition control. [Microsoft child-window documentation](https://learn.microsoft.com/en-us/windows/win32/gdi/child-window-update-region).

### 10.2 Modal behavior

- A modal begins by reserving the affected editor action.
- The current pointer gesture settles or cancels according to the action's policy.
- Hide the native child before presenting a DOM modal that overlaps its rectangle.
- Preserve GPU resources while merely suppressing the surface.
- Restore the aperture and request redraw after the modal closes.
- Restore keyboard focus to an appropriate editor control rather than leaving hidden native capture active.
- Native OS file dialogs remain separate windows but still require input-state cleanup.
- Popup and context-menu behavior must be exercised on the actual Windows host.

### 10.3 Docking and layout

- Continue using the existing bounds refresh after layout movement without a resize.
- A moved pane needs updated client coordinates even when its dimensions are identical.
- Clip the child aperture to visible ancestors before creating native bounds.
- Zero-size and hidden apertures suspend rendering and input.
- Keep resize handles in a reachable area outside or beside the child aperture.
- A large pointer hit target must not be silently covered by the child HWND.
- Test Assets drawer resizing while a handle is hovered and while a camera gesture is active.
- Future tear-off views need explicit worker/window ownership; they are not part of this milestone.

## 11. Drag and drop across the boundary

Treat in-editor catalog dragging and external operating-system file dropping as separate transports.
Both can produce the same validated placement intent after their inputs are resolved.
Do not assume an HTML drag event will continue naturally over a native child window.

### 11.1 Internal catalog drag

1. Angular starts an internal drag session with an opaque drag ID and an asset reference.
2. The host validates that reference against the open project's catalog and generation.
3. The native controller receives enter/move/leave/end state for that drag ID.
4. A placement ghost uses the current editor-camera ray and placement policy.
5. A valid release requests an authored import/instantiate operation.
6. An invalid release or Escape ends the session without authored entities.
7. Completion selects the newly created entities through the authoritative result.

### 11.2 Placement computation

- Prefer a scene-surface hit when surface placement is enabled and a supported hit exists.
- Fall back to the selected work plane when surface placement misses.
- If both fail, retain the last valid ghost and show that the current drop is invalid.
- Snapping applies to the resolved placement transform, not to raw screen coordinates.
- Keep vertical offset and surface-normal alignment explicit options.
- Avoid silently rotating every imported model to a surface normal by default.
- Preview bounds must remain visible even before the full asset finishes GPU upload.
- A loading ghost cannot be dropped as an empty permanent entity without clear feedback.
- Multiple selected assets use a documented arrangement rather than all occupying one origin by surprise.

### 11.3 External file drops

- Resolve an OS drop through the native event path appropriate to the actual target window.
- Separate importing a file into project assets from placing an existing catalog asset.
- A `.gltf` source may depend on buffers and images; copying one filename is not sufficient.
- Keep first-delivery external import scope explicit if only self-contained `.glb` is supported.
- Show validation/import progress before allowing placement of unavailable content.
- Dropping a scene file should not silently replace an unsaved scene.
- Do not treat operating-system file paths as project-relative asset identities.
- A failed import leaves the authored scene unchanged and preserves a useful error message.

Tauri's WebView drop API reports physical positions relative to the WebView; the native child uses its own client coordinate system, so conversion is required. [Tauri WebView API](https://v2.tauri.app/reference/javascript/api/namespacewebview/).

## 12. Coordinates, DPI, and capture

Define coordinate spaces at each API boundary and encode the units in type names or documentation.
Do not pass a generic `{x,y}` object through several layers with an assumed meaning.

| Value | Space | Conversion owner |
| --- | --- | --- |
| DOM aperture | CSS logical client pixels | Angular measurement |
| Child position/size | Parent physical client pixels | Native host using scale factor |
| Native pointer position | Child physical client pixels | winit event |
| WebView OS-drop position | WebView physical client pixels | Drop bridge |
| Handle design size | Logical pixels | Native view presentation |
| Pick pixel | Actual render-target physical pixel | Native input adapter |
| World ray | Scene world units | View/picking module |

### 12.1 Conversion rules

- Translate a WebView drop position by the physical child origin before hit testing.
- Account for clipped apertures instead of assuming the original DOM box remains fully visible.
- Use the actual prepared surface extent for unprojection.
- Normalize only after validating a nonzero extent.
- Preserve fractional pointer coordinates until the final pixel-readback conversion.
- Do not multiply an already physical winit pointer coordinate by DPI again.
- Recompute geometry after a scale-factor change or monitor transition.
- Cancel or rebase an active gesture when its coordinate basis changes materially.
- Include viewport extent and view revision in asynchronous picking requests.

### 12.2 Capture lifecycle

- Capture starts only after an explicit drag threshold or navigation gesture is recognized.
- Captured gestures continue without depending on DOM pointer events over the child.
- The native controller tracks the owner button and gesture token.
- Release capture on matching button-up, Escape, focus loss, surface hide, project switch, or shutdown.
- Clear movement keys and modifiers when focus is lost.
- Restore cursor visibility after a navigation mode that hid it.
- If cursor confinement or locking is unsupported, use a documented bounded-drag fallback.
- Never leave a failed cursor-grab attempt as a half-active tool state.

winit exposes physical/logical scaling and cursor-grab operations, and reports that scale factors can change as a window moves between displays. [winit Window API](https://docs.rs/winit/0.30.13/winit/window/struct.Window.html).

## 13. Rendering cadence and latency

Retain on-demand rendering for a static editor scene.
Request continuous useful frames during camera movement, handle dragging, placement, or active simulation.
Return to waiting once the interaction and any smoothing have settled.
Do not run an uncapped busy loop just because the editor is open.

### 13.1 Wake reasons

- Scene document changes.
- Selection decoration changes.
- Editor camera changes.
- Grid/tool display changes.
- Pointer hover changes a handle highlight.
- Placement preview changes.
- Surface bounds or DPI changes.
- A requested GPU pick requires a frame.
- An asset becomes ready for rendering.
- Play simulation advances.
- A presentation retry becomes eligible.

### 13.2 Queue policy

- Keep the latest camera navigation state rather than one queued command per raw motion event.
- Preserve ordered begin/end/cancel boundaries for authored gestures.
- Keep one transform command in flight and at most one latest pending transform.
- Never coalesce a commit into a transform sample or a cancellation into an unrelated gesture.
- Bound pending pick requests and replace obsolete hover picks.
- Allow click picks to retain their own request sequence while later hover requests are discarded.
- Avoid cloning or serializing the entire document for a camera-only change.
- Do not perform directory scans inside pointer-move processing.
- Do not wait for asset import completion while holding the workspace mutex.

### 13.3 Proposed acceptance targets

These are design targets for a documented reference machine, not current measured claims.
Record hardware, driver, scene, power mode, monitor refresh, and build mode with every measurement.

| Operation | Initial target | Evidence required |
| --- | --- | --- |
| Native camera sample to frame submission | p95 within one 60 Hz frame budget | Timestamped input and submit correlation |
| Inspector accepted edit to visible update | p95 within two 60 Hz frame budgets | Command receipt and rendered revision |
| Empty-scene hover processing | No full-scene serialization | Transport trace |
| Handle drag command backlog | One in flight, one latest pending | Queue counters under stress |
| Idle static Scene view | No continuous draw loop | Frame counter over an idle interval |
| Cancelled drag | No committed history patch | History and authored-data comparison |
| Background asset import | Navigation remains responsive | Concurrent import/navigation trace |

### 13.4 Measurement points

- Input arrival timestamp on the native worker.
- Gesture interpretation completion timestamp.
- Scene command enqueue and acknowledgement timestamps.
- Latest applied editor view revision.
- Latest rendered scene revision.
- CPU frame preparation duration.
- GPU frame duration when supported.
- Present completion or display latency only when a valid measurement method is available.
- Queue depths, dropped obsolete samples, and stalled transaction time.
- Asset upload work and full-resync counts during a controlled editing session.

## 14. Play, Scene view, and standalone behavior

The first implementation should keep the existing single-surface Scene/Game mode gating.
Do not imply simultaneous independent Scene and Game cameras are already available.
Make simultaneous preview a later milestone with an explicit simulation-owner design.

### 14.1 Enter Play

1. Settle the active authored gesture.
2. Preserve editor camera and tool preferences in editor state.
3. Start the existing runtime play session.
4. Switch the visible surface to the runtime camera.
5. Disable ordinary authoring handles and placement.
6. Route gameplay input only while the game surface owns focus.
7. Keep Stop and pause controls reachable outside the child surface.

### 14.2 Stop Play

- Clear held gameplay input before returning focus to authoring.
- Restore the editor camera exactly rather than copying the game's final camera.
- Restore the prior editor selection if its authored keys still exist.
- Recompute selection decoration against authored transforms.
- Do not persist simulation-only transforms into scene JSON automatically.
- If an eventual Apply Play Changes feature is desired, design it as an explicit reviewed operation.

### 14.3 Future simultaneous Scene/Game views

- Give each camera and surface a distinct `ViewportId`.
- Keep one authoritative simulation scheduler per play session.
- Do not tick simulation once per visible viewport.
- Separate simulation visibility from whether the Scene tab currently covers a surface.
- Capture immutable render packets for each view without duplicating authored ownership.
- Decide focus/input routing before adding a second native surface.
- Preserve single-view behavior as the compatibility baseline.

## 15. Proposed implementation modules

These are suggested responsibilities; final filenames may follow the repository's accepted naming conventions.
Existing public contracts should change through explicit versioned additions where required.

| Area | Suggested change |
| --- | --- |
| `crates/engine-core` | Reusable validated picking ray and hierarchy-aware transform helpers |
| `crates/runtime` | View-independent selection/query access where existing editor APIs require authored camera |
| `crates/bridge` | View IDs, camera-state requests, gesture IDs, and bounded acknowledgements |
| `apps/editor-host` | `editor_view` state/controller beside native viewport worker |
| `apps/editor-host` | Native gesture ownership and drop-session routing |
| `crates/renderer` | Explicit optional editor overlay packet and dedicated grid pipeline |
| `crates/renderer/shaders` | Grid vertex/fragment shaders and existing shader build integration |
| Angular `core/editor/workspace` | Typed view preferences and acknowledged view-state signals |
| Angular `scene-viewport` | View control outputs and focus/status integration |
| Angular workspace toolbar | Navigation, projection, tool, pivot, and orientation controls |
| Angular Assets panel | Internal drag start and keyboard Place action |

### 15.1 Delivery sequence

1. Introduce editor camera state and migrate Frame Selection to the view-targeted path.
2. Add native orbit/pan/dolly and visible plus/minus controls.
3. Add perspective/orthographic switching and named axis views.
4. Add the depth-aware adaptive grid and independent visual/snapping preferences.
5. Connect viewport picking to the existing authoritative selection and Inspector.
6. Add translation handles with one transaction per drag.
7. Extend handles to rotation and scale with representability checks.
8. Add internal catalog drag sessions and placement ghosts.
9. Add surface placement and explicit supported external import drops.
10. Measure representative scenes before selecting additional optimization work.

### 15.2 Compatibility checkpoints

- Existing numeric Inspector edits continue to use the same Rust mutation semantics.
- Existing scene JSON remains loadable and exportable.
- Existing standalone rendering does not acquire editor grid or handle dependencies.
- Existing authored camera commands remain available for explicit camera authoring.
- Existing layout preferences retain their meaning.
- Existing stale-revision rejection remains active for scene changes.
- Navigation preferences can be reset without altering project content.
- Browser mode remains an honest fallback without claiming Vulkan rendering.

## 16. Concrete acceptance scenarios

### 16.1 Navigation does not author content

Given a saved scene with a configured game camera, orbit, pan, zoom, and switch to Top view.
The visible Scene view changes immediately.
The authored scene camera remains byte-for-byte equivalent after canonical serialization.
The scene remains clean and Undo still refers to the last content edit.
Restarting the editor restores the chosen view preference without changing the exported game camera.

### 16.2 Grid and geometry agree

Place a cube on the XZ plane, orbit near the horizon, and zoom through several grid levels.
The cube occludes grid lines behind its visible faces.
The grid does not become a bright solid band at the horizon.
Displayed grid spacing transitions smoothly while snap increment remains unchanged.
Top view aligns the cube and grid without a vertical coordinate flip.

### 16.3 Handle editing remains reversible

Select a parent and child, drag the X handle, then press Escape before releasing the pointer.
Both entities return to their original authored transforms.
No new committed history entry appears.
Repeat the drag and release normally; Undo restores the entire gesture in one action.
The child is not moved twice because its selected ancestor also moved.

### 16.4 DPI and aperture changes remain coherent

Move the editor between monitors with different scaling, then resize the Assets drawer.
The native child still aligns with the Scene pane.
The same visible mesh location can be selected without a scale-dependent pointer offset.
Handle hit targets retain comparable logical size.
A gesture interrupted by the display change cancels or rebases according to the documented rule.

### 16.5 A cancelled drag leaves no content behind

Start dragging a catalog model into the Scene view and wait for its preview to become visible.
Move outside the aperture and press Escape.
The placement ghost disappears and no authored entity is added.
The asset job may finish into a cache, but it cannot complete an abandoned placement command.
Starting a new drag receives a new token and cannot consume the old result.

### 16.6 Stale results cannot cross projects

Start a delayed pick or placement preparation, then open another copy of the same project manifest.
The workspace generation changes even if scene and entity keys are identical.
The old result is rejected and cannot change selection or create entities in the new workspace.
The status message identifies cancellation or supersession without a misleading success toast.

### 16.7 Native failure does not trap input

Begin camera navigation, then simulate surface loss or worker failure.
The cursor is visible, capture is released, and movement keys are cleared.
The Inspector and Stop/recovery controls remain reachable.
The editor shows a recoverable viewport error rather than leaving an invisible captured surface.
Retrying attachment starts with an idle interaction state and the last valid view preference.

## 17. Verification boundaries for this design

- Source inspection confirms the existing functions and state boundaries described in section 2.
- Referenced API documentation supports the external platform and graphics facts cited nearby.
- The proposed grid algorithm has not been compiled as an engine shader in this design task.
- No native DPI, pointer capture, drag/drop, Vulkan visual, or latency acceptance is claimed here.
- No existing Angular or Rust application test result proves these proposed features are implemented.
- The next implementation should record actual evidence against the acceptance scenarios above.
- Keep documentation status as proposed until the corresponding code and desktop checks exist.

## 18. Research references

- [Vulkan depth guide](https://docs.vulkan.org/guide/latest/depth.html): depth coordinate and output conventions.
- [Vulkan depth buffering tutorial](https://docs.vulkan.org/tutorial/latest/07_Depth_buffering.html): separate depth-test and depth-write controls.
- [Khronos GLSL specification](https://registry.khronos.org/OpenGL/specs/gl/GLSLangSpec.4.60.html): derivative operations used by analytic line coverage.
- [Vulkan synchronization examples](https://docs.vulkan.org/guide/latest/synchronization_examples.html): GPU/host visibility and readback completion.
- [Khronos CPU/GPU synchronization sample](https://docs.vulkan.org/samples/latest/samples/performance/wait_idle/README.html): avoiding unnecessary whole-device idle synchronization.
- [winit 0.30.13 Window API](https://docs.rs/winit/0.30.13/winit/window/struct.Window.html): DPI scale, cursor position, and grab capabilities.
- [winit cursor-grab modes](https://docs.rs/winit/0.30.13/winit/window/enum.CursorGrabMode.html): confinement and locking modes and platform support limits.
- [Tauri WebView API](https://v2.tauri.app/reference/javascript/api/namespacewebview/): external drag/drop payloads and physical coordinate space.
- [Microsoft child-window regions](https://learn.microsoft.com/en-us/windows/win32/gdi/child-window-update-region): native clipping and sibling composition behavior.
