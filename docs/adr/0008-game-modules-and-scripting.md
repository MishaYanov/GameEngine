# ADR-0008: Game modules and scripting

- **Status:** Accepted native module and typed game-state direction; scripting runtimes, script assets and reload
  extensions remain proposed.
- **Date:** 2026-10-27
- **Decision update:** 2026-10-29, following explicit user acceptance of the researched recommendations.
- **Scope:** Gameplay extension contracts, runtime selection, lifecycle, and failure containment.

Authoritative accepted core: [Resolved decision](resolved/0008-native-modules-and-typed-progress.md). This original URL
retains research, history and remaining proposals; those extensions are not resolved by the core implementation.

## Context

A guard's script should read perception results, request movement, and set an animation parameter. Attaching that script
to another object should create independent runtime state while sharing its code asset. An infinite loop or invalid
request must not silently corrupt the authored scene or leave the editor unable to stop Play.

The editor already presents script subjects, but a script picker is not an execution architecture. Decide the
engine-facing boundary now; choose a new execution runtime only after measuring representative workloads and defining
the trust model.

## Current implementation

Inspected commit `b6ec4ba5a88c5ef66eff82fa61eb5747b940400d` plus staged, unstaged and untracked changes on
2026-10-29. The drafting baseline was `fcb748b`; the current optimization and transition changes were inspected again:

- `crates/engine-core/src/simulation_session.rs` defines `GameModule`, `GameModuleFactory`, read-only world planning,
  and an `ActionWriter` capped at 4,096 actions. Its configured module supports movement and spin behaviors. These are
  native engine integrations, not a general scripting language.
- `GameModuleFactory::create_prepared` receives engine-constructed `PreparedModuleConfig` with project, scene ID and
  resolved behaviors. The default forwards to `create` for existing factories; the configured factory consumes the
  prepared behavior list. This avoids repeating configuration work in the normal construction path.
- `GameSession` in `crates/engine-core/src/game_session.rs` shares `Arc<ProjectData>` with its active simulation and
  prepared scenes. It owns scene transitions with owner/generation checks. It now owns typed ProgressState, retains it
  across ordinary transitions and prepares fresh defaults for New Game. GameplayContext/ProgressView and ActionWriter::
  push_progress provide typed candidate operations with native compatibility defaults.
- `apps/editor/src/app/core/subjects/subject.models.ts` defines script references and editable property descriptions.
  Those UI models do not establish loading, script execution, isolation, or hot reload.

No embedded interpreter or Wasm runtime is selected. Acceptance of the native direction does not establish a plugin
ABI, runtime isolation or behavioral verification of the full current working tree. Typed in-memory progress is
implemented; the resolved page records its scope and attributed evidence.

## Dependencies and assumptions

[ADR-0002](0002-object-composition-and-components.md) defines authored `ScriptComponent`
data; [ADR-0004](0004-authoring-play-and-editor-state.md) separates it from runtime
state; [ADR-0005](0005-runtime-lifecycle-and-scheduling.md) determines execution
phases. [ADR-0006](0006-asset-identity-and-import-pipeline.md) supplies script assets and
imports, [ADR-0007](0007-graph-assets-and-execution.md) supplies animation parameter contracts,
and [ADR-0010](0010-input-actions-and-possession.md) defines input actions. Script tooling
uses [ADR-0003](0003-rust-typescript-bridge.md); gameplay does not require a round trip through Angular.

These dependencies define boundaries before a scripting language is chosen. They do not justify freezing a plugin ABI or
all public callbacks today.

## Accepted native module and typed state decisions

### Game-defined types and operations

Game code defines progress structures and meaningful operations; the engine manages their lifetime, bounded execution,
validation/commit boundary and read-model delivery. For example, a game can define `CampaignProgress { score: i32,
has_courtyard_key: bool }` with operations such as `AwardScore` and `UnlockCourtyard`. These names belong in the game
layer rather than `engine-core`. Built-in components retain their own typed schemas.

A universal string-keyed integer/flag dictionary is not the primary gameplay-state API. A later scripting/designer
variable feature may define a separate versioned dynamic schema. Exposed numeric representations must follow
ADR-0003; `i32` is a suitable initial example for JSON/TypeScript, while unrestricted `i64` needs an explicit wire rule.
The resolved page records the current Rust trait/type-erasure representation; it does not freeze a plugin ABI, transport
schema or all future public signatures.

Unreal exposes game-defined instance classes and lifetime-scoped subsystems. Bevy resources are typed values, with one
resource of a type per `World`; persistence follows the owner that actually survives scene changes. Unity's
ScriptableObject assets support reusable configuration shared by reference. These precedents motivate explicit types
and lifetimes here; they do not impose an ECS migration, automatic resource immutability, or a particular Rust ABI.
[Unreal GameInstance](https://dev.epicgames.com/documentation/en-us/unreal-engine/API/Runtime/Engine/UGameInstance),
[Bevy resources](https://docs.rs/bevy/latest/bevy/prelude/trait.Resource.html),
[Unity ScriptableObject data assets](https://docs.unity.com/en-us/engine/6000.0/manual/scripting/get-started/fundamental-unity-types/class-scriptable-object).

### Construction, planning and application

Preserve the engine-prepared factory path. Factory construction can resolve immutable configuration and allocate
scene-local module data, but cannot award progress, publish gameplay events or activate a prepared scene. A candidate
may never be installed. Mutable game-run state belongs to `GameSession`, independently of each scene's module.

Planning now receives GameplayContext with a read-only ProgressView of committed state or current candidate values
within the aggregate update. Submit domain requests through a bounded writer. Game code validates its domain rules; the
engine enforces scheduling, action limits, scene/session identity and the commit boundary. The current typed view/writer
API has explicit compatibility defaults and is recorded in the resolved page.

Stage supported world and progress changes under ADR-0005's aggregate update contract. Publish events after successful
commit. Host/debug operations use the same domain operations between updates. `plan(&self, ...)` does not prohibit
interior mutation, so arbitrary module-owned side effects are outside the promised rollback scope. The lazy progress
candidate establishes the declared boundary only when trusted native clones are independent and validation/application
has no external effects.

Keep native Rust modules as the first extension path. Choose interpreter/Wasm support only after workload, trust and
packaging evidence. Session-boundary code reload remains the initial policy; live reload and migration remain future
decisions. Typed in-memory progress across scenes does not provide disk saves.

## Remaining scripting proposals and contract elaboration

The native ownership, typed-state and staged-operation direction above is accepted. The following language-neutral
extension details, script components, capabilities, callback APIs and runtime alternatives remain proposals where
they extend that core. They do not require implementing a general scripting platform for the next feature.

### Define an extension boundary independent of language

Retain native Rust modules for engine-owned behavior. Define a narrow, versioned gameplay API that runtime adapters can
implement: query a bounded snapshot/view, consume normalized input and domain events, and submit validated intents or
actions. The engine remains responsible for entity lifetime, collision, transforms, component invariants, and command
application.

Use opaque, generation-checked entity handles and asset IDs. Prevent a deleted entity's handle from addressing a
replacement. Batch queries and actions when profiling shows boundary cost; do not expose a raw world pointer, Vulkan
resource, global workspace lock, or unlimited mutation callback. Reject unsupported API versions and undeclared required
capabilities at load time.

Gameplay requests modify runtime state through the simulation pipeline. Persisting an authored change remains an
explicit editor command under ADR-0001. Scripts cannot bypass that boundary by invoking Tauri commands. Animation
controllers, audio sources, and movement motors expose domain operations; a script is not the owner of all their
internal storage.

### Separate definition, component, and instance

A script asset contains code, a versioned exported property schema, dependencies, and engine API requirements.
`ScriptComponent` stores its asset reference, enabled flag, and validated authored properties. Each enabled
ScriptComponent on an active entity gets private transient state keyed by entity, component instance ID, and session
generation. Two scripts attached to the same entity therefore remain independent.

Property descriptors are data that the inspector can render without executing gameplay code in the WebView. Prefer
build/import-generated descriptors. If generating descriptors requires running code, run that work through the same
restricted import policy rather than during ordinary property inspection. Defaults and migrations are validated by Rust;
language objects or closures are not the persistence format.

### Specify lifecycle and failures before adding callbacks

Use explicit phases such as create, enable, fixed-step update, event delivery, disable, and destroy, aligned with
ADR-0005. Final callback names remain an API design task. Initialization must complete before updates; disabling stops
further updates; teardown must invalidate handles, subscriptions, and pending tasks. Deterministic ordering comes from
the scheduler rather than incidental container iteration.

Start with synchronous, bounded update calls and deferred action batches. No unrestricted background task or direct
blocking I/O runs on the simulation thread. Async asset requests return handles/results at defined phase boundaries.
Time, random seeds, and input come from engine context when reproducibility is required; choosing a scripting runtime
alone does not guarantee deterministic simulation.

Stage each callback's actions and discard that batch on a callback error. Define whether the faulty instance is disabled
or Play pauses through a visible development policy. Report script asset/revision, entity, phase, and source location
when available. Rate-limit repeated failures. Engine errors are not converted into successful script outcomes.

An action count limit does not stop an infinite loop before actions are emitted. Native code remains trusted and can
stall or terminate the process. A Rust trait object is neither a sandbox nor a stable dynamically loaded plugin
interface: the Rust ABI has no stability
guarantee. [Rust Reference: external ABIs](https://doc.rust-lang.org/reference/items/external-blocks.html). A future
native plugin protocol would need explicit ABI, allocator, ownership, panic, and version rules.

### Make reload transactional

Initially reload only between sessions. Later, compile/load a candidate in isolation, validate its schema, then replace
at a safe scheduler boundary. Keep the previous valid instance if preparation fails. The default is to restart script
state; state preservation requires explicit versioned serialization and migration. Cancel tasks from the old generation.
Do not unload native code while threads, callbacks, or function pointers can still reference it.

## Runtime alternatives to measure

| Option                                                  | Benefit                                            | Cost or limit                                                                                 |
|---------------------------------------------------------|----------------------------------------------------|-----------------------------------------------------------------------------------------------|
| Native Rust compiled with the game                      | Direct engine integration; existing module pattern | Compile iteration time, platform builds, trusted execution; no safe arbitrary unload promise. |
| Embedded Lua                                            | Purpose-built embedding and small language surface | New language/tooling, binding and GC cost, capability/limit configuration required.           |
| Embedded JavaScript, potentially authored as TypeScript | Familiar tooling and generated API types           | Separate VM from Angular, transpilation/source maps, GC and runtime integration work.         |
| Wasm guest modules                                      | Explicit imports and a portable execution boundary | Compilation/startup, guest memory and ABI crossings, debugging and packaging costs.           |

Lua exposes instruction-count hooks, but those hooks run during Lua execution; host functions need their own
bounds. [Lua reference manual](https://www.lua.org/manual/5.4/manual.html#lua_sethook). QuickJS provides memory, stack,
and interrupt controls worth prototyping; their presence alone does not establish a secure
sandbox. [QuickJS embedding API](https://bellard.org/quickjs/quickjs.html#Memory-handling).

Wasmtime offers deterministic fuel accounting and lower-overhead epoch interruption with different scheduling
properties. It is an evaluation candidate, not an adopted dependency. Execution limits must also cover host calls,
memory, and emitted work. [Wasmtime interruption mechanisms](https://docs.wasmtime.dev/examples-interrupting-wasm.html).

## Open decisions and acceptance gates

Decide whether scripts are trusted project code, downloaded mods, or both. Default the API to no filesystem, network,
process spawning, native library loading, or editor access; grant required capabilities explicitly. An untrusted-code
product promise needs a separate isolation review and may require process isolation. Language familiarity cannot decide
this question alone.

Benchmark the same small behavior, many active instances, event bursts, allocations, idle instances, and reload workflow
across candidates. Record frame-time percentiles, boundary-call count, memory, startup, packaging size, supported
platforms, and debugger usability. Set acceptance budgets before choosing the runtime, and record that selection in a
follow-up ADR.

## Implementation sequence and verification

1. Implemented native core: CampaignProgress and domain operations owned for the GameSession lifetime. Preserve
   shared project ownership and the prepared factory path.
2. Implemented native core: GameplayContext/ProgressView, bounded typed requests, aggregate world/progress staging and
   prepared New Game. Ordinary transitions retain progress; fresh runs initialize defaults. Keep inspector/debug wire
   types explicit and gameplay meanings in the game layer.
3. Verify the declared rollback boundary, no events from rejected work, preparation without progress effects, and
   stale/foreign session rejection. Runtime acceptance is separate from documentation publication.
4. Before adding scripts, specify remaining callback lifecycle, handles and API versions; test deletion during
   callbacks,
   stale handles, invalid requests, failed initialization, action overflow,
   stop/restart, and teardown ordering.
5. Build isolated Lua/JavaScript/Wasm feasibility prototypes against the same narrow API; keep production dependencies
   unchanged until selection.
6. Verify timeout and memory behavior, including expensive host calls, and demonstrate that Stop remains responsive for
   the selected trust model.
7. Add script asset import, generated property descriptors, diagnostics, then session-boundary reload. Add live
   migration only after failure/recovery tests.

The accepted native core is now implemented in the inspected tree and recorded in the resolved page. Scripting-runtime
selection remains open. This resolution changes documentation only; attributed behavioral evidence has snapshot limits.
No scripting performance, sandbox or hot-reload guarantee follows from subject UI, compile success or publication
checks.
