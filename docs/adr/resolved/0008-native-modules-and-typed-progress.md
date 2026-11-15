# Resolved ADR-0008: Native modules and typed game progress

**Decision status:** Accepted native module and typed-progress core, explicitly agreed on 2026-10-29.

**Implementation status:** Trusted native factories, typed game-defined progress/actions, engine-owned bounded
candidates and read-only planning context are present in the working tree. No scripting runtime, plugin ABI, sandbox or
live-reload system is selected or delivered.

**Verification status:** Project5 reports a successful workspace lib/bin compile check with cache hard-link fallback
warnings, not behavioral/native interactive acceptance. Newer external regression evidence is attributed below with
snapshot limits. Documentation maintenance executes only site checks.

## Decision

Game code defines meaningful state and operations; the engine owns run lifetime, bounded staging, validation/commit
boundaries and read access. A universal string-keyed integer/flag map is not the primary game-state API. Keep gameplay
meanings outside engine-core and retain native Rust modules as the first extension path.

Factories construct scene-local modules from immutable engine-validated configuration. They must not award progress,
publish gameplay events or activate a candidate that may never be installed. Shared `Arc<ProjectData>` and
`PreparedModuleConfig` avoid repeated configuration copying/resolution. Typed in-memory progress survives ordinary scene
changes and resets only through an explicitly new run; this does not provide disk persistence.

## Current Rust contract

| API/source                                                   | Implemented role                                                                                                                                                                      |
|--------------------------------------------------------------|---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `game_progress::GameProgress`                                | Game-defined `Clone + PartialEq + Send + Sync + 'static` state with an associated Action type, validation, byte accounting and domain application                                     |
| `ProgressState`, `ProgressAction`, `ProgressView`            | Engine-owned type-erased storage, checked typed actions and immutable `get::<P>()`/revision access; wrong progress types fail                                                         |
| `GameSession`                                                | Owns one progress value; supports typed reads and between-tick atomic action batches; ordinary scene preparation preserves it, prepared New Game replaces defaults/identity on commit |
| `GameplayContext`, `ActionWriter::push_progress::<P>`        | Read-only world/input/step/progress for module planning and bounded typed progress requests in the common action buffer                                                               |
| `GameModuleFactory::create_progress`                         | Run initialization, defaulting to empty progress for compatibility; ordinary scene preparation does not recreate progress                                                             |
| `create_prepared(PreparedModuleConfig)`                      | Engine-prepared project/scene/resolved-behavior path; the default forwards to existing `create` implementations                                                                       |
| `GameModule::plan_with_progress`, `needs_tick_with_progress` | Progress-aware hooks whose defaults forward to existing `plan` and `needs_tick`; previous native modules can retain the original methods                                              |

State is limited to 64 KiB of reported retained storage, an action to 4,096 reported bytes and a transaction to 64
progress actions. Owned heap storage must be included. These caps complement the aggregate simulation budgets
in [resolved ADR-0005](0005-serial-scheduling-and-rollback.md); they do not bound all native allocations or prevent a
callback from stalling.

The transaction lazily creates an independent candidate, applies/validates actions, and increments progress revision
once if final state differs. Later fixed steps in the same aggregate update see candidate values; observers see the
committed value only after success. A no-op batch leaves revision unchanged. Rejected batches leave committed progress
intact. Native clones must not alias mutable state, and domain validation/application must be side-effect-free. The
engine cannot undo arbitrary module interior state or external I/O.

## Implemented game example

`crates/courtyard-gameplay/src/lib.rs` defines `CampaignProgress` with score, key ownership, exit reached and
completion. Its typed actions award score, collect the key, reach the exit and complete the run. Nonnegative score,
checked addition, the key requirement and the exit requirement enforce the domain rules. `CampaignFactory` supplies
defaults and wraps configured movement and spin through the prepared factory path.

The authored role bindings identify entities for the proximity checks. Key pickup awards one point once. After
pickup, reaching the exit commits progress with a typed request for `finish`. The Finish marker completes the
run after that exit and the module stops planning
movement. [Play the route](../../apps/game/README.md#demo-progress-and-new-game).

The bundled manifest selects `courtyard_run`. Editor Play, the Player and the headless runner use the shared
resolver. It reads the current authored project at the start of a run. The selected factory supplies the scene
module and initial typed progress. Projects with no selected module use configured behaviors and empty progress.
See [the manifest contract](../../project-lifecycle.md#gameplay-module-selection).

The native player title exposes key, exit, score and completion. F7 awards a point and F8 cycles scenes before
completion. F10 prepares New Game at the configured startup scene. Editor Game shows the module's read-only status
and exposes Restart Run through the viewport action channel.

## Rationale and remaining proposals

Typed domain actions make game meaning, validation and failure scope inspectable. Engine ownership gives state a
lifetime independent of scene modules without making progress an authored asset or global singleton. Read
the [original rationale and alternatives](../0008-game-modules-and-scripting.md#accepted-native-module-and-typed-state-decisions)
for context.

The representation above is the current Rust implementation, not a stable dynamically loaded ABI or a final public
transport schema. Script components/assets, an interpreter/Wasm choice, capability sandboxing, generated script
descriptors, callback lifecycle, module code reload/migration, general events and persistent game saves remain open
in [original ADR-0008](../0008-game-modules-and-scripting.md). Subject UI does not implement those features.

## Evidence and limits

The prior documentation checkpoint attributes 555 passing workspace tests/zero failures/two ignored to TEST/root,
including three progress regressions for type/atomic/budget rejection, aggregate world/progress rollback and scene
retention/New Game/token invalidation. Current module-selection checks and packaged Player evidence are recorded in
`docs-site/docs-state.json`. The packaged-player smoke check covers startup and initial progress. Key acquisition,
scene cycling and restart still need native interaction acceptance.
