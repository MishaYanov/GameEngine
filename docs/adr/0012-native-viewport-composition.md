# ADR-0012: Native viewport composition

- **Status:** Proposed
- **Date:** 2026-10-27
- **Scope:** Native rendering surfaces, editor overlays, focus, geometry, and resource lifetime.

## Context

A user opens an object menu over the rendered scene, drags a pane to another monitor, then opens a second camera view.
HTML stacking, native window stacking, GPU resource lifetime, and input ownership must agree. A CSS `z-index` change
cannot by itself establish that agreement for a native child window.

Retain the existing Windows child viewport as an explicit limited rendering mode. Define the ownership and input
contract now, and require a composition prototype before promising arbitrary HTML overlays or multiple embedded views.

## Current implementation

The following files were inspected at commit `fcb748b04853209b5e17e680c38c847c19f7f365` and in the current working tree:

| Evidence                                                       | Current behavior                                                                                                                                                                                                          |
|----------------------------------------------------------------|---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `apps/editor/src/app/shared/components/portal/portal.ts`       | Wraps the native directive and hides it while a `dialog:modal` is present.                                                                                                                                                |
| `apps/editor/src/app/core/editor/engine-viewport.directive.ts` | Reserves a viewport, observes geometry/visibility, coalesces updates, clips a rectangular aperture against ancestors, and detaches on failure or destruction.                                                             |
| `apps/editor/src/app/core/editor/editor.service.ts`            | Supplies the bridge and a single viewport reservation.                                                                                                                                                                    |
| `apps/editor-host/src-tauri/src/viewport.rs`                   | Windows-only native child creation, a rendering worker, DPI conversion, native input events, and coordinated shutdown before parent-window destruction. Moving an attached viewport to another editor window is rejected. |

The native worker clears Play input on focus loss and occlusion. These are existing safeguards, not evidence that all
overlay, capture, accessibility, or multi-monitor cases have passed. Browser-only design previews do not exercise the
native composition path.

## Dependencies and assumptions

[ADR-0003](0003-rust-typescript-bridge.md) governs the control/status
boundary. [ADR-0004](0004-authoring-play-and-editor-state.md) defines which scene or Play session a view
observes, [ADR-0005](0005-runtime-lifecycle-and-scheduling.md) owns simulation timing,
and [ADR-0010](0010-input-actions-and-possession.md) owns focus and action
routing. [ADR-0006](0006-asset-identity-and-import-pipeline.md) and [ADR-0007](0007-graph-assets-and-execution.md)
govern assets and programs consumed by rendering. These are coordinated proposals rather than implemented multi-view
support.

## Proposed decision

### Separate the view from the rendering surface

Model a logical `ViewportId` with its scene/session binding, camera, display mode, visibility, input context, and
generation. A native surface is a replaceable host resource associated with that view. Angular owns layout and view
controls; the host owns native windows, GPU surfaces, swapchains, synchronization, and destruction. Never expose native
handles through ordinary editor DTOs.

A view displays an existing simulation session; opening another view must not start another tick loop. Camera navigation
can remain editor state without mutating a scene camera. Playing through a scene camera uses an explicit binding. Frame
presentation and simulation scheduling must therefore remain separable even though current worker code integrates them.

In the first implementation, retain one embedded view and return a clear unsupported error for additional reservations.
Introduce IDs and generations before enabling multiple views so delayed bounds updates, picks, or shutdown completions
cannot affect a replacement surface.

### Treat Windows child-window airspace as a product constraint

The current native child does not participate in DOM painting or CSS clipping. Preserve a rectangular aperture and hide
it when an overlay must occupy its pixels. A modal observer is a useful existing safeguard, but does not cover menus,
tooltips, drag previews, or every future overlay. Centralize an explicit overlay/occlusion lease that can suppress
presentation and release input; ordinary components should not independently manipulate native window order.

Place persistent HTML toolbars outside that aperture in the initial mode. Do not promise native rounded corners, CSS
transforms, transparency, or HTML controls above the live image. Render scene gizmos through the renderer, with editor
commands providing their interaction semantics.

For the desired richer UI, prototype a composited path: render offscreen and present a shared GPU surface in a
composition tree with the WebView, or another supported host integration that permits overlays. Microsoft documents
distinct WebView2 windowed and visual hosting modes; visual hosting requires additional scaling and spatial-input
routing. This is evidence of a possible Windows architecture, not proof that Tauri exposes the required
integration. [WebView2 hosting modes](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/windowed-vs-visual-hosting).

Do not transport full-resolution frames through JSON, base64, or ordinary event messages. GPU sharing, synchronization,
format/color-space compatibility, and adapter interoperability require measurement. A CPU-readback path may support
low-rate thumbnails, but must not become the default interactive viewport without a documented budget.

### Define coordinates, focus, and input ownership

Send CSS client-space logical bounds and a monotonically increasing geometry generation. The host converts using the
owning window's scale; apply scaling once. Track logical aperture, native physical extent, and render resolution
separately. Picks reference the view and geometry/frame generation so a resize cannot make a stale coordinate select the
wrong location. Define rounding and letterboxing explicitly.

Test monitor transitions and WebView zoom; CSS pixels and host logical units cannot be assumed equivalent under every
zoom configuration. Until supported, enforce a known application zoom policy. A zero-size, hidden, detached, or invalid
aperture must not allocate a zero-extent swapchain or continue presenting at old coordinates.

Input goes to the focused view or UI control according to ADR-0010. UI-owned editor shortcuts remain in the editor;
gameplay samples target the shared Rust action mapping used by editor Play and the standalone game. Existing native
events forwarded to Angular are not the target gameplay authority. Native pointer capture, relative motion, cursor
locking, and drag cancellation need explicit ownership. Text entry and IME stay with the intended editor control.
Opening a modal, losing focus, hiding the view, or ending Play releases capture and clears held actions. Restoring
visibility alone must not resurrect previously held movement keys.

### Keep GPU ownership and shutdown explicit

Use a lifecycle such as detached, attaching, active, suspended, failed, and stopping. Coalesce replaceable geometry
requests; preserve ordering for attach/detach and ownership changes. A worker owns its surface resources, and completion
is required before destroying their native parent.

Resize invalidation, surface loss, and device loss are distinct outcomes. Vulkan requires an out-of-date swapchain to be
recreated before successful presentation; a suboptimal swapchain may still
present. [Vulkan swapchain contract](https://docs.vulkan.org/refpages/latest/refpages/source/VK_KHR_swapchain.html).
Retire in-flight resources only after the relevant GPU/presentation completion conditions; do not treat a successful
bounds command as proof that old resources are already unused.

Multiple views may eventually share a device and immutable asset caches while owning separate cameras, targets, and
surface lifetimes. Decide worker/device sharing after measurement. Detaching one view must not destroy resources still
used by another; moving across top-level windows initially recreates a surface rather than reparenting an active GPU
child.

## Alternatives and consequences

| Alternative                         | Tradeoff                                                                                         |
|-------------------------------------|--------------------------------------------------------------------------------------------------|
| Current child window                | Direct native presentation and least integration change; constrained overlays and composition.   |
| Offscreen/shared GPU composition    | Potential for richer overlays; platform interop, synchronization, and input work must be proven. |
| Separate native viewport window     | Clear ownership and useful fallback; weaker docking experience.                                  |
| CPU image transfer into the WebView | Straightforward prototype; copying, latency, and bandwidth can dominate interactive rendering.   |

An explicit initial limitation is preferable to inconsistent menus. Platform adapters can share the logical viewport
contract without claiming identical native behavior.

## Open decisions and implementation gates

First document and test Windows single-view behavior: menus/modals, pane resize/scroll, minimize/restore, mixed DPI,
focus, pointer capture, and shutdown during rendering. Then prototype composition with measured presentation latency,
frame-time percentiles, memory, resize cost, and input correctness. Confirm actual Tauri/WebView2 integration support
before selecting a path.

Only then enable multiple views and test closing one while another renders, separate camera bindings, failed surface
recreation, and stale completion rejection. Cross-window docking requires its own acceptance cases. macOS and Linux need
independent native spikes; their support is not established by this Windows implementation or ADR.

Acceptance requires native GPU testing on declared hardware/platforms, including validation-layer checks and recorded
limits. Static documentation checks and browser design previews cannot verify this proposal's native rendering claims.
