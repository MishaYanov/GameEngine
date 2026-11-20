# Components, linked prefabs and runtime lifecycle

This review covers the eighteen features in Milestones 4–6. The inspection baseline is commit `c34555e`, plus
the working tree on 2026-11-20. The earlier [sixteen-feature review](milestones-review.md) remains the record
for Milestones 1–3. The repository checkpoint records final source fingerprints and aggregate verification;
individual results below identify their scope rather than implying visible desktop acceptance.
The [machine-readable verification record](verification/milestones-4-6-tests-2026-11-20.json) preserves the
reported commands, results and artifact fingerprints.

## Milestone 4: authored components

| Feature | Implemented boundary |
| --- | --- |
| 4.1 Component contracts | Stable object-local component IDs, component schema 1 and typed Mesh Renderer, Collider and Native Behavior records |
| 4.2 Migrate existing capabilities | Scene schema 4 stores canonical component arrays; legacy scenes migrate through existing flat internal adapters |
| 4.3 Component editing commands | Add, replace, remove and enable commands use the existing revision, transaction, history and save paths |
| 4.4 Component Inspector | Typed forms edit model/material, collider and native behavior settings; unavailable controls remain read-only |
| 4.5 Independent native behavior instances | Key and Door, Collectible and Timed Spawner attachments keep independent runtime state |
| 4.6 Prove reusable gameplay assemblies | The Component Lifecycle Lab contains two independent key/door pairs and reusable component prefab sources |

An object supports at most 16 components and 8 native behaviors. Mesh Renderer and Collider are singletons.
Component IDs use 1–128 ASCII letters, digits, underscores or hyphens. Add creates an ID; editing preserves it.
The Inspector reads computed `component_views`, while commands mutate typed backing data. The frontend does
not make that read-only projection a second source of truth. Changed entity deltas carry their component views.

Select an object and use **Add Component**, **Edit**, **Enable/Disable** or **Remove**. Edit the typed draft and
select **Apply Component** to accept one undoable change. **Discard Component Draft** leaves the authored
component unchanged. Pending Inspector edits finish before a discrete component command captures its revision.
A changed component or replacement workspace rejects an old full-component draft rather than overwriting it.

Key and Door requires distinct Actor, Key sensor and Door references; Key visual is optional. Collectible needs
distinct Actor and Trigger sensor references and can hide a visual or despawn its owner during Play. Runtime
preparation checks sensor/collider compatibility. Timed Spawner uses a relative JSON prefab path, interval and
lifetime of 1–1,000,000 ticks, maximum alive of 1–128 and finite offsets within ±1,000,000. Its up to 32 explicit
reference bindings connect source slot names to entities in the current scene. It has no implicit global Player.

Material color channels use finite values from 0 to 1. Collider controls reuse the existing shape, sensor and
layer/mask editor. Numeric drafts reject incomplete or unrepresentable values before publication.

## Milestone 5: linked prefab authoring

| Feature | Implemented boundary |
| --- | --- |
| 5.1 Prefab source identity | A source retains its persistent asset ID, path and captured fingerprint; instances retain an explicit source version |
| 5.2 Prefab source editing | Workspace-owned drafts expose typed source objects, hierarchy, components and external slots, with validate/save/close guards |
| 5.3 Linked instance resolution | Instantiation remaps source identities and declared external bindings into a synthetic scene anchor and members |
| 5.4 Typed property overrides | Name, transform vectors and typed component records have explicit override addresses and values |
| 5.5 Update, conflict handling, and unpack | Explicit update, reject/keep-local/use-source conflict policies, property/all revert and unpack are undoable |
| 5.6 Export resolved prefab content | Compiler preparation resolves the captured linked content and validates its source/dependency closure |

Copied prefab placement remains separate from **Link Prefab…**. Legacy gameplay prefabs can still be copied.
Linked sources use canonical prefab schema 3 with component entities and explicit external reference slots;
they do not silently convert project behaviors or legacy gameplay configuration. Ordinary parent hierarchy and
multiple roots are supported. Nested linked sources and variants remain outside this implementation.

### Create, edit and link an assembly

1. Select the component objects that form the assembly. In Assets, select **Create Linked Prefab Source…**.
   Choose a new JSON path inside an existing Assets folder. Registered scene destinations, including unsaved
   scene drafts, remain reserved.
2. Declare references outside the selection. For the lab key/door assembly, declare `actor = lab-player`.
   This is an explicit source placeholder; it does not copy the selected scene actor into each instance.
3. The **Prefab Source** editor edits its own draft. Source keys and existing component IDs are read-only.
   Add/remove objects, edit local transforms and parent relationships, or use the shared Component Inspector.
   Apply or discard the current object/component subdraft before validating or saving the source.
4. **Unsaved source changes** includes applied object edits, components and slot changes. Close, Escape and
   backdrop close cannot silently discard dirty or pending work. Save, or use **Discard Source Changes and Close**.
5. **Validate Prefab Source** checks the reviewed draft. **Save Prefab Source** checks its captured backend draft,
   authoring revision, source identity and disk/catalog fingerprint before publication. A stale source remains
   editable locally with an error; it is not overwritten automatically. Save updates the source baseline only
   after native acceptance.
6. Select the source in Assets and choose **Link Prefab…**. Bind every declared slot to a current scene entity,
   then instantiate. Placement begins at the origin; moving the anchor positions the assembly.
7. Make another instance with its own Actor binding. Edit a member's name, transform or component in the normal
   Inspector. Its linked section shows typed Base and Local values for the resulting overrides.

Sources support up to 128 unique external slots. Slot names use 1–64 ASCII letters, digits, underscores or
hyphens; placeholder keys are distinct from owned source entities. Timed Spawner's binding limit is separately
32. For an independent copy of a component source with external slots, Link with its bindings and then Unpack.

### Review source changes and overrides

Selecting any member resolves its linked anchor in native code. **Edit Linked Source…** opens the current source,
including a moved source resolved by asset ID. Saving it leaves existing instances on their current version.
Linked status refreshes on authoring changes and asset metadata refresh, rather than reading files every Game tick.

Use **Review Source Update…** to load the latest source, review reference bindings and choose a conflict policy.
**Reject overlapping changes** reports conflicts without changing the instance. **Keep local values** or
**Use source values** makes the overlap policy explicit. Non-overlapping changes merge. New source slots need
explicit bindings. Source member removal can be blocked by local overrides or external scene references.

Override granularity is explicit: name, each transform vector, or a typed component record. **Revert Property**
restores that group, and **Revert All Overrides** restores the supported groups together. **Unpack Linked Prefab**
keeps the resolved objects and removes the link. Scene Undo/Redo handles instance edits, update, revert and unpack.
A source-file Save is a separate publication; Scene Undo does not rewrite a saved source.

Sources, instances, materialized dependencies and package publication retain their existing bounds. File writes
are failure-atomic transactions, not a crash-atomic multi-file database. A missing source is reported without
silently retargeting an instance to a different asset. Runtime Play changes never apply back to source documents.

## Milestone 6: runtime entities and lifecycle

| Feature | Implemented boundary |
| --- | --- |
| 6.1 Runtime handles and lifecycle rules | World/key/generation handles reject stale ownership and expose authored/spawned origin |
| 6.2 Prepared prefab spawning | Hosts prepare prefab requests and admit bounded instances at fixed-step boundaries |
| 6.3 Despawn and cleanup | Removal retires instance entities, references, collision records and native resource owners |
| 6.4 Simulation timers | Fixed-step deadlines support bounded lifetime and cleanup without wall-clock callbacks |
| 6.5 Runtime inspection | Game's read-only Runtime Inspector uses explicit Refresh and bounded entity pages |
| 6.6 Dynamic gameplay and stress fixture | Repeated collectible spawn/contact/despawn cycles exercise state, identity, timers and resource retirement |

The host stops catch-up at a prefab request boundary, prepares the requested data/resources, admits the result,
then drains retained simulation steps with `tick(0)`. Player input edges are not replayed during that drain.
Admission and timer behavior therefore follow simulation boundaries rather than the number of render calls.
Restart, Stop and runtime scene replacement clear the relevant runtime ownership and timer state.

While Play and Game are selected, **Runtime Inspector** replaces the authoring Inspector. **Refresh Runtime**
captures a page of at most 32 entities; native queries allow at most 128. Previous/Next changes pages explicitly.
The panel reports runtime scene, tick, topology revision, timer count, pending spawns, handles, instance origin
and per-component activation/elapsed ticks. It does not send authoring commands or copy an authored snapshot
every frame. Stop, restart, scene transition and project replacement discard stale query replies.
Selecting Scene during Play still exposes disabled authoring fields; Stop restores normal authoring.

## Run the Component Lifecycle Lab

Open `apps/game/component-demo.json` with the editor project chooser and select Play. Its two key/door pairs are
independent. WASD moves the configured actor at 2 units/second. The timed collectible source declares an Actor
slot bound to `lab-player`; inspect the spawner component and its explicit binding. Refresh Runtime Inspector
while objects appear and retire. Restart and Stop restore authored state.

From the repository root, run the same manifest headlessly:

```powershell
cargo run -p engine-headless -- --project apps/game/component-demo.json --steps 1245
```

To build a standalone release, choose an existing destination parent that does not already contain the project:

```powershell
cargo run -p compiler -- --export apps/game/component-demo.json 'D:\Projects'
```

The installer creates `D:\Projects\Component Lifecycle Lab`. Keep its `game.exe`, `project.json` and `assets`
together. Player does not support `--project`; use editor Open Project, headless `--project`, or the completed
export/verified launch flow to select authored content. **F10** restarts a Player run.

## Source boundaries and verification

| Boundary | Relevant source |
| --- | --- |
| Component schema, migration and commands | `engine-core/components.rs`, `scene_data.rs`, scene codec and typed editor commands |
| Inspector forms and ownership | Angular workspace `components/`, `SceneEditingService` and `editor-updates.ts` |
| Source drafts and publication | Host `prefab_sources.rs`, runtime asset transactions and canonical prefab codec |
| Linked instance UI | Angular workspace `prefabs/`, typed draft/status DTOs and existing editor command queue |
| Runtime lifecycle | Core simulation/world ownership and the shared host admission boundary |
| Read-only runtime UI | `runtime-inspection.ts` and workspace `runtime-inspector/` |

The Angular suite passes **455 tests across 49 files**, including structural accessibility checks. Production
bundle validation passes with the existing project-setup stylesheet budget warning. Logs:
`target/milestones46-angular-tests.log` and `target/milestones46-angular-production.log`.
The documentation build link-checks 61 pages and passes 8 site tests, including publication of its referenced
verification JSON without changing its bytes. Log: `target/milestone46-docs-site.log`.
Focused tests cover component drafts and history intent, source isolation and dirty closure, explicit external
references, conflict retries, authoritative save adoption, stale contexts and bounded runtime inspection.

Root lifecycle checks passed 10 tests and fixed-step admission-boundary checks passed 13 in a separate file.
Both overlap the workspace aggregate and must not be added to it again. See `target/milestone46-lifecycle-tests.log`
and `target/milestone46-spawn-barrier-tests.log`. Repeated 1,245-tick headless replays produced identical state
hashes with 10 collectible cycles, topology revision 20, 6 retained entities and no timers/pending spawns/errors.
The reports are `target/milestone46-replay-one.json` and `target/milestone46-replay-two.json`.

The hidden Vulkan lifecycle test exercises 20 seconds/1,200 ticks, repeated admitted collectibles, collection,
resource counts returning to baseline, runtime transition and restart. Logs are
`target/milestone46-native-lifecycle.log` and `target/milestone46-native-export-lifecycle.log`.
These use actual native resources; they are not visible desktop authoring sessions or OS-delivered manual input.
The workspace run reported 849 passed, 1 failed and 9 ignored across 75 test binaries. Its single failure was
an outdated prefab-v2 assertion, corrected to v3; the Courtyard target then passed all 6 tests on a corrective
rerun. This evidence is the aggregate plus its successful corrective rerun. A clean post-commit run remains
part of the independent test review. Logs: `target/milestone46-workspace-tests.log` and
`target/milestone46-courtyard-regression.log`.

The completed export is
`C:/Users/misha/Documents/Codex/Milestones 4-6 Review 2026-11-20/Component Lifecycle Lab`.
Its headless route finished at tick 1,245 with state hash `9e7bf1d490b2adb5`, topology revision 20, 6 entities,
no timers and no pending requests. The hidden exported-data lifecycle run passed separately. It is a native
GameApp harness loading exported content, without visible UI or manual OS input to the exported executable.
The native editor embedding build passed in `target/milestone46-native-final-build.log`.
Final source hashes and export evidence belong to the root verification checkpoint.
Game App Developer remains empty.
