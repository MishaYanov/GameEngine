# Engine pipeline and current scope

Reviewed against HEAD `a48ff6c` and the current working tree for steps21–35. The intended editor is the Angular app with
an editor area and embedded game pane. Runtime owns domain logic, bridge owns contracts, and Tauri owns project/native
lifecycle. **No editor UI layout, mock, or template composition was added in this sequence.**

Read the separate [steps21–35 learning report](editor-steps-21-35.md) for responsibility, rationale, file map, examples,
resources and final evidence. The earlier [steps11–20 report](editor-steps-11-20.md) describes a historical native
prototype, not the current product ownership decision.

## Implemented contracts and foundations

| Step | Current scope                                                                                              | Explicit limit                                                                      |
|------|------------------------------------------------------------------------------------------------------------|-------------------------------------------------------------------------------------|
| 21   | Typed Angular EditorService/models/directive, runtime EditorSession, Tauri workspace/native child viewport | Integration APIs; actual editor layout/game-pane composition remains future UI work |
| 22   | Validated bounded JSON-aware compiler/admin asset packaging                                                | Copy/compile success is not installed-run acceptance                                |
| 23   | Frame/orbit/pan/dolly command intent                                                                       | No default toolbar/key binding added                                                |
| 24   | Validated translation/rotation/scale snapping                                                              | UI owns settings presentation and intent                                            |
| 25   | Multi-key selection, grouped transactions and bounded patch history                                        | 128 records/16MiB CPU history; legacy GPU history remains separate                  |
| 26   | Parent keys, indexed world matrices, keep-world reparent validation                                        | TRS-only; shear/singular conversions can fail                                       |
| 27   | Schema3 with compatible v1/v2 migration                                                                    | Limits are acceptance ceilings, not measured scene capacity                         |
| 28   | Bounded prefab export/instantiate/save/load with remapped keys                                             | No live-linked prefab override system                                               |
| 29   | Selected static glTF scene/node/mesh traversal                                                             | No skin/morph/animation playback or all material extensions                         |
| 30   | Per-entity tint overrides                                                                                  | Not a material graph or PBR inspector                                               |
| 31   | Directional/point scene lights                                                                             | No shadows or measured lighting appearance acceptance                               |
| 32   | Default60Hz fixed steps with bounded catch-up                                                              | Not strict networking determinism or full game scheduling                           |
| 33   | Collider overlap/debug and conservative swept movement                                                     | Not a rigid-body/triangle-accurate physics solver                                   |
| 34   | Actual asynchronous GPU queries plus submitted work metrics                                                | Not GPU utilization, display latency or current-frame wall time                     |
| 35   | Compatible opaque Vulkan instancing and resource reuse                                                     | No measured speedup or all-transparent/GPU-driven rendering claim                   |

## Editor ownership and integration gates

Angular owns layout, accessibility presentation, tool widgets, dialogs and physical input mappings. Core editor service
signals and capabilities supply authoritative state; the runtime command factory determines supported semantics. No
second native authoring UI should be developed as the product path. Normal game mode is overlay-free; its native editor
demo is explicitly legacy opt-in.

The embedded viewport is currently a Windows native child. Airspace, clipping, DPI, focus, bounds/visibility and
teardown are real integration constraints. An available directive does not mean the app template already uses it.
Composing the actual Angular editor with game pane is an explicit remaining UI task.

EditorSession owns authored/saved/Play documents and patches. Command revision, render scene revision, and workspace
generation are distinct; Play ticks should not invalidate every queued command. Runtime has no hardcoded widget/keyboard
policy. Idle native Edit waits for invalidation, and paused/no-input Play avoids unnecessary ticking.

## Broader engine work still open

Full PBR/shadows, animated/skinned/morph imports, advanced physics, audio, scripts, particles, authored game UI,
backend-independent handles, linked prefab updates, broad asset hot reload and GPU-driven rendering remain independent
future features. Their implementation is not implied by a typed command or a data schema field.

Packaging JSON inclusion is fixed. Installed release execution outside the checkout, clean-checkout reproducibility and
Jenkins execution evidence remain separate gates. Human-visible editor/game-pane acceptance and workload-specific GPU
performance measurements have not been selected for this batch.

## Verification

Final TESTER/LOGGER/SECURITY and native-example evidence for this implementation is recorded in [testing](testing.md),
the step report and docs-site/docs-state.json. Historical smoke/test totals belong to their earlier batches. Automated
checks, hidden native execution and visible acceptance must not be collapsed into one completion claim. Steps beyond35
are planning only, not extra implementation in this sequence.
