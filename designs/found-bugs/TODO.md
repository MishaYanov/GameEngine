# Deferred optimization TODO

Recorded: 2026-10-27. These are open performance opportunities, not confirmed correctness defects. The five selected
dashboard, layout and editor synchronization improvements are handled separately.

Line estimates describe the likely changed implementation and test scope, not guaranteed deletions. Margins express
estimation uncertainty; regression risk is listed separately. Validate benefit with representative measurements before
expanding each change.

## Implementation progress inspected on 2026-11-20

The original estimates below are historical planning ranges. The current source has narrowed several items:

- Bounded synchronous mesh/texture upload batches are implemented. AssetManager also caches bounded glTF resource
  snapshots, lazily prepares selected-mesh images and reconciles cache retention/accounting. Asynchronous streaming
  and measured before/after loading results remain open.
- Instance storage is reusable and survives compatible resize through an explicit completed-frame transfer. Opaque
  order/batches are still rebuilt and instance data is still written per draw; changed-range caching remains open.
- Acquisition/submission completion and per-swapchain-image presentation semaphores are explicit, with failed-target
  retirement before reuse. There is still one in-flight frame; the two-frame proposal remains unimplemented.
- Retained document cost caches and bounded patch history exist, but some authoring commands still clone scene data.
  The broader allocation/latency profiling and persistent-storage proposal remains open.

These are source capabilities and regression-test targets, not measured performance gains. Current evidence and
remaining native limits are in the [building-blocks review](../../docs/building-blocks-review.md#optimization-review).

| Follow-up                                               | Affected project / crate                                             | Estimated changed lines | Estimate margin | Complexity | Regression risk                                      |
|---------------------------------------------------------|----------------------------------------------------------------------|------------------------:|----------------:|------------|------------------------------------------------------|
| Cache render batches and update changed instance ranges | `renderer`, `runtime`; integration in `game` and `app` (editor host) |                 250–550 |            ±40% | High       | High: stale instances, ordering and invalidation     |
| Batch GPU uploads and fence waits                       | `renderer`, `runtime`; integration in `game` and `app`               |                 350–750 |            ±45% | High       | High: resource lifetime and synchronization          |
| Support two frames in flight                            | `renderer`; integration in `game` and `app`                          |                 400–850 |            ±45% | High       | High: shared frame resources and swapchain lifecycle |
| Avoid full-scene clones for common edits                | `engine-core`, `runtime`; integration in `game` and `app`            |               500–1,200 |            ±50% | Very high  | High: undo, transactions and authored/play isolation |

## Cache render batches and changed instance ranges

- [ ] Profile batch reconstruction, sorting and instance-buffer uploads for static scenes and isolated entity edits.
- [ ] Cache stable render grouping and track changed instance ranges.
- [ ] Invalidate on insertion, removal, asset/material changes, visibility, ordering and scene replacement.
- [ ] Compare rendered output and submitted instance data against full rebuilds; measure CPU time and uploaded bytes.

## Batch GPU uploads and fence waits

- [ ] Measure synchronous upload submissions and waits during scene load and editing.
- [ ] Batch compatible staging copies and make completion ownership explicit.
- [ ] Keep staging buffers and destinations alive until GPU completion; preserve barriers and queue ownership.
- [ ] Exercise failures, shutdown and asset replacement with validation enabled; measure submissions and wait time.

## Two frames in flight

- [ ] Inventory per-frame and shared command buffers, semaphores, fences, uniform buffers, descriptors and depth
  attachments.
- [ ] Introduce independent frame resources and track swapchain-image ownership.
- [ ] Handle resize, minimized windows, swapchain replacement and shutdown without reusing resources still in flight.
- [ ] Validate with both game and editor viewports; measure latency and throughput before changing defaults.

## Avoid full-scene clones for common edits

- [ ] Profile remaining document clones in command execution, undo/history, transactions and runtime synchronization.
- [ ] Choose narrowly scoped inverse operations, persistent storage or copy-on-write based on those measurements.
- [ ] Preserve atomic failure, undo/redo, transaction cancellation, scene switching and authored/play isolation.
- [ ] Keep broad replacement commands on a safe fallback path until equivalent behavior is verified.
- [ ] Compare allocation counts, copied bytes and latency for representative small edits on large scenes.

## Completion evidence

For each follow-up, record the tested scene sizes, before/after measurements, correctness checks and remaining limits.
The current editor transport improvement reduces returned data for compatible commands; it does not establish that
command execution or undo history is free of full-scene copies.
