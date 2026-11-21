# Gameplay events, character movement and animation

This review records the eighteen implemented features in Milestones 7–9, committed as `6b217e3` after baseline
`fc95059`, plus the final review cleanup on 2026-11-21. History, the index, modified source, new fixtures and tests
were inspected together. The four unrelated `*~` backups are outside this work.
[Milestones 4–6](milestones-4-6-review.md) remains the earlier component/prefab/lifecycle record.

The [verification ledger](verification/milestones-7-9-tests-2026-11-21.json) records commands, source fingerprints
and acceptance stages. The [review findings](verification/milestones-7-9-review-findings.md) explain corrected
issues and remaining limits. Native acceptance used hidden Vulkan windows; no visible desktop walkthrough is claimed.

## Milestone 7: typed events and state machines

| Feature | Implemented behavior |
| --- | --- |
| 7.1 Addresses and bounded queues | Saved `{entity, component}` references resolve to runtime world/key/generation endpoints; pending envelopes are ordered by due step and sequence |
| 7.2 Transactional delivery | An emission at step N is eligible at N+1; the inbox is frozen once, and the entire multi-step tick call rolls back on failure |
| 7.3 Machine definitions | Schema-1 native tables validate every state and reference, then compile destinations to indices once per admitted attachment |
| 7.4 Machine execution | Each enabled attachment owns its state, entry step and initialization flag; entry effects and fixed-step deadlines use the existing action/lifecycle transaction |
| 7.5 Authoring and prefab references | The Component Inspector edits structured states, inputs, transitions and entry actions through one Apply; typed references remap with copied/linked prefab members and external slots |
| 7.6 Playable assembly | Event Interaction Lab contains two independent switch/door/pickup assemblies and a reusable `event-door.json` source with an explicit actor slot |

Implementation is in `crates/engine-core/src/gameplay_events.rs`, `state_machine.rs`, `state_machine_runtime.rs`,
`runtime_lifecycle.rs` and `simulation_session.rs`. The existing `NativeBehaviorSpec` adds
`state_machine { definition }`; scene schema 4, prefab schema 3 and component schema 1 remain the storage formats.
Runtime entries, queues and timers never enter saved component definitions.

### Definitions and limits

`MachineDef` contains `schema_version`, `initial`, `inputs` and `states`. Each state has `id`, `on_enter` and
`transitions`; each transition has `when` and `next`. Triggers are `signal { signal, source }`, where null source
accepts any live sender, or `after_ticks { ticks }`. Entry actions are `emit`, `set_visible`,
`set_collision_enabled` and `despawn_owner`.

The signal vocabulary is `activated`, `released`, `collected`, `reset`, `jumped` and `landed`. The last two are the
M8 adapter described below. Sensor input rows route accepted Enter edges; they do not repeatedly fire for a held
overlap or implicitly translate Exit into Released.

Editor capabilities publish the native state-machine descriptor, built from the same validation constants.
The typed frontend rejects an incompatible definition version, vocabulary or bounds before authoring against it.

| Limit | Value |
| --- | --- |
| Retained envelopes | 1,024 |
| Emissions / deliveries per fixed step | 512 each |
| Deliveries per aggregate tick call | 4,096 |
| States / sensor inputs | 1–32 / 0–8 |
| Transitions in one definition | 128 |
| Entry actions per state | 8 |
| Transitions per machine per step | 32 |
| Timer | One per state, 1–1,000,000 ticks |
| Serialized definition | 64 KiB, counted without creating another JSON string |
| State/input IDs | 1–64 ASCII letters, digits, underscores or hyphens |

These are admission/work limits, not measured frame-time claims. The earlier limits of 8 native attachments per
object and 4,096 active native attachments per scene still apply. A wildcard and restricted-source transition for
the same signal are ambiguous and rejected; two distinct restricted sources are valid. Destinations and the initial
state must exist. Disabled machines still validate all definitions, routes and prospective action capabilities.
Visibility targets need a mesh component even if hidden; collision targets need a collider even if disabled.

### Deterministic step order and lifecycle

1. Take due messages once and reject an oversized inbox before consuming it. Drop stale/disabled endpoints with a
   bounded diagnostic count; malformed newly authored routes fail validation.
2. Index directed Enter `(sensor, actor)` pairs once. Traverse machines by entity/component and their inputs by ID,
   enqueueing self-addressed sensor signals for the next step. Duplicate input edges do not duplicate a route's send.
3. Initialize a machine on its first actual fixed step. Fold its frozen inbox in sequence order, then evaluate one
   current-state timer. Self-transitions re-enter and reset the entry step.
4. Stage entry effects, resolve controller movement, apply actions and structural retirement, and append accepted
   controller edge signals. All producers share the same per-step emission limit.
5. Publish lifecycle, world, motion, animation, triggers and progress only if the aggregate tick succeeds.

Pending messages and uninitialized/timed machines keep simulation scheduled. Pure signal-only idle machines do not.
Pause preserves accepted queue/state and advances neither clock. Restart or a fresh scene creates a new runtime
world. Removing an owner cancels messages from or to it and removes its machine state. `DespawnOwner` may retire a
spawned instance; invoking it on an authored owner rejects the transaction.

### Author and play the event lab

Open `apps/game/event-demo.json` in the editor. The actor initially overlaps the left switch. Move along negative Z
to touch the right switch, pass the door and reach the pickup. The completion marker becomes visible. Move laterally
to the second assembly at X=6 to exercise its independent state. For the authored movement profile, W moves along
negative Z; the controlled movement speed is 2 units/second.

Each switch sends Activated to itself one step after contact, then to its door one step after latching. The door
uses five states: `closed`, `left_only`, `right_only`, `open`, `resetting`. Initial closed entry enables collision
without resetting switches. Open hides/disables the door for 180 fixed steps. Resetting restores it and queues both
switch resets; one step later the door returns to closed. The actor must leave and re-enter a reset sensor.
The pickup hides/disables once and queues completion. These state changes remain runtime-only.

In the Component Inspector choose Native Behavior / State Machine, edit its initial state, sensor inputs, state
rows, transitions and typed entry actions, then **Apply Component**. Entity/attachment selectors are explicit;
state names are not entity references. Prefab remapping changes only typed entity fields. External actor bindings
are validated after slot resolution. Linked source versions continue to use the reviewed source/draft guards from M5.

## Milestone 8: upright character controllers

| Feature | Implemented behavior |
| --- | --- |
| 8.1 Authored controller and authority | Singleton Character Controller settings are authored components; an enabled body owns its transform and collider during Play |
| 8.2 Fixed-step input intent | XZ movement and held/pressed jump intent cross the existing native/editor input boundary; a press survives zero-step frames and is consumed once |
| 8.3 Support and velocity | Speculative casts, flat support, acceleration, gravity, sliding and bounded fall speed produce accepted displacement and motion state |
| 8.4 Jump timing and air control | Fixed-tick coyote and press-buffer deadlines, one accepted jump per press, air acceleration and velocity clipping |
| 8.5 Step-up and landing | Bounded up/forward/down box-step attempts, ceiling rejection, exact landing transitions and shared collision-query work |
| 8.6 Inspector and parity | Typed controller form, committed motion inspection, Controller Room fixture, headless replay and package tests |

The initial solver is an upright box controller, not a dynamic rigid body, capsule solver or triangle-mesh slope
solver. The enabled owner must be a scene root with identity rotation, unit scale and an enabled blocking Box
collider. Its visual descendants cannot have enabled colliders. Axis-aligned box support is walkable; sensors,
spheres and rotated box bounds do not become flat walkable floors. Initial penetration is rejected.

Linked source authoring remains legal, but linked members are parented beneath an anchor. **Unpack** preserves that
hierarchy. Unpack, then separately reparent the controller owner to the scene root with world transform preserved,
and revalidate its orientation/scale before Play. Disabled controller records preserve their authored settings and
can remain parented. Configured Move/Spin and module transform actions may not compete with an enabled controller;
actions cannot enable a second collider beneath its body.

Defaults are walk speed 4, ground acceleration 30, air acceleration 8, gravity 20, fall-speed cap 30, jump speed 7,
ground probe 0.02, step height 0.25, and five ticks each for coyote time and jump buffering. Speeds/accelerations are
finite and bounded; jump windows permit 0–120 ticks. There are at most 64 enabled controllers. Probe and step sizes
must fit the body's dimensions. The Inspector exposes the actual settings and a jump action ID, which must exist
in the project's Game Input profile.

`set_character_input { key, movement_xz, jump_held }` normalizes diagonal/oversized finite input. Movement uses XZ;
legacy controlled movement retains its established adapter. Native physical input and browser code input feed the
same held/pressed state. Key repeat does not add jump presses. Focus/pause neutralization discards held and pending
input; a new press afterward works normally. Prefab admission preserves pending intent without replaying it.

The solver stages support, velocity, coyote/buffer deadlines, jump availability and jump/landing timestamps together.
After support removal/disable, grounded/support are cleared before later systems read the staged motion.
`CommittedMotion` exposes only accepted state. The animation system uses the same step's staged resolved motion
inside the transaction; public committed motion changes only after success.

The M7 adapter maps a producing-step jump/landing timestamp to a next-step `jumped`/`landed` signal for each enabled
state machine on that controller's owner. Each recipient is also its own machine sender endpoint. This is a narrow
internal authority adapter, not a claim that arbitrary controller component IDs are message endpoints. Source-filtered
transitions therefore name the recipient's own machine attachment. Signals and timestamps roll back together.

Open `apps/game/controller-demo.json`. Use WASD and Space to walk, jump, test the small step and tall obstacle, and
approach the low ceiling. Holding Space must not bounce repeatedly after landing. Runtime inspection includes
grounded state, accepted planar/vertical speed and the last jump/landing ticks. The box body remains authoritative
even when a capsule or animated mesh is used as its visual.

## Milestone 9: bounded skinned animation

| Feature | Implemented behavior |
| --- | --- |
| 9.1 CPU animation assets | Shared bounded embedded-GLB decoder, source fingerprints, dense transform hierarchy, skin influences, clips and material images |
| 9.2 Clip sampling | STEP/LINEAR translation/scale and shortest-path quaternion interpolation, exact looping/clamping, global transforms and mesh-local palettes |
| 9.3 Animation lifecycle | Shared immutable assets, independent fixed-tick clocks/poses, staged publication, restart/spawn/retirement ownership and animation-only scheduling |
| 9.4 Skinned rendering | Separate influence stream and shader, per-frame palette storage, material-preserving push fields and conservative animated bounds |
| 9.5 Locomotion and preview | Idle/Walk/Air selection from resolved controller motion, six-tick transition fades and isolated pinned native clip preview |
| 9.6 Acceptance fixture | Two-joint GLB with three clips, two materials and blended influences; independent-loop companion, replay/package/CPU and opt-in native tests |

The shared decoder is `bridge::animation_asset`; CPU types and sampling live in `engine_core::animation`. Runtime
and renderer adapters own uploaded resources. Headless and compiler paths use the same CPU decoder. Animation
bindings capture the catalog asset ID, SHA-256, selected scene, mesh node and skin, and must match the entity's mesh
asset and selected mesh. A changed source is explicitly reviewed/adopted; it is not silently substituted at Play.

### Supported asset and material subset

Use an embedded GLB containing one selected skinned mesh node, one skin, 1–128 distinct joints, up to 256 transform
nodes and 1–32 nonempty clips. Hierarchy depth is bounded to 128. Geometry permits at most 100,000 vertices,
300,000 indices and 256 triangle primitives. The encoded asset and decoded retained data are each bounded to
32 MiB; the JSON chunk is bounded to 2 MiB. A world supports 64 animation instances and a 128 MiB aggregate
immutable-asset budget, sharing repeated sources rather than charging each reference as a new decoded asset.

Four influences per vertex use finite nonnegative weights near a unit sum, normalized after validation. Sparse
accessors, morph animation, CUBICSPLINE, extra influence sets, external buffers/images and required extensions are
outside this subset. Times must be finite, strictly increasing and bounded to 600 seconds, with at most 4,096 keys
per track and 262,144 keys in the asset. Repeated node/property channels are rejected.

Opaque base-color factors and optional embedded PNG/JPEG base-color images are supported, with TEXCOORD_0 and
linear/repeat sampling without mipmaps. Images are bounded to 4,096 pixels per dimension and share the decode byte
budget. Other texture maps, unsupported alpha modes and sampler policies are rejected. This does not add full PBR.

Animated TRS and placement must preserve positive uniform scale with no shear/reflection; inverse operations use
basis checks rather than a single absolute determinant cutoff. Mesh-local palettes follow
`inverse(G_mesh) * G_joint * inverse_bind`. The placed mesh matrix is applied once at render time. Weighted joint
envelopes provide conservative bounds for accepted poses; disabled rest poses still update their world-space bounds
when their owners move.

The native Edit preparation and incremental update paths validate animated world placement too. A nonuniform,
reflected or otherwise unsupported placement is rejected before replacing the accepted native scene. A final
submission guard also protects direct render callers. Source positions and inverse-bind translations are bounded
to keep the shader's intermediate mesh-local arithmetic finite, as well as the final analytical world bounds.

### Playback, authoring and native preview

An Animation component chooses `single_clip { clip_index, looping }` or `locomotion3 { idle, walk, air }` and an
integer speed ratio. Numerator/denominator are 1–16 and the ratio is 1/4–4. Single clips can loop or stop at their
last sample. Nonfinished playback and fades keep an otherwise idle simulation scheduled. Disabled components show
rest pose without advancing clip time. Pause preserves accepted poses; restart rebuilds fresh instances.

Locomotion requires an enabled Character Controller on the same entity. Air has priority when not grounded. On the
ground, Idle enters Walk above 0.1 units/second and Walk returns to Idle at or below 0.05. Motion uses accepted
displacement, so pushing against a wall does not select Walk solely from held input. Transition fades last six fixed
ticks and capture the outgoing displayed pose, including interrupted fades. Clips remain in-place; gameplay movement
does not come from animation root motion.

Select a catalogued model in the Animation form and choose **Review Animation Asset**. Select clips and speed, make
the Mesh Renderer use that same asset/mesh, then **Apply Component**. For an applied Scene entity, choose a preview
clip, **Open Native Preview**, enter seconds and choose **Show Pose**. **Close Preview** releases that preview.
The native token is tied to workspace generation/revision and pins immutable accepted bytes. Scrubbing changes no
authored document, history, run progress or Play clock. Revision/workspace changes make old previews stale. Invalid
sample times preserve the last accepted pose. Reopening after source changes requires fresh validation.

The renderer keeps the 96-byte `ModelPushConstants`: `flags.x` retains material/draw semantics while `flags.y/z`
carry palette base/count. The existing 32-byte vertex stream remains unchanged; skin influences use a separate
24-byte joint/weight stream. Set 2/binding 0 is palette storage, bounded to 64 × 128 matrices. Palette writes follow
frame completion and remain separate from ordinary static instancing. Shader compilation and CPU layout tests do
not by themselves prove native image quality or resource retirement.

Open `apps/game/animation-demo.json`. The controller-driven player chooses Idle/Walk/Air while `clip-demo` runs an
independent looping clip. The authored 3,568-byte `characters/two-joint.glb` contains a translated mesh basis, two
joints, blended influences and blue/green material primitives. Its named clips are Idle, Walk and Air at indices
0, 1 and 2. The fixture is intentionally small enough to inspect and package with the sample asset catalog.

## Run and verify

Run these CPU samples from the repository root:

```powershell
cargo run -p engine-headless --offline --locked -- --project apps/game/event-demo.json --steps 3
cargo run -p engine-headless --offline --locked -- --project apps/game/controller-demo.json --steps 60
cargo run -p engine-headless --offline --locked -- --project apps/game/animation-demo.json --steps 60
```

The event command demonstrates startup contact and queued machine progression, not the whole movement route.
The replay tests drive movement, pickup, jump, pause and restart through real command/tick inputs:

```powershell
cargo test -p engine-core --test gameplay_events_api --test character_controller_api --offline --locked
cargo test -p bridge --test animation_asset_api --test animation_asset_safety --offline --locked
cargo test -p engine-headless --test event_replay --test character_replay --test animation_replay --offline --locked
cargo test -p compiler --test event_package --test character_package --test animation_package --offline --locked
```

The checked-in [event route](verification/milestones-7-9-event-route.json),
[character route](verification/milestones-7-9-character-route.json) and
[animation route](verification/milestones-7-9-animation-route.json) reproduce the packaged comparison inputs:

```powershell
cargo run -p engine-headless --offline --locked -- --project apps/game/animation-demo.json --replay docs/verification/milestones-7-9-animation-route.json
```

The event route completes one assembly and waits for its door reset. The movement routes jump, release the press,
move, stop and land. Acceptance also appended `restart_run` to each route and checked the new world's empty clocks
and matching native-state hash against its authored source.

To create a CPU snapshot, choose an existing absolute destination parent. A new child named from the manifest will
be created; occupied outputs are rejected. `--export` additionally builds the native Player. Neither command launches
the game automatically.

```powershell
cargo run -p compiler --offline --locked -- --stage 'D:\Coding\Game Engine\apps\game\animation-demo.json' 'D:\Snapshots'
cargo run -p compiler --offline --locked -- --export 'D:\Coding\Game Engine\apps\game\animation-demo.json' 'D:\Exports'
```

Open the resulting `project.json` in the editor, or run the exported `game.exe --player` from its package. Use the
existing native controls, including F10 for a new run. Headless tests decode animation CPU data but create no GPU or
window. The opt-in hidden Vulkan test is separate:

```powershell
cargo test -p game --offline --locked animated_characters_draw_independent_palettes_preview_and_restart -- --ignored --nocapture
```

### What the replay hashes mean

The historical `state_hash` remains FNV-1a-64 over `project_and_render_scene`. It is not a whole simulation hash.
`native_components_hash` separately hashes the complete `native_components` JSON projection under scope
`native_components_v1`, with the same algorithm and a reported byte count. It is null data in Edit mode.

The projection includes every machine, queued envelope, due step/sequence, endpoint key/generation/same-world flag,
lifecycle timer/request, controller state/input and animation clock/fade/pose/palette/bounds. It is not restricted
to the runtime inspector's first 128 entities. Random world UUIDs are normalized. Opaque module-owned state,
game progress, scheduler accumulator, trigger history, presentation revisions and rendered pixels remain outside
this hash. Hash equality is replay evidence within this stated scope, not a cryptographic integrity guarantee.

### Verification and remaining acceptance boundaries

The all-features/all-targets Rust workspace run passed **937 tests**, with 11 ignored. The editor passed **467 tests**;
the retained admin and Python suites passed 15 and 51 respectively. Rust documentation checks passed with zero actual
doctests. Production frontend checks passed with the existing project-setup stylesheet budget warning.
Focused suites overlap these totals. The documentation site built and link-checked 64 pages and passed all eight
tests; Game App Developer remains empty.

Fresh release packages for all three samples were created outside the checkout. Their complete route and restart
native-state projections matched their authored sources, as did the historical project/render-scene hashes.
The exported animation package also passed the hidden native Player harness on an NVIDIA GeForce RTX 4070 with
Vulkan 1.4.341. It exercised independent palettes, two materials, Idle/Walk/Air, preview restoration, Stop/Play,
Player restart, staged skin import, source pinning, invalid placement, and four animated spawn/despawn cycles.
The warmed resource baseline returned to 32,932 logical GPU bytes and five resources; validation reported zero errors.

An explicit CPU-only debug measurement of the two-joint fixture ran 64 independent animation instances for 300 ticks
in 276.509 ms on `MISHAPC`, approximately 922 microseconds per tick. It shared 3,171 logical asset bytes and produced
8,192 palette bytes per frame. This measures pose evaluation for this small fixture, not frame time or FPS, and is
not a worst-case 256-node/128-joint benchmark.

The tests cover startup overlap without accidental Reset, exact door deadlines, typed prefab remapping, disabled
definitions, aggregate rollback, shared controller/event emission caps, next-step Jumped/Landed, native-state hash
normalization, source-version rejection, mesh-local basis math, small scales, bounds, shared assets, independent
clocks, one-shot scheduling and embedded base-color images. Visible desktop interaction, OS-level keyboard delivery,
another-machine release operation and measured frame-time budgets are separate acceptance claims.

The implemented scope deliberately retains authored flat state tables, root-level box movement, a bounded embedded
GLB subset and in-memory run state. General scripting/graph execution, capsule or arbitrary-slope physics, root
motion, morph blending, arbitrary animation graphs and persistent checkpoint/save-game storage are future work.

## Next learning path

The local, Git-ignored `.learning-guides/README.md` starts the next three milestones: playable camera controls (10),
deliberate interaction with HUD/audio feedback (11), and checkpoints/save slots/Continue (12). Each has six ordered
features with suggested commits, implementation examples, conceptual explanations, failure tests and resources.
These are future implementation guides, not shipped functionality or a compiled patch series.
