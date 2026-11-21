# Milestones 7–9 review findings

This record accompanies the [implementation review](../milestones-7-9-review.md) and
[verification ledger](milestones-7-9-tests-2026-11-21.json). Reviews inspected the implementation relative to
`fc95059`, including new files. The ledger identifies the final inspected commit and executable checks.

## Findings and fixes

| Finding | Resulting change | Verification |
| --- | --- | --- |
| Malformed GLB accessor strides, offsets, counts and semantic types could reach unchecked reader arithmetic | Validate nonzero counts, checked byte ranges, alignment, padded element sizes and exact encodings before constructing readers | Adversarial decode tests use `catch_unwind`; a panic fails separately from a controlled error |
| Finite normal components could overflow during normalization | Reject unusable normal lengths with checked normalization | Huge finite normal regression |
| Material factors outside the supported range passed dependency validation | Require finite base-color factors in 0–1 | Negative, above-one and extreme-factor regressions |
| A finite analytical world bound could hide overflow in the shader's mesh-local multiplication | Bound source positions and inverse-bind translations and validate accepted pose matrices | Regression follows the shader's actual multiplication order |
| Sensor routing scanned every trigger for every input | Index directed Enter pairs once, retaining stable machine/input traversal | Duplicate Enter and stable emission-order regression |
| Each material primitive copied an already-shared RGBA image before checking the texture cache | Prepare shared images once per cache miss, then reuse texture owners | Native material/import checks and source inspection |
| Compiler animation validation rejected a model's reserved former path while runtime accepted it | Resolve the path through the catalog and compare stable asset IDs and selected mesh | Moved-model runtime/staged-package parity regression |
| An Angular effect observed preview state while closing it, so opening could trigger immediate cleanup | Make context-change cleanup untracked | Preview remains open after completion; revision/generation replacement and destruction close it |
| Disabled animation rest-pose bounds did not follow owner movement | Refresh placement and bounds without advancing its animation clock | Disabled-animation movement regression |
| Rebuilding native topology could reopen a changed animation source despite a live immutable pin | Reuse an existing source-matching immutable pin; a new world still validates disk | Pinned-source versus fresh-load checks |
| A spawned native scene could expose existing animations at their initial pose for one frame | Synchronize the accepted animation bank before publishing the replacement scene | Native spawn/retirement acceptance |
| Static-only GPU import preparation was used for a newly imported animated GLB | Prepare skinned resources through the shared decoder and retain discard/promote ownership | Native import preparation checks |
| Edit-mode placement could bypass the animation shader's uniform-scale assumptions | Validate initial and incremental world placement before mutation, plus a fallible draw guard | Rejected nonuniform edits preserve the accepted scene; reflected direct placement is rejected |

The initial aggregate also caught a missing sibling-module declaration in the host's CPU integration-test harness
and an obsolete headless help-text assertion. Both were corrected. The collider accessibility audit was split into
one test per shape so each independent audit has its own normal timeout; no checks were removed or timeout raised.

Implementation commit `6b217e3` and cleanup `17d1b0e` were inspected in the final security pass, including the Edit
placement guards. No further confirmed security issue was found. Final cleanup uses the exact angle constant in
native tests, simplifies equivalent match guards, and places test modules after production code. It does not expand
the supported feature subset. Clippy succeeds; remaining warnings originate in unchanged bridge DTO, renderer-frame
signature and viewport-tool code, plus the local incremental-cache hard-link limitation.

## Boundaries checked

Events carry generation-bearing endpoints and become eligible on the next fixed step. Pending, per-step and
aggregate delivery budgets remain distinct. Invalid authored routes fail before admission, while retired runtime
endpoints are discarded according to the documented policy. Machine actions, controller signals and animation
poses stay inside the aggregate transaction. Rejection restores the prior world and component state, then the
existing failure policy pauses the session and clears held input; resuming requires fresh input.

Preview commands require the trusted editor window, the current authoring context and a matching opaque token.
Sampling holds one immutable source and changes no authored document or simulation clock. Old tokens cannot
close replacement previews. Asset loading remains contained, bounded and catalog-based; no URI fetching was added.

Palette storage belongs to a frame and is written only after its completion wait. Existing retirement waits remain
in place. Palette indices, matrix counts, immutable influences, material flags and resource owners are checked by
CPU and hidden native tests. The initial supported animation and controller subsets remain explicit.

## Optimization evidence and limits

Before indexing, legal limits allowed 4,096 machines × 8 inputs × 4,096 unmatched trigger edges, or 134,217,728
comparisons per fixed step before any emission. Indexed routing uses expected linear work in trigger edges plus
input routes while preserving emission order. This is source-derived complexity, not a latency measurement.

For 128 primitives sharing one 4 MiB image, the former load path copied 512 MiB. The shared-image path copies
4 MiB on a cold texture-cache miss and no image data on a cache hit. These are illustrative byte counts within
the supported limits, not measured frame-time improvements.

Animation-bank staging still copies mutable pose arrays. At 64 instances with 256 local nodes, 256 global matrices
and 128 palette matrices, the principal arrays total approximately 2.125 MiB per clone before fades. This bounded
cost is retained to preserve rollback. An immutable pose snapshot optimization should follow a representative
measurement and preserve the same transaction/resource tests.

Focused suites overlap the workspace total. Hidden Vulkan acceptance uses actual GPU resources and exported data;
it does not claim a visible desktop walkthrough or another-machine result. Raw replay hashes exclude opaque game
module internals and scheduler state as documented by the CLI. They are diagnostic comparison hashes, not security
hashes or a save-game format. No exhaustive fuzzing or adversarial same-user filesystem-race proof is claimed.

## Future guide review

Independent review of the local M10-M12 guides corrected stale camera input across ordinary scene transitions,
rejected-tick pause/input expectations, prefab entity-local component identity, and disabled interaction-reference
validation. The save guides also distinguish preserved run state from the input clearing required when entering
loading, and keep locomotion on the same entity as its controller. These reviews improve proposed learning material;
they do not constitute compilation or acceptance of future features.
