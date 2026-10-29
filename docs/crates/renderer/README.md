# Vulkan renderer

Source: `crates/renderer`. WindowRenderer owns presentation resources and on-demand native viewport rendering. The
status-only Renderer compatibility wrapper remains separate; the host now probes/caches status without retaining an
eager permanent second headless device.

## Ownership and presentation

WindowRenderer creates a window surface, suitable device, swapchain/depth/frame/pipeline resources and shared layouts.
prepare defers zero-size rendering and rebuilds dirty targets after idle; resize marks state dirty. Uploaded resources
must still be released before their owning device. Native viewport shutdown and scene replacement respect that lifetime
order; Arc handles are not backend-independent resource ownership.

The lower-level `VulkanFrame::draw` accepts borrowed color/depth resources together as `FrameTargets`;
WindowRenderer supplies that pair from its current presentation targets.

`draw` returns `DrawStatus::{Rendered, Suspended, NeedsRedraw}`. Zero-size targets suspend rendering; out-of-date or
suboptimal presentation requests another redraw/rebuild. Resize recreates swapchain/depth/frame targets after idle,
retiring old targets before a failed replacement can be reused. Material pipelines survive extent-only changes while
their color/depth formats and renderer-owned descriptor layouts remain compatible.

FIFO remains the presentation policy. Native Edit waits for invalidation rather than continuously submitting identical
frames. Fixed-step simulation scheduling is separate from presentation pacing.

## Current shader data

| Data               | Layout                                                                                 |
|--------------------|----------------------------------------------------------------------------------------|
| Vertex             | 32 bytes: position offset0/location0, UV offset12/location1, normal offset20/location2 |
| InstanceData       | 80-byte model matrix+tint, binding1, locations3–7, per-instance input rate             |
| ModelPushConstants | 96 bytes: model0, factor64, flags80                                                    |
| Camera             | Descriptor set0/binding0                                                               |
| Lighting           | Descriptor set0/binding1, up to16 validated directional/point lights                   |
| Texture            | Descriptor set1/binding0                                                               |

Flags retain unlit/font/vertex-color/depth-free behavior and add the frame-batcher instanced-input bit. Per-instance
tint combines entity/material values; normals use inverse transpose with finite fallback for collapsed transforms. Basic
directional/point lighting is not PBR, shadows, or every glTF material feature.

## Batching and resource reuse

Compatible opaque runs are grouped for indexed instanced draws. Mesh/material/pipeline compatibility matters;
overlay/unlit/alpha objects form order barriers and cannot be sorted through arbitrarily. A reusable host-visible
instance buffer grows only as needed up to a limit; capacity growth is still an allocation. Dynamic legacy UI/gizmo
buffers synchronize writes with frame completion, and the Angular product path renders no forked native editor overlay
UI.

`WindowRenderer::upload_batch` accepts `MeshUpload` and `Rgba8Upload` inputs, preserving order in `UploadedBatch`.
One synchronous transfer submission handles at most 256 resources and 64 MiB of payload; returned resources are ready
for drawing. Staging resources, command pool and completion fence stay owned until GPU completion. If fence and queue
waits both fail without device loss, the device retains the submission and blocks another upload until it can drain
outstanding work. Failure/rollback owners release partially created resources. This is bounded synchronous batching,
not asynchronous streaming or a measured loading-speed claim.

## Actual GPU measurements

DrawMetrics reports submitted draw calls, instances, triangles, instanced groups, timestamp support and optional
gpu_time_ms. Two timestamp queries measure an older completed GPU command range. Device valid bits/period convert ticks;
availability checks avoid adding a query WAIT on top of the existing frame fence.

GPU time excludes CPU update, acquisition, presentation and display pacing. None/unsupported is not zero time.
Implementation of timing/batching is not evidence of measured speedup, GPU utilization or a tested frame budget. The
typed viewport status exposes these values to the Angular owner.

## Resource accounting

Instrumented logical devices, buffers, images, views, samplers, descriptors, pipelines, command objects, synchronization
objects and swapchain objects report through `engine_logger::metrics`. Guards follow their owning resources, including
upload staging resources and rebuild/release paths. AssetManager adds asset labels to uploaded meshes and textures.
`WindowRenderer::resource_metrics()` returns an optional current snapshot for its device.

Tracking defaults on in debug builds or with `ENGINE_LOG_MODE=debug`. Set `ENGINE_METRICS=1` to enable it in any build,
or `ENGINE_METRICS=0` to disable it; invalid values warn and disable tracking. This selection is independent of the log
severity filter. Initialize the application logger with persistent storage to expose snapshots to the
[admin resource monitor](../../apps/admin-dashboard/README.md#vulkan-resource-monitor).

Allocation bytes are the sizes requested from Vulkan, including alignment; payload bytes describe logical content.
Device-local and host-visible categories can overlap. Swapchain images have no explicit engine allocation size, so
their backing memory, driver overhead and CPU RAM are excluded. Object counts cover instrumented objects only.
Frame counters and GPU timing accompany the inventory; a missing GPU timing sample stays unavailable. Closed devices
retain final snapshots, while an unexpectedly stopped process may leave a stale last snapshot.

## Checks and learning

```powershell
cargo test -p renderer --all-targets --locked --offline
cargo check --workspace --all-targets --locked --offline
```

Shader/pipeline/light/instance/timestamp contract and native example results are recorded
in [testing](../../testing.md). No interactive child-window or visual material acceptance is claimed.
See [steps 31,34,35](../../editor-steps-21-35.md#step-31-directional-and-point-lights)
and [graphics/assets](../../graphics-and-assets.md).
