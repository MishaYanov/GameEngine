# ADR-0003: Lightweight Rust–TypeScript contracts and transport

Status: Proposed; research and implementation plan. No bridge migration or performance acceptance is claimed.

Date: 2026-10-27

Scope: Rust engine definitions, Tauri desktop IPC, Angular editor models, object inspection, and bulk data.

## Recommendation

Use Rust-owned, versioned data transfer objects (DTOs), generated TypeScript contracts, and JSON over Tauri commands
for ordinary editing. Use receipts and targeted state queries to reduce traffic. Use Tauri channels for subscribed
streams with application-level flow control. Introduce raw binary buffers only for workloads that measurements show
need them. Keep simulation, pose evaluation, assets, and Vulkan resources in Rust.

This extends [ADR-0001](0001-editor-command-pipeline.md). Object composition and component naming are defined in
[ADR-0002](0002-object-composition-and-components.md). The proposals do not require an ECS framework, XML, a web server,
or a second copy of the engine in WebAssembly.

The likely first win in this repository is sending less state after an edit. Whether JSON encoding itself becomes a
bottleneck is an open measurement question. A smaller message is not automatically a faster interaction, and a faster
Rust serializer does not establish lower WebView latency.

## Evidence and current implementation

Research used official project documentation, specifications, upstream source, and the local tree. The inspected Git
baseline is `a377b1f87dcc2972851c1518e4fd3cf1dc1e8951` plus staged, unstaged, and untracked changes on the date above.
HEAD is unchanged from the previous documentation checkpoint, but the object API and editor work have changed.

| Inspected surface                                   | Current behavior and implication                                                                                                                                             |
|-----------------------------------------------------|------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `Cargo.lock`                                        | Resolves Tauri 2.11.5, Serde 1.0.229, serde_json 1.0.151, glam 0.33.6. These are the research baseline, not a recommendation to upgrade.                                     |
| `apps/editor/package-lock.json`                     | Resolves `@tauri-apps/api` 2.11.1. Native-host and JS API compatibility must be tested together.                                                                             |
| `crates/engine-core/src/command.rs`                 | Typed Serde commands, strict fields, tagged variants, transform DTOs, snapshots, and numeric `u64` revisions already exist.                                                  |
| `crates/bridge/src/editor.rs`                       | Re-exports the public command graph and defines viewport DTOs. No Rust-to-TS generator is installed.                                                                         |
| `apps/editor-host/src-tauri/src/commands/editor.rs` | Protocol 2; full-snapshot execution and receipt execution both exist. Receipt execution emits `editor://changes-ready`. Work is already dispatched through blocking workers. |
| `crates/engine-core/src/events.rs`                  | Journal capped at 256 entries and 1 MiB, with cursor-based recovery. Entries invalidate categories/keys; they are not complete value deltas.                                 |
| `apps/editor/src/app/core/editor/editor.service.ts` | Ordered UI mutation queue, handwritten TS types, and `editor_execute` returning a full snapshot. The smaller receipt endpoint is not yet this service's execution path.      |
| `apps/editor/src/app/core/objects/`                 | Injectable CRUD/query/duplicate/reparent/watch contracts; operations reject `not_implemented`. They are not native object endpoints.                                         |
| `apps/editor-host/src-tauri/src/viewport.rs`        | Native runtime/viewport synchronization already owns rendering. There is no need to send every simulated object to Angular every frame.                                      |
| `crates/bridge/tests/editor_contract.rs`            | Existing Rust serialization fixtures cover strict commands, transforms, scene lifecycle, and component-shaped fields. They do not yet validate generated TypeScript.         |

Tauri documents structured command arguments/results, optimized raw responses, and raw request bodies. Its event
guide recommends channels for ordered streaming instead of high-volume events. These are transport capabilities,
not measured performance results for this engine. [Tauri command guide](https://v2.tauri.app/develop/calling-rust/),
[Tauri frontend guide](https://v2.tauri.app/develop/calling-frontend/).

## Separate schema, representation, transport, and ownership

| Concern                    | Proposed choice                                                                             |
|----------------------------|---------------------------------------------------------------------------------------------|
| Semantic authority         | Rust validates and mutates engine state; UI holds drafts and read models.                   |
| Public schema              | Small explicit Rust DTOs and typed commands exposed through `bridge`.                       |
| TypeScript declarations    | Generated at development/build time; imported with `import type`.                           |
| Ordinary representation    | JSON-compatible records, arrays, strings, finite numbers, booleans, and null.               |
| Request/response transport | Tauri `invoke`, hidden behind `EditorClient`.                                               |
| Notifications              | Small wakeups that trigger authoritative queries.                                           |
| Streams                    | Tauri `Channel`, scoped to a subscription and bounded by our protocol.                      |
| Bulk representation        | Optional versioned bytes over raw IPC or a scoped native asset URL.                         |
| Persistence                | Existing scene/project JSON plus explicit migrations; independent of IPC envelope versions. |
| Runtime storage            | Rust's own structures, handles, and caches; not prescribed by the wire schema.              |

A Rust struct is not a JavaScript object shared by reference. Tauri crosses a process boundary. The frontend receives
values, not Rust pointers, methods, mutexes, trait objects, or GPU handles. TS classes cannot retain a Rust object's
identity or ownership merely by deserializing its fields. Tauri describes this boundary as asynchronous message
passing. [Tauri IPC model](https://v2.tauri.app/concept/inter-process-communication/).

### DTO families and naming

| Contract                                    | Purpose                                                                                 |
|---------------------------------------------|-----------------------------------------------------------------------------------------|
| `ObjectRefDto`                              | Scene ID plus stable object key; session generation belongs to the enclosing request.   |
| `ObjectSummaryDto`                          | Hierarchy row: identity, parent, name, enabled state, and component kind summary.       |
| `ObjectDetailsDto`                          | Selected object's authored values and typed component instances.                        |
| `ComponentDescriptorDto`                    | Versioned component/field definitions, defaults, constraints, and supported operations. |
| `ComponentInstanceDto`                      | Instance ID, component type/version, enabled state, and typed configuration.            |
| `TransformDto` / `TransformEditDto`         | Existing authored transform and explicitly targeted transform replacement.              |
| `CommandEnvelopeDto` / `CommandReceiptDto`  | Validated intent and authoritative acceptance metadata.                                 |
| `SnapshotBaselineDto` / `ChangePageDto`     | Consistent state baseline and recoverable change consumption.                           |
| `StreamDescriptorDto` / `StreamFrameHeader` | Optional stream identity, format, sequence, and lifetime.                               |

These names describe proposed responsibilities; most are not existing types. Avoid exporting `Engine`, `World`,
renderer objects, full assets, or arbitrary `Record<string, unknown>` as the ordinary application contract. An
explicitly bounded extension payload is a separate case, validated using its registered component schema.

Keep current wire `snake_case` and enum tags during migration. The existing object facade's `sceneId` and
`expectedRevision` are TS convenience names, not permission to recursively rename every response. Adapt only at the
client boundary where semantics differ. Tauri command argument casing and nested Serde field casing are distinct;
test both with actual invokes. Do not stringify a request before passing it to a typed command or return a JSON string
where the endpoint declares a structured DTO.

## Rust-to-TypeScript generation

Start with a proof using **ts-rs** on the public DTO graph. It exports TS declarations without coupling the domain to a
specific frontend transport. The researched release is 12.0.1 with Rust 1.88 minimum; the editor host declares Rust
1.89. Compatibility with all workspace targets remains a build check. Serde compatibility is partial: custom
serializers and unsupported attributes require explicit treatment. Its default large-integer TS type is `bigint`,
which does not change Serde JSON encoding. [ts-rs documentation](https://docs.rs/ts-rs/12.0.1/ts_rs/).

| Candidate                           | Assessment for this repository                                                                                                                                                                          |
|-------------------------------------|---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| ts-rs                               | Preferred first proof: DTO exports plus a small typed `EditorClient`; test command names/casing separately.                                                                                             |
| Specta + Tauri Specta               | Can also generate command/event bindings. Attractive if wrapper drift becomes significant, but the researched Tauri integration uses release candidates that must be pinned as a compatible set.        |
| Rust JSON Schema + TS generation    | Useful where dynamic plugin schemas and runtime validators are central; schema-to-TS tooling must preserve tuples, unions, and nullable/default semantics. More moving parts for an initial static API. |
| Handwritten Rust and TS models      | Current approach; keep only while migrating because both sides can drift independently.                                                                                                                 |
| Protobuf schema as primary contract | Reasonable for a future multi-language/remote service; changes schema ownership and adds a toolchain without a demonstrated local IPC need.                                                             |

At research time, Tauri Specta's guide specifies matching `specta` and `tauri-specta` `=2.0.0-rc.27` with
`specta-typescript` 0.0.14. Treat that as a dated candidate matrix, not a floating install instruction or a proven
workspace integration. [Tauri Specta guide](https://specta.dev/docs/tauri-specta).

Proposed export workflow:

1. Export only the public contract graph. Reuse existing domain types where their exact serialized shape is suitable;
   introduce boundary DTOs when numeric encoding, versioning, or data selection differs.
2. Put optional generation derives behind a `bindings` feature so normal engine/player builds need no generator.
   Do not put Angular decorators, signals, or form types in generated contracts.
3. Generate into one dedicated directory, for example `apps/editor/src/app/core/bridge/generated/`, rather than into
   component folders or the design guide. UI view models remain handwritten beside their features.
4. Pin generator versions/configuration. Regenerate deterministically in CI and fail on differences or extra/missing
   exports. Do not suppress all unsupported-Serde warnings to obtain a clean build.
5. A generator annotation that says a field is a string must have matching Rust serialization; an annotation alone
   cannot fix a numeric wire representation. Rust writes canonical JSON fixtures; TS validates/consumes them. TS writes
   valid and invalid request fixtures; Rust deserializes and domain-validates them. Test actual serialized values,
   not only successful TS compilation.
6. Split request and response DTOs when Serde accepts omitted defaults but emits populated fields. One convenient TS
   type must not accidentally make required output fields optional or reject valid input omission.

Generated declarations disappear during TypeScript compilation. `invoke<MyDto>()` and `as MyDto` do not validate an
incoming
value. [TypeScript assertions](https://www.typescriptlang.org/docs/handbook/2/everyday-types.html#type-assertions).

Validate received data once at the client boundary, then keep typed immutable state inside the UI. Rust validates all
incoming commands, references, ranges, and cross-field rules regardless of frontend validation. Build-time generation
of validators is preferable to constructing a second schema repeatedly in Angular components.

For a larger DTO surface, evaluate Schemars-generated JSON Schema with Ajv standalone validators in the same proof.
Schemars derives schemas from Rust; Ajv can emit validation functions ahead of time. This can avoid shipping a schema
compiler and runtime `Function` construction, but generated code and helper imports still have size costs. Use strict
non-coercing validation and measure the resulting bundle. Keep these tools out of the initial dependency set until the
proof passes; focused guards plus cross-language fixtures are sufficient for the first narrow client.
[Schemars](https://docs.rs/schemars/latest/schemars/), [Ajv standalone](https://ajv.js.org/standalone.html).

## Wire rules that prevent subtle corruption

| Concern        | Required contract                                                                                                                            |
|----------------|----------------------------------------------------------------------------------------------------------------------------------------------|
| Stable IDs     | Opaque strings; never array indexes, object names, pointers, or transient renderer handles.                                                  |
| `u64`/`i64`    | New protocol counters use canonical decimal strings and checked Rust parsing. Never infer safety from a TS `number` declaration.             |
| Small integers | Use explicit bounded `u32`/`i32` DTO fields where appropriate; avoid architecture-dependent `usize` as a new public schema.                  |
| Floating point | Finite values only. Validate domain bounds before narrowing to `f32`; expect backend normalization/rounding and display accepted values.     |
| Arrays/vectors | Fixed-length tuples for mathematical values; bounded lists for collections. Validate lengths and total bytes.                                |
| Enums          | Explicit discriminant with stable wire names, matching Rust Serde tags and TS discriminated unions.                                          |
| Maps           | String-keyed records or arrays of entries. Define ordering separately; no JSON object-key ordering dependency.                               |
| Time           | State the unit and clock domain; duration fields use named units. Backend sequence establishes order, not wall time.                         |
| Assets         | Stable references and metadata. Resolve host paths in Rust; do not ship file contents inside object records.                                 |
| Defaults       | Rust supplies authored defaults on creation/migration. Inspector defaults describe these values; they do not reset existing state on render. |

JSON has no representation for NaN or infinity, and interoperable integer precision is limited around the binary64
safe-integer range. The schema must define behavior at that boundary, independent of the serializer's acceptance of
larger literals. [RFC 8259, numbers](https://www.rfc-editor.org/rfc/rfc8259#section-6).

For the existing protocol 2 endpoints, retain numeric encoding and add checks on both ends before relying on it:
reject values outside `0..9007199254740991` before native serialization or mutation. A frontend check after parsing
cannot recover precision already lost. A new protocol uses strings consistently for revisions, cursors, ticks, and
workspace generations. Use separate semantic types even if they share encoding. TS may parse a string with `BigInt`
for local comparison, but must send its canonical string form back. Lexicographic string order is not numeric order.

### Missing, null, and patch operations

The object stub's `Partial` patch currently distinguishes omission from explicit `null`: omission means unchanged,
while `null` can remove a material/collider. A naive Rust `Option<T>` patch loses that distinction. Even a plain nested
`Option<Option<T>>` needs a suitable deserializer to preserve it. The documented `serde_with::rust::double_option`
adapter illustrates the three
states. [Serde double-option adapter](https://docs.rs/serde_with/latest/serde_with/rust/double_option/index.html).

Prefer explicit typed commands for common nullable changes, such as `set_material` with a value or `null`, and explicit
`add_component`/`remove_component`. If a multi-field patch is introduced, use a tested tri-state encoding or a tagged
operation list. Do not implement it by blindly merging incoming JSON into engine memory. An atomic patch must validate
all operations first and produce one transaction/history result; partial-success batches need a different contract.

Reject unknown command variants and unknown fields in mutation requests. Additive response evolution can permit
unknown fields only under the negotiated protocol policy; a changed enum meaning is not an additive field. Unknown
plugin data may be retained for read-only diagnosis or a separately specified lossless migration workflow under
ADR-0002. This does not authorize editing or executing unsupported components.

Serde internally/adjacently tagged enums provide explicit variant selection. Avoid untagged command unions, which try
variants in sequence. Avoid combining flattened structures with strict unknown-field rejection, a combination Serde
does not support. [Serde enum representations](https://serde.rs/enum-representations.html),
[Serde flatten restrictions](https://serde.rs/attr-flatten.html).

## Spatial transforms and UI transformations

Two different transformations are involved: converting engine data to an inspector view model, and changing an
object's position/rotation/scale. They must not share a vague catch-all serializer.

The current `TransformDto` carries three arrays of three `f32` values. `engine-core/src/transform.rs` evaluates rotation
with `glam::EulerRot::XYZ`. Preserve that contract during the bridge migration:

```ts
// Illustrative generated wire shape; the existing Rust TransformDto stays authoritative.
type Vec3Dto = readonly [number, number, number];

interface TransformDto {
    readonly position: Vec3Dto;
    readonly rotation: Vec3Dto; // XYZ Euler angles in radians
    readonly scale: Vec3Dto;
}

interface TransformEditDto {
    readonly key: string;
    readonly transform: TransformDto;
}
```

| Representation           | Responsibility                                                                                                              |
|--------------------------|-----------------------------------------------------------------------------------------------------------------------------|
| Authored local transform | Persisted relative to the parent; root objects are relative to the scene.                                                   |
| World transform query    | Backend-computed for tools that need it; include revision and space. Do not persist a second authoritative world transform. |
| Inspector draft          | Can show degrees and separate X/Y/Z controls; converts radians/degrees once at the feature adapter.                         |
| Transform command        | Explicit `local` or `world` space and complete replacement values, with transaction/revision context.                       |
| Runtime matrix/pose      | Computed in Rust; matrix layout and animation poses are not part of ordinary object inspection.                             |

Document the engine coordinate basis and scene length units before exposing physical measurements; this ADR does not
silently reinterpret existing coordinates as meters. Reparent-with-world-preservation must reject unsupported or
singular parent transforms and handle non-uniform scale/shear explicitly. It must not pretend every matrix can be
represented losslessly as local TRS.

Quaternions are a sensible future runtime/animation representation. If authored rotations change, introduce a
versioned `rotation_quat_xyzw` contract, normalization rules, migration tests, and an inspector Euler view. Do not
replace a three-element `rotation` array with four elements under the same schema. Choose quaternion sign continuity
for interpolation without relying on component-wise equality of equivalent rotations.

### Illustrative command in the next protocol

The following is a proposal, not a request accepted by today's endpoints. Version 3 is illustrative and must be
allocated during implementation. It intentionally keeps the existing transform payload semantics:

```json
{
  "protocol_version": 3,
  "request_id": "request-17",
  "workspace_generation": "4",
  "scene_id": "main",
  "expected_revision": "42",
  "transaction_id": null,
  "command": {
    "type": "set_transforms",
    "edits": [
      {
        "key": "object-7",
        "transform": {
          "position": [
            1,
            2,
            3
          ],
          "rotation": [
            0,
            0,
            0
          ],
          "scale": [
            1,
            1,
            1
          ]
        }
      }
    ],
    "space": "local",
    "snap": false
  }
}
```

A successful result is a small receipt containing correlation, authoritative revisions, affected objects/scenes,
change categories, and a cursor. It does not need to echo every object. A rejection carries a stable code and
structured field/reference details. Timeout or transport failure is an unknown outcome, not permission to retry a
mutation. ADR-0001 owns ordering, transaction, deduplication, and recovery requirements.

## Transfer definitions once, values when needed

Component definitions are relatively static. Object values change frequently. Keep them separate:

1. Handshake obtains protocol version, supported commands, component catalog revision, and negotiated size limits.
2. Fetch the component descriptors once per catalog revision. Cache by workspace/module catalog identity; invalidate
   when a module reload changes a schema. Avoid repeating labels, defaults, enum options, and property metadata per
   object.
3. Query hierarchy summaries in bounded pages. A page cursor must belong to a stable revision or explicitly require a
   restart when the underlying result changes; pagination alone does not make a consistent snapshot.
4. Query details only for selected/inspected objects. Subscribe to that working set and any separately visible graph
   or diagnostics view. Use explicit bounded query projections, not arbitrary field expressions.
5. A UI adapter maps descriptors plus typed values to Signal Forms and subjects. Widget choices, collapse state,
   theme tokens, icons, and layout remain presentation concerns. Descriptor semantic types/units/constraints guide
   controls but do not grant execution or replace backend validation.
6. Submit typed commands on accepted form edits. Retain invalid/incomplete text in a local draft; do not send every
   intermediate keystroke. Acknowledged values replace the draft baseline without clobbering a newer edit.

Normalized stores index by stable object/component identity and update affected records. Avoid deep-cloning the
entire scene, reconstructing every form, or rebuilding all graph nodes after a single scalar edit. This is a proposed
UI update policy, not a claim that normalization by itself makes every Angular update constant-time.

## Traffic and flow control

| Workload                                           | Initial route                         | Delivery policy                                                                                |
|----------------------------------------------------|---------------------------------------|------------------------------------------------------------------------------------------------|
| Rename, create, delete, attach/remove component    | Typed invoke + receipt                | Ordered, bounded mutation queue; never silently dropped.                                       |
| Continuous transform drag                          | Coalesced typed invoke                | One active transaction; keep latest unsent absolute edit per target; flush before commit.      |
| Initial open/recovery                              | Consistent snapshot + cursor          | Bounded baseline; revision-consistent paging for large scenes.                                 |
| Authoritative changes                              | Wakeup + cursor query                 | Drain in sequence; missed wakeups recover through querying.                                    |
| Selected-object/live preview values                | Subscribed channel if needed          | Latest complete state may replace older samples; merge partial updates by key before dropping. |
| Build/import progress                              | Small channel messages + status query | Throttle progress; preserve terminal state in the backend.                                     |
| Logs/profiling                                     | Bounded stream                        | Explicit dropped-count markers and pause/unsubscribe controls.                                 |
| Thumbnail/waveform/debug geometry                  | Raw response or bounded byte stream   | Demand-driven; cancel/release by subscription/resource ID.                                     |
| Simulation, skeletal pose evaluation, Vulkan frame | Stay native                           | No per-frame serialization of the complete engine world.                                       |

The initial receipt migration still needs snapshots when an invalidation affects displayed values. To reduce those
copies further, implement targeted queries or value-bearing deltas as a separate tested extension. Do not treat the
existing `WorldChange` keys as if they already carried new component values.

Use a snapshot-and-cursor captured consistently by the backend. Never obtain each independently and assume they
describe the same state. Apply deltas only to their declared base/session; a gap or a replaced workspace triggers
resynchronization. A receipt cursor is not proof that prior journal entries have reached the UI.

Proposed scheduling starting points for measurement are at most one outstanding gesture mutation, 30 updates/second
for a visible inspector preview, and 10 updates/second for progress counters. These are tunable product policies,
not Tauri limits or measured optima. Native simulation runs independently. Queue capacity and byte limits must be
explicit even when rate limiting is enabled.

For channels, bound producer queues and bytes in flight. Use acknowledged batches/credits for non-droppable data, or
a pull-next-chunk design for bulk reads. For disposable preview state, keep a bounded latest-value cache. A
`Channel::send` success must not be treated as confirmation that Angular applied the message. Process ordered stream
handlers serially when asynchronous work is involved. Stop production and release Rust-held channel clones when the
view closes, workspace changes, consumer cancels, or delivery fails.

Inspection of the pinned Tauri channel implementation shows size-dependent delivery paths, including JSON conversion
for small raw payloads and a queued fetch path for larger data. It supplies no application-level consumption receipt
in `send`. Therefore this design does not claim universal zero-copy transfer or automatic bounded backpressure.
[Tauri 2.11.5 channel source](https://github.com/tauri-apps/tauri/blob/tauri-v2.11.5/crates/tauri/src/ipc/channel.rs).

### Native scheduling and UI work

Keep CPU-heavy conversion/import/serialization off the host UI thread. An async function alone does not make
CPU-bound work nonblocking. Use a bounded worker pool or controlled blocking tasks, capture a consistent owned result
under the workspace lock, and release the lock before lengthy encoding or delivery. Avoid a task per scalar field.
Respect existing workspace/viewport lock ordering. A long-lived stream belongs on a managed producer, not a blocking
worker retained forever.

Tokio warns that blocking tasks need concurrency limits for CPU work and cannot generally be aborted once running.
Cancellation must be cooperative and distinct from merely discarding a frontend promise.
[Tokio blocking-task guidance](https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html).

If decoding or visual preparation is expensive in the WebView, evaluate a Web Worker. Transfer the `ArrayBuffer`
between JS contexts instead of cloning it where ownership permits; the sender loses access to a transferred buffer.
This optimization is within the WebView and is not proof of zero-copy Rust IPC. Avoid transferring a buffer still
owned by a store or another
renderer. [Transferable objects](https://developer.mozilla.org/en-US/docs/Web/API/Web_Workers_API/Transferable_objects).

## Binary path requirements

### Format comparison

These are workload-based recommendations from the research, not a measured speed ranking.

| Format                 | Strength and cost                                                                                                                                                                   | Decision                                                                                                                                                                                                                                                   |
|------------------------|-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| JSON                   | Matches the existing Serde/Tauri contract and native JavaScript parsing; readable fixtures. Text size and allocation can become expensive for bulk values.                          | Default commands, queries, descriptors, receipts, and low-volume streams.                                                                                                                                                                                  |
| XML                    | A document/markup format with its own parsing and mapping model. This editor has no mixed-content or XML integration requirement.                                                   | Do not add it to IPC. Rejection is based on fit and complexity, not a claim that XML is universally slower. [XML specification](https://www.w3.org/TR/xml/)                                                                                                |
| MessagePack            | General binary structured data with Rust and JS implementations. Must specify named maps versus positional structs, bytes, and 64-bit integers. Adds a JS codec.                    | First general-purpose codec candidate if large structured JSON is a measured problem. [Rust named-map encoder](https://docs.rs/rmp-serde/latest/rmp_serde/encode/fn.to_vec_named.html), [JS implementation](https://github.com/msgpack/msgpack-javascript) |
| CBOR                   | Standardized byte strings, numeric representations, maps, and extension tags. Interoperability needs an agreed profile for tags/map keys and deterministic encoding where required. | Reconsider if a standards-based integration requires it; no current requirement favors another binary object codec. [RFC 8949](https://www.rfc-editor.org/rfc/rfc8949.html)                                                                                |
| Postcard               | Compact schema-dependent Serde format; does not carry struct field names and leaves schema evolution to applications.                                                               | Better fit for constrained Rust-oriented protocols than this evolving editor contract. A TS decoder/schema toolchain would need separate proof. [Postcard specification](https://postcard.jamesmunns.com/wire-format)                                      |
| Protobuf               | Explicit schema, field numbering, evolution rules, and generated bindings. Adds an IDL/toolchain and language-runtime choices; field presence requires care.                        | Revisit for a durable remote or multi-language engine service. Do not introduce it solely to replace local JSON. [Protobuf guide](https://protobuf.dev/programming-guides/proto3/)                                                                         |
| FlatBuffers            | Generated buffer accessors can avoid eagerly unpacking a large read-heavy payload. Requires an `.fbs` schema; converting through the object API adds allocation again.              | Candidate for large stable tables or debug data after profiling; direct access does not eliminate native/WebView transfer copies. [FlatBuffers TypeScript](https://flatbuffers.dev/languages/typescript/)                                                  |
| Explicit packed arrays | Small specialized header plus dense numeric values; needs exact layout, bounds, identity mapping, and lifecycle rules.                                                              | Preferred focused experiment for dense transforms/waveforms rather than inventing a generic binary object system.                                                                                                                                          |

Do not swap serializers under the same endpoint and assume the Serde DTO graph, enum representation, integer policy,
and TS decoder remain compatible. In particular, the JS MessagePack implementation makes 64-bit bigint behavior an
explicit option, and its own benchmark notes recommend testing the actual workload; native JSON decoding can win.
[MessagePack options and benchmarks](https://github.com/msgpack/msgpack-javascript#benchmark).

WebSockets, HTTP, or a sidecar do not remove serialization and introduce additional transport/lifecycle work. They
belong to a separately scoped remote-editor requirement. WebAssembly can be useful for isolated computation in the
browser, but does not expose the existing native Rust/Vulkan process's memory. Shared-memory/native interop would
need a separate platform-specific ownership, synchronization, and recovery design; it is not an initial shortcut.

For bulk TS-to-Rust data, pass a raw `Uint8Array`/`ArrayBuffer` to the command and decode `InvokeBody::Raw`. For
Rust-to-TS, use `tauri::ipc::Response` for raw bytes; stream raw response bodies only after proving that exact path
against the installed versions. Returning an ordinary serialized `Vec<u8>` or embedding bytes in a JSON DTO can
produce arrays of numbers. A generic `invoke<ArrayBuffer>` annotation does not change transport behavior.
[Tauri raw request/response documentation](https://v2.tauri.app/develop/calling-rust/).

Do not wrap a binary codec in base64 JSON just to fit the existing DTO. Prefer an explicit raw endpoint. A future
binary stream must define:

- Format magic/version, byte order, header/payload lengths, bounded count/stride, and numeric representation.
- Workspace/scene/stream generation, sequence, sampling timestamp, and full-frame versus delta semantics.
- A stable mapping from numeric stream indexes to object keys, with its own generation; never reuse indexes across
  topology changes without updating that mapping.
- Ownership, buffer release, cancellation, size limits, malformed-input rejection, and stream end/error messages.
- Precision policy: use `f32` arrays where they match the engine; do not add quantization without an error budget.

For illustration, 1,024 transforms with nine `f32` values contain 36,864 numeric payload bytes before headers/IDs.
That calculation says nothing about IPC copies, decode cost, or total frame time. Dense arrays can help a measured
visualization workload; ordinary inspector edits still benefit from named JSON fields.

Compression is off by default for small local messages. If a measured large transfer warrants it, record CPU cost,
latency, compressed size, and decompressed-size limits. Already-compressed thumbnails/assets should be evaluated
separately. Retain a JSON fallback for capability mismatches and diagnostics.

## Measurement and acceptance plan

No repository-specific Rust–WebView benchmark was run for this ADR. Codec claims in upstream documentation are
capabilities or their authors' measurements, not results for this engine. The following experiment is required before
adding a binary codec to the production editing path.

| Fixture/workload                               | Variants to compare                                                           | What it reveals                                                           |
|------------------------------------------------|-------------------------------------------------------------------------------|---------------------------------------------------------------------------|
| Rename and one-object transform                | Current snapshot reply; JSON receipt + refresh; JSON receipt + targeted query | Cost of sending less state and avoiding unnecessary reads.                |
| 1, 100, and 1,024 transform edits              | Per-object calls; one bounded batch; coalesced gesture samples                | IPC-call overhead, batching benefit, and queue latency.                   |
| 1k/10k object hierarchy                        | Full details; summary pages; visible subtree                                  | State volume, frontend allocation, and first usable display time.         |
| Repeated preview frames                        | JSON channel; raw typed arrays; selected codec candidate                      | Encode/decode cost, sustained throughput, memory, and main-thread stalls. |
| Thumbnail/debug payloads: 64 KiB, 1 MiB, 8 MiB | Raw response; bounded chunks; scoped asset URL where appropriate              | Peak memory and transfer overhead.                                        |
| Slow/hidden/closed consumer                    | Every stream mode                                                             | Backpressure, cleanup, stale data rejection, and bounded retention.       |

Measure end-to-end latency from UI submission to accepted state and separately to visible presentation. Report
p50/p95/p99, bytes/message, calls/second, Rust encode/decode/validation time, queue wait, UI parse/update time,
allocation
or heap growth, dropped/replaced samples, and added production bundle size. Measure each process with its own
monotonic clock; do not subtract uncalibrated Rust and browser timestamps.

Use release/native builds with pinned dependencies and recorded OS, hardware, WebView version, fixture hashes, warmup,
repetitions, and dispersion. Include cold start and sustained load. Keep rendering/validation semantics equal across
comparisons. Node-only tests and Rust microbenchmarks cannot replace a native Tauri/WebView2 run; additional supported
platforms need their own runs. Criterion can isolate Rust codec changes, while the desktop harness measures the full
interaction. [Criterion methodology/tooling](https://bheisler.github.io/criterion.rs/book/).

Adopt an alternative only if it improves a defined workload's latency or memory budget without regressing normal
editing, compatibility, correctness, or bundle/startup costs. Establish product budgets on representative hardware
first; no universal size threshold or invented throughput number is asserted here.

Correctness acceptance includes bidirectional fixtures for every command variant, integer boundaries, null/omission,
tuple lengths, Unicode, finite/overflow values, defaults, missing/unknown fields, malformed binary lengths, stale
references, revision conflicts, mixed protocol versions, cursor gaps, late responses, channel cleanup, and slow
consumers. Fixtures must prove that a committed edit is not retried after a later UI refresh failure.

## Implementation sequence

1. Baseline the current snapshot path and preserve existing command tests. Extract `EditorClient` behind dependency
   injection; the design guide can use a deterministic in-memory adapter without a native runtime.
2. Prove generation against representative DTOs and bidirectional fixtures. Generate TS types and add boundary
   validation, explicit numeric rules, structured errors, and drift checks. No wire-format changes hidden in this step.
3. Add the negotiated envelope/baseline protocol from ADR-0001. Migrate to receipts, recoverable cursor consumption,
   and coalesced transactions. Keep protocol 2 behind an explicit compatibility adapter until consumers migrate.
4. Add hierarchy summaries, selected-object details, component catalog caching, and targeted queries. Map `ObjectApi`
   to these commands/queries without constructing a second undo or revision authority. Its current `afterRevision`
   is not interchangeable with a journal cursor; replace/extend the watch resume contract deliberately.
5. Add managed channels for actual subscribed progress/preview needs. Verify memory bounds and cancellation before
   increasing stream rates. Keep authoring changes separate from disposable runtime preview values.
6. Run the comparison matrix. Add a versioned binary endpoint only where the measured results justify maintenance
   of a second representation. Benchmark again after WebView/Tauri/codec upgrades.

An initial layout may put protocol adapters/export tooling in `crates/bridge`, host endpoints in
`apps/editor-host/src-tauri/src/commands`, and generated/client files under `apps/editor/src/app/core/bridge`.
Feature-specific form mapping stays with its feature. None of these proposed files or dependencies is created by
this documentation change.

## Consequences and limits

The editor gains a small, explicit boundary with less duplicated schema and a path to lower traffic. Rust retains
control of object validation, simulation, transactions, and asset/resource lifetimes. The design also keeps browser
previews independent of native execution.

Costs include export tooling, compatibility fixtures, state synchronization, and explicit stream lifecycle logic.
JSON still allocates and serializes; binary still has transfer, validation, and decoding costs. Generated types do
not provide runtime safety, and neither a channel nor a format change guarantees responsiveness. Performance must be
demonstrated with the workload and installed WebView the user actually runs.

Documentation-site checks verify publication and links. They do not validate a generator integration, native IPC
latency, cross-platform behavior, object migrations, or runtime/GPU correctness.
