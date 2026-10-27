# ADR-0010: Input actions, focus contexts, and possession

Status: Proposed; current keyboard and movement commands remain the implemented baseline.

Date: 2026-10-27

Scope: Logical gameplay actions, device routing, editor focus, player/AI control, and controlled-object transitions.

Dependencies: [ADR-0002](0002-object-composition-and-components.md) defines composable controller capabilities;
[ADR-0003](0003-rust-typescript-bridge.md) defines wire constraints;
[ADR-0004](0004-authoring-play-and-editor-state.md) defines session ownership;
[ADR-0005](0005-runtime-lifecycle-and-scheduling.md) defines input consumption and movement authority. Coordinate native
focus with [ADR-0012](0012-native-viewport-composition.md), bindings persistence with
[ADR-0011](0011-persistence-autosave-and-recovery.md), and module consumers with
[ADR-0008](0008-game-modules-and-scripting.md).

## Context and example

The W key should move a player while the Play viewport owns gameplay focus. It should type a character into an inspector
field while that field has focus. If a dialog opens while W is held, the player must stop even when the native child
window
never receives the release. Entering a vehicle should redirect control deliberately, without a delayed input frame
moving
the previously controlled character.

Device bindings, editor shortcuts, player identity, and the controlled object therefore need separate contracts.
“Player”
is a control relationship, not a mandatory subclass of every controllable scene object.

## Existing implementation inspected

The baseline is `fcb748b04853209b5e17e680c38c847c19f7f365` plus the working tree inspected on 2026-10-27.
Concurrent simulation scene-targeting changes were preserved and were not build-validated by this documentation pass.

| Source                                                         | Observed behavior                                                                                                                                                   |
|----------------------------------------------------------------|---------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `crates/engine-core/src/input.rs`                              | A fixed keyboard enum and held/pressed/released/suppressed sets. Suppression requires release before a held key can activate again.                                 |
| `crates/engine-core/src/command.rs`                            | `SetPlayInput` supplies movement broadly; `SetEntityInput` targets one entity. There is no general logical-action protocol.                                         |
| `crates/engine-core/src/simulation_session.rs`                 | Input is a normalized movement vector per configured controlled entity. The broad setter targets all configured controlled-movement behaviors in the runtime scene. |
| `apps/editor-host/src-tauri/src/viewport.rs`                   | Native viewport events are forwarded to Angular. Native lifecycle paths clear Play input on focus/visibility/detachment changes.                                    |
| `apps/editor/src/app/core/editor/engine-viewport.directive.ts` | The directive manages viewport bounds/visibility and forwards input to its feature owner; it does not define gameplay bindings.                                     |

There is no implemented device-pairing, general gamepad action map, or possession system established by these sources.

## Research rationale

Unity's input documentation separates actions from physical bindings, allowing one gameplay action to use different
device controls. That separation is useful here.
[Unity action workflow](https://github.com/Unity-Technologies/InputSystem/blob/develop/Packages/com.unity.inputsystem/Documentation~/using-actions-workflow.md).

Godot explicitly provides input handling in which GUI consumption precedes unhandled gameplay input. This supports a
focus/context gate; it does not solve routing between our WebView and native child window automatically.
[Godot input handling](https://docs.godotengine.org/en/stable/tutorials/inputs/inputevent.html).

## Proposed decision

### Logical actions and bindings

Define stable action identifiers such as `game.move`, `game.look`, `game.jump`, and `game.interact`. Each definition has
a value shape: button, scalar axis, or two/three-axis vector. Bindings map physical controls to actions, including
explicit
dead zones, sensitivity, inversion, and supported interaction behavior. Rename display labels without changing identity.

Project defaults are authored input configuration. User rebinding is a validated override with reset behavior and its
own
persistence scope. Editor shortcuts use a separate namespace and context; rebinding movement cannot silently rebind
scene
Delete or Undo. Reject incompatible value shapes and ambiguous ownership rather than accepting arbitrary JSON callbacks.

Gameplay action resolution should be reusable Rust logic with thin platform adapters, independent of Angular rendering.
Headless tests can inject logical action frames. Editor shortcuts remain UI/tool logic. This is a proposed migration
from
the current native-event-to-Angular flow: introduce it only after proving focus and routing behavior in the native host.
No device library or binary transport is chosen by this ADR.

### Context and device ownership

Route each physical event through one owner for its active context. Do not deliver the same movement from both DOM and
native listeners. Model contexts explicitly: modal/text input, editor tools, and gameplay. A focused text control or
modal
consumes its applicable controls before gameplay. Editor emergency commands such as stopping Play remain deliberately
available where specified; they are not accidental propagation through every listener.

The UI requests context changes; the host validates them against native focus, visibility, and Play mode. A context
epoch
invalidates queued input from the previous owner. Native focus loss, hidden/detached viewport, device disconnect, pause,
Stop, and capture cancellation all neutralize gameplay state without depending on a browser release event. Reacquisition
does not synthesize presses for keys still held; require release/rearm according to the action's policy.

Pair devices with a local player identity explicitly. One device/control stream has one gameplay owner unless a declared
shared-device binding partitions it. Start with one local player while retaining that identity in internal contracts.
Multiple local players, device claiming UI, and accessibility combinations require later proofs.

### Sampling and bounded delivery

Distinguish held values, ordered button edges, and relative deltas. Coalesce superseded absolute axis samples where
safe;
preserve press/release order and accumulate relative motion rather than keeping only its last sample. Button transitions
must survive a press and release between simulation steps. Bound queues and define overflow as
cancellation/neutralization
with a diagnostic, not a permanently held action.

Consume edges once at the next eligible fixed step; catch-up steps reuse held state without repeating a jump press.
Tag frames with runtime generation, context epoch, player identity, possession generation, and ordering information.
Reject stale or duplicate delivery. These are proposed fields; their wire representation follows ADR-0003 rather than
silently changing current commands. Gameplay input is transient and does not enter authored undo history or autosave.

### Controllers and possession

Separate these roles:

| Role               | Responsibility                                                   |
|--------------------|------------------------------------------------------------------|
| Local player       | Owns device assignments and input preferences.                   |
| Controller         | Converts player actions or AI decisions into domain intent.      |
| Possessed object   | Receives that controller's intent through declared capabilities. |
| Movement authority | Resolves actual movement/collision under ADR-0005.               |

A character and a vehicle expose different intent capabilities; possession validates compatibility and same-world
identity. Transfer control atomically at a simulation boundary: neutralize the old target, detach its controller,
increment
possession generation, attach the new target, and begin from neutral input. A stale frame cannot target either object
through an obsolete association. Destruction or disabling the target unpossesses it safely.

AI and player controllers use the same domain intent receiver, with one primary controller per capability initially.
Switching AI to player control is an explicit transfer. Blended assistance requires a declared arbitration policy later;
it must not become two competing transform writers. Camera following is an associated policy, not proof of possession.
Network authority and remote-player prediction remain separate future decisions.

## Alternatives, open choices, and sequence

Hard-coded key checks inside each object are cheap initially but duplicate rebinding, device, and focus rules. Keeping
all
gameplay mapping in Angular simplifies the current editor but couples installed/headless execution to the UI. Sending
raw
events to every subsystem creates duplicate actions and inconsistent cancellation. The proposed shared action layer adds
an explicit routing seam and migration work in exchange for consistent semantics.

Open choices include device backend support, physical-key versus layout-aware bindings, dead-zone defaults, interaction
types, and user-facing rebinding conflicts. Implement action/value contracts and tests first, then a single-player
adapter
and native focus proof, then possession, then rebinding and additional devices. Profile input latency before changing
the
transport or claiming performance gains.

## Acceptance and verification limits

- A quick press/release is observed once; catch-up steps cannot repeat its edge or discard accumulated look motion.
- Opening a text field/dialog while moving, alt-tabbing, hiding the viewport, and unplugging a device all stop movement.
- Native and DOM delivery cannot double an action. Reacquiring focus does not resume a previously held movement key.
- Rapid possession changes, target deletion, and delayed old frames cannot control the wrong object.
- AI/player transfer uses the same intent contract; movement still passes through the configured movement authority.
- Default bindings, overrides, reset, and headless input fixtures agree on action identity and value validation.

These gates require unit coverage plus native Windows focus/device checks. This documentation pass did not implement an
action mapper, exercise gamepads, or measure end-to-end input latency.
