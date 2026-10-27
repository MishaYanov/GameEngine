# Deferred optimization TODO

Recorded: 2026-10-27. These are open performance opportunities, not confirmed correctness defects. The five selected
dashboard, layout and editor synchronization improvements are handled separately.

Line estimates describe the likely changed implementation and test scope, not guaranteed deletions. Margins express
estimation uncertainty; regression risk is listed separately. Validate benefit with representative measurements before
expanding each change.

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
