# ADR-0002: Scene objects, components, presets, and inspector subjects

Status: Proposed architectural direction; no object-model migration is implemented by this ADR.

Date: 2026-10-27

Scope: Engine authoring model, runtime composition, object APIs, reusable assets, and Angular inspectors.

Related decisions: [ADR-0001: Editor command pipeline](0001-editor-command-pipeline.md) and
[ADR-0003: Rust–TypeScript bridge](0003-rust-typescript-bridge.md).

## Context

The editor needs a common object definition that can represent scenery, a moving prop, a character, a player avatar,
an NPC, a camera, a light, and an audio source. Objects can combine these abilities: a vehicle can be controlled by a
player, emit sound, carry lights, and run an animation graph. A class tree such as `Object -> LiveObject ->
ControllableObject -> Player` makes those combinations difficult and gives unrelated concepts the same inheritance axis.

There are also two meanings of “children”: specialized object definitions and objects beneath another object in the
scene hierarchy. They must be modeled separately. The former become creation presets; the latter remain parent-child
relationships between object instances.

### Existing implementation inspected

The source baseline is commit `a377b1f87dcc2972851c1518e4fd3cf1dc1e8951`, plus the staged, unstaged, and untracked
working tree inspected on the date above. The following distinctions matter for migration:

| Source                                                   | Current behavior                                                                                                                                                                                                                   |
|----------------------------------------------------------|------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `crates/engine-core/src/scene_data.rs`                   | Scene schema 3 stores `EntityData` with identity, name, model path, transform, renderability, optional parent/material/collider/mesh index. Camera and lights are separate scene fields. There is no general component collection. |
| `crates/engine-core/src/entity_key.rs`                   | `EntityKey` is a validated string; newly generated keys use UUIDs. Names are independent of identity.                                                                                                                              |
| `crates/engine-core/src/transform.rs`                    | Position, XYZ Euler rotation in radians, and scale become a matrix through `glam`.                                                                                                                                                 |
| `crates/engine-core/src/project.rs`                      | Project schema 2 stores scene-scoped `ControlledMove` and `Spin` behavior specifications.                                                                                                                                          |
| `crates/engine-core/src/simulation_session.rs`           | Game modules plan bounded actions against a read-only runtime world. The engine applies and validates mutations.                                                                                                                   |
| `apps/editor/src/app/core/editor/editor.models.ts`       | Handwritten TypeScript models mirror the existing scene and project formats.                                                                                                                                                       |
| `apps/editor/src/app/core/objects/`                      | Injectable object API stubs define CRUD, list, duplicate, reparent, and watch. Their default implementation rejects with `not_implemented`; it is not a native object adapter.                                                     |
| `apps/editor/src/app/core/subjects/`                     | Scoped inspector state defines Object, Transform, and Script subjects. These are UI models, not an engine component registry.                                                                                                      |
| `apps/editor/src/app/shared/components/graph/animation/` | Animation/locomotion graph authoring models exist in the UI. They do not establish runtime animation evaluation or native persistence.                                                                                             |

## Research and design rationale

Unity places functionality in components attached to a common GameObject. A light and an animated character therefore
share a container without needing the same specialized subclass. This supports the composition direction, although
our Rust implementation does not copy Unity's C# lifecycle or inheritance model.
[Unity GameObjects](https://docs.unity3d.com/6000.0/Documentation/Manual/GameObjects.html).

Rust supports structs and traits but does not provide struct/class inheritance. Common owned data plus explicit behavior
interfaces fit its model better than trying to reproduce a deep class tree.
[The Rust Book: object-oriented characteristics](https://doc.rust-lang.org/stable/book/ch18-01-what-is-oo.html).

Bevy demonstrates entities composed from data and systems that process selected component sets. Its documentation also
distinguishes iteration-oriented table storage from insertion-oriented sparse storage. This is a useful implementation
option, not evidence that this engine needs a new ECS dependency or that composition alone guarantees speed.
[Bevy ECS concepts](https://bevy.org/learn/quick-start/getting-started/ecs/),
[Bevy component storage](https://docs.rs/bevy_ecs/latest/bevy_ecs/component/trait.Component.html).

Reusable asset definitions must be separate from their scene instances. Godot's resources and scene instantiation offer
a useful example of that separation. Here, assets remain engine-owned records referenced by stable identity; importing
Godot's resource or scene formats is outside scope.
[Godot resources](https://docs.godotengine.org/en/stable/tutorials/scripting/resources.html).

## Proposed decision

Use one common **SceneObject** authoring record and attach typed **components**. Offer **presets** for common
combinations.
Keep **Object** as the editor label. Reserve **Subject** for the inspector card that edits an object or component.

The common base is a data contract rather than an inherited runtime class. This keeps the user's “everything is an
Object” model while allowing a character, camera, light, or script to be combined on the same instance. A runtime can
store components in separate typed arrays or an ECS later without changing the authored object contract.

```text
Scene
  SceneObject: identity + name + parent + local transform + enabled
    MeshRendererComponent
    ColliderComponent
    AnimationControllerComponent -> AnimationGraphAsset
    AudioSourceComponent         -> AudioClipAsset
    ScriptComponent              -> ScriptAsset + validated property values

ObjectPreset -> creates an initial SceneObject/component combination
PrefabAsset  -> reusable hierarchy/component definitions + explicit overrides
Runtime      -> resolves asset handles, evaluates systems, owns transient state
Inspector    -> shows Object/Transform subjects and component subjects
```

### Vocabulary and naming

| Term       | Meaning and naming rule                                                                                                                             | Examples                                                                   |
|------------|-----------------------------------------------------------------------------------------------------------------------------------------------------|----------------------------------------------------------------------------|
| Object     | User-facing name for a scene instance. Use `SceneObject` in public code to avoid confusion with JavaScript `Object` and Rust trait objects.         | A chair, camera rig, player avatar, or trigger volume.                     |
| Entity     | Runtime identity/storage concept. Keep existing `EntityKey` for stable scene identity during migration; avoid a mass rename merely for terminology. | Runtime entity handle mapped from a scene-scoped `ObjectRef`.              |
| Component  | Serializable attachment describing one capability and its authored settings. Use `*Component` when disambiguation is useful.                        | `CameraComponent`, `AudioSourceComponent`, `AnimationControllerComponent`. |
| Property   | A typed value inside a component or the object base. Use Property in domain APIs; the current UI term Trait can remain a display synonym.           | `intensity`, `gain`, `movement_speed`.                                     |
| Subject    | UI presentation and form state for a group of fields. It can map to one component, the object base, or several selected components.                 | Transform subject, Script subject, Animation subject.                      |
| Controller | Logic that translates goals, input, or graph state into actions. It is not the name of every attachment.                                            | Camera follow controller, animation controller, AI controller.             |
| System     | Runtime code that processes instances and owns execution order.                                                                                     | Transform propagation system, animation system, audio system.              |
| Asset      | Reusable definition or imported resource, independent of any one instance.                                                                          | Animation graph, audio clip, script definition, model, material.           |
| Preset     | A creation recipe that copies initial component values. Changing the preset does not silently change existing objects.                              | Static Mesh, Character, Player Avatar, Point Light.                        |
| Prefab     | Reusable object/hierarchy asset with deliberately managed instance overrides.                                                                       | Guard prefab with mesh, controller, footsteps, and equipment children.     |
| Role/tag   | Search or gameplay classification, not a substitute for a required component.                                                                       | `enemy`, `interactable`, `quest_target`.                                   |

Use `snake_case` field and variant names on the existing Rust/Serde wire surface. Keep PascalCase for Rust and
TypeScript
type names. Stable component type identifiers are namespaced strings, such as `engine.audio_source` or
`game.inventory`; they must not depend on a Rust module path, class display label, array position, or build-local type
ID.
The exact transport representation is governed by ADR-0003.

The graph asset and its attachment need distinct names. Proposed names are `AnimationGraphAsset` for reusable graph
data and `AnimationControllerComponent` for the graph reference and per-instance settings. Runtime playback state lives
in an animation instance. Unity similarly separates a controller asset containing state-machine/clip relationships from
the object that uses it; our names deliberately make the asset/instance distinction explicit.
[Unity Animator Controllers](https://docs.unity3d.com/6000.0/Documentation/Manual/class-AnimatorController.html).

Use `AudioSourceComponent` for sound playback and `AudioListenerComponent` for listening. Introduce
`AudioControllerComponent` only when there is actual stateful audio orchestration, such as an audio graph or cue rules;
do not require it just to play a clip. Unity's Audio Source is a playback component, which is a useful naming precedent.
[Unity Audio Source](https://docs.unity3d.com/6000.0/Documentation/Manual/class-AudioSource.html).

### Requested object categories

These are proposed presets and capabilities, not a promise that their runtime systems already exist.

| Requested category  | Recommended editor preset       | Components or settings                                                                      | What it does not imply                                                                                                           |
|---------------------|---------------------------------|---------------------------------------------------------------------------------------------|----------------------------------------------------------------------------------------------------------------------------------|
| Static object       | Static Mesh / Static Prop       | Mesh renderer, optional collider, mobility `static`.                                        | “Static” does not mean impossible to edit. An accepted editor transform invalidates affected cached/baked data.                  |
| Live object         | Actor / Dynamic Prop            | Appropriate movement, physics, animation, or script components; mobility `movable`.         | Movement, being enabled, and being alive are different concepts. A Health/LifeState component is optional for living characters. |
| Controllable object | Controllable Actor              | Control receiver plus a movement implementation such as a character motor or vehicle motor. | Control need not come from a local player; AI, scripts, and remote input can produce the same intents.                           |
| NPC object          | NPC Character                   | Character motor, AI controller, optional animation, perception, health, and audio.          | NPC status does not force a skeleton, rigid body, or human movement model.                                                       |
| Player              | Player Avatar                   | Control receiver, player binding, appropriate motor, optional animation/audio/camera rig.   | A player/session identity is not the avatar's object identity. Possession may change to a vehicle or another avatar.             |
| Camera object       | Camera                          | Camera component plus the base transform; optional follow/look controller.                  | The editor navigation camera is not automatically a saved game camera.                                                           |
| Light object        | Directional Light / Point Light | Light component with typed light-kind settings plus the base transform.                     | A light need not have a mesh, collider, or tick callback. Spot lights require a separate implemented capability.                 |
| Sound object        | Audio Source                    | Audio source component plus the base transform; optional controller.                        | Every sound need not be spatial, and clip bytes do not live in the object record.                                                |

Physics body mode (`static`, `kinematic`, `dynamic`) is separate from rendering/mobility policy. A moving platform can
be
kinematic; a loose crate can be dynamic; a nonphysical animated object can be movable. Avoid one `isStatic` boolean that
silently controls rendering, physics, navigation, and animation at once.

“Alive” should mean a gameplay state only when a game supplies life/health semantics. Generic scene objects do not all
pay for health, inventory, AI, audio, or per-frame update state.

### Base object, identity, and hierarchy

The base object carries only common authored data:

- Stable scene-scoped identity, editable name, and optional tags.
- One optional parent object reference within the same scene.
- Exactly one local transform, stored as a dedicated required field. Present it as a Transform subject; do not also
  insert a second transform into the generic component collection.
- Local enabled state, separate from renderer visibility. Effective enabled state is derived from the parent chain;
  disabling a parent suspends descendants according to documented system policies.
- A bounded collection of typed component instances. A preset identifier may be retained as creation metadata, but
  runtime behavior is determined by the components and values actually present.

Use the existing `ObjectRef { sceneId, key }` identity model for cross-boundary references. Add a stable `ComponentId`
for each attachment so two audio sources or scripts on the same object remain distinguishable after reordering. A
component's type ID identifies its schema; its instance ID identifies this attachment. Component references contain
the object reference plus component ID. Assets have their own identities; asset paths are locators, not instance IDs.

Keep runtime handles private and map them from persistent keys. An ECS handle can include a recycled index/generation
and need not be a stable disk format. Bevy explicitly warns that its entity representation is not a long-term wire
compatibility contract. This reinforces retaining the engine's persistent keys even if storage changes later.
[Bevy Entity stability warning](https://docs.rs/bevy_ecs/latest/bevy_ecs/entity/struct.Entity.html#stability-warning).

Parenting is a spatial/lifecycle relationship, not a class relationship. A camera can be the child of a vehicle, and a
light can be the child of a character's equipment object. Reject cycles, self-parenting, missing parents, and
cross-scene
parent references. Keep one authoritative parent field and derive child indexes; do not maintain conflicting serialized
`parent` and `children` relationships. If authored sibling order is needed, add an explicit order key with defined
insertion semantics rather than deriving identity from array order.

An attachment needing its own spatial offset can define one in its typed schema; a separately selectable transform,
hierarchy, or lifecycle should be a child object. A component does not become a child object merely because the
inspector
renders it in a separate card.

Duplicate/prefab instantiation remaps object and component IDs and all internal references together. External asset
references remain shared. External object references follow an explicit preserve/clear/error policy for that operation.
Deleting an object must validate references and subtree policy atomically; no partially removed hierarchy is accepted.

Keep the current local XYZ Euler/radian transform contract for initial migration. Converting the inspector's
`{ x, y, z }` form draft to a numeric tuple is a presentation adapter, not a new coordinate convention. Quaternion
authoring or playback support requires an explicit versioned contract and migration; do not reinterpret the existing
three-number rotation field. Existing keep-world reparenting already rejects decompositions that introduce unsupported
shear; preserve that behavior rather than silently losing information.

### Strict component definitions

Built-in attachments use Rust structs/enums with validated data. Export their wire shapes to TypeScript as a
discriminated union. Do not use an unrestricted property dictionary for built-in components, and do not serialize
Rust trait objects, function pointers, GPU handles, or arbitrary class instances.

The following is a small target contract sketch, not current compiled code. `TransformDtoV1` uses the existing position,
rotation, and scale tuples; `EntityKey`, `SceneId`, `ComponentId`, and `AssetId` are opaque serialized strings. The
numeric
limits and generated aliases belong in the bridge contract rather than being guessed independently by the UI.

```ts
interface SceneObjectDto {
    readonly key: EntityKey;
    readonly name: string;
    readonly parent: EntityKey | null;
    readonly transform: TransformDtoV1;
    readonly enabled: boolean;
    readonly components: readonly ComponentDto[];
}

type ComponentDto =
    | {
    readonly id: ComponentId;
    readonly type_id: 'engine.audio_source';
    readonly schema_version: 1;
    readonly enabled: boolean;
    readonly data: {
        readonly clip: AssetId | null;
        readonly gain: number;
        readonly loop: boolean;
        readonly spatial: boolean;
    };
}
    | {
    readonly id: ComponentId;
    readonly type_id: 'engine.animation_controller';
    readonly schema_version: 1;
    readonly enabled: boolean;
    readonly data: {
        readonly graph: AssetId | null;
        readonly playback_speed: number;
        readonly root_motion: 'ignore' | 'apply';
    };
};
```

The union extends with supported components; the sketch is intentionally not the whole catalog. Object ownership is
implicit in the containing object, while commands identify both object and component. Missing asset references remain
visible as unresolved data for recovery, but an unresolved required asset cannot silently enable invalid runtime
behavior.

Registered script properties need dynamic schemas because a project's scripts are not all compiled into the editor UI.
Use a closed `PropertyValue` union with type tags, such as number, boolean, string, vector, enum, asset reference,
object
reference, and bounded list/record. Validate it against the registered script schema on the Rust side. A property bag
without a trusted schema must not gain native mutation access. Script source language, loading, sandbox, and ABI are
separate decisions; a Script subject does not establish support for executing arbitrary code.

### Definition registry and defaults

The engine owns a registry of supported component definitions. A definition includes its stable type ID, schema version,
typed data schema, factory defaults, multiplicity, requirements, conflicts, and supported authoring/runtime operations.
The registry is queried/cached independently from instance values; labels, limits, and descriptions are not resent with
every object update.

| Definition rule | Proposed behavior                                                                                                                                                                               |
|-----------------|-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| Defaults        | The Rust factory supplies defaults on explicit creation/reset. Persist materialized authored values. Changing factory defaults does not silently alter existing objects.                        |
| Multiplicity    | Transform is exactly one dedicated base field. Most built-ins start single-instance; script and audio source types may allow multiple instances. Rules are per type, never inferred from title. |
| Requirements    | A skinned animation controller requires a compatible skeleton/target; a character motor requires a compatible collision representation. Validate the complete proposed edit before commit.      |
| Removal         | Reject removal of a required component unless the same explicit transaction also updates/removes all dependents. No hidden cascade.                                                             |
| Conflicts       | Competing active movement drivers require explicit arbitration. The first version rejects player and AI drivers simultaneously controlling the same receiver.                                   |
| Lifecycle       | Adding, enabling, disabling, and removing an attachment produce domain changes. Runtime systems reconcile owned resources; they do not execute UI callbacks.                                    |
| Bounds          | Set per-type field, string, array, nesting, component-count, and asset-reference limits. Validate before expensive allocation and resource loading.                                             |
| Versions        | Component data schemas, scene-file schema, and bridge protocol have separate version domains. Never infer one from another.                                                                     |

Engine metadata determines valid values and capabilities. The Angular presentation registry selects the local renderer,
icon, field grouping, row layout, and advanced-field placement using stable component and property IDs. Defaults and
validation constraints come from the engine definition; user layout preferences can override presentation defaults.
Semantic display metadata such as units and suggested field groups can be exported once with the descriptor, but it
does not authorize behavior or replace Rust validation. No HTML, executable JavaScript, or Angular class name arrives
from a component descriptor.

This keeps the existing `underExpanded` mechanism: common properties appear immediately, advanced properties appear
under the compact caret, and the subject header still collapses the whole card. Those choices are editor preferences,
not scene data. CSS `!default`/theme tokens remain a separate styling concern from domain factory defaults.

### Runtime behavior and minimal footprint

Store authored data separately from transient execution state. The animation component references a graph and initial
parameters; playback time, evaluated pose, state-machine progress, and temporary blends belong to runtime instances.
The audio source references a clip and settings; decoder buffers and active voices stay in the audio system. The same
rule applies to AI working memory, collision caches, and resolved asset handles.

Do not add a virtual `update()` call and a boxed object per attachment by default. Start with the engine's existing
typed
storage and systems, and resolve stable IDs to compact runtime handles once at load or structural change. Cache shared
asset data, iterate only participating instances, and keep render/audio/animation evaluation native. UI subscriptions
request selected inspector data or bounded diagnostics, not every property of every object on every simulation tick.

A component design does not require adopting Bevy or another ECS now. Benchmark authoring, structural changes, runtime
iteration, and memory before replacing storage. If an ECS is later selected, keep persistence and bridge DTOs
independent
of that framework's entity and component IDs.

For a character, input or AI produces a movement intent; the motor resolves movement/collision; animation receives
measured motion/ground state and produces a pose. Optional root motion must have one declared owner and feed the
movement
pipeline rather than writing transforms in competition with the motor. A controller's evaluation order is explicit;
the visual order of inspector subjects does not determine simulation order.

### Commands and UI integration

Retain ADR-0001's single ordered mutation queue, backend revision validation, transactions, undo/redo, receipts, and
change-cursor synchronization. New component operations are typed domain commands such as:

- `add_component` with object reference, type ID, supported schema version, and validated initial values.
- `replace_component_data` with object/component reference and the matching typed payload, or a specific semantic
  command for operations that need stronger invariants.
- `set_component_enabled` and `remove_component`.
- `create_from_preset` or `instantiate_prefab` with an explicit target scene and parent.

The existing object API can expose convenience methods over this pipeline, but must not create an independent mutation
authority or undo stack. Changing graph assets, assigning an animation graph to a component, and changing the
component's
runtime inputs are different operations. The inspector creates a Signal Forms draft, converts values through a typed
adapter, submits a command, and reconciles accepted values from the backend. Local validation improves feedback; Rust
validation decides acceptance.

Use component instance IDs to retain card focus and collapse state across data updates. For multiple object selection,
show common component/property schemas and represent mixed values explicitly. A multi-edit is one validated transaction;
a blank or mixed field is not automatically a value to write to every object.

### Persistence and migration

Introduce a new scene schema only when the loader, writer, editor, runtime, and compiler can all consume it. Keep the
schema-3 reader and use an explicit migration pipeline:

1. Preserve each entity key, parent, name, and transform. Convert model/material/renderable fields to mesh/render
   components and collider fields to collider components without changing their meaning.
2. Convert each legacy light to an object with its original key and a light component. Validate any cross-collection
   identity collision; never silently rename an identity referenced elsewhere.
3. Create a stable identity for the legacy game camera and migrate its position/orientation/projection with dedicated
   conversion tests. Keep editor navigation state separate. Define the scene's active camera reference explicitly.
4. Convert scene-scoped project behavior specifications to matching component configurations only when equivalent
   systems exist. Preserve existing local/world movement semantics. Do not run both legacy and migrated behaviors.
5. Migrate prefab schemas and internal references in the same release; validate before a user-triggered save writes
   the new representation. Opening an old file must not overwrite it immediately.

An older executable must reject an unsupported scene/component schema rather than loading it while dropping fields.
A future editor may offer a read-only diagnostic view that preserves unknown data, but unsupported executable components
must not be silently stripped or treated as supported. Plugin data needs a separately specified lossless preservation
and
migration policy before editable unknown components are allowed.

## Alternatives considered

| Alternative                                                       | Assessment                                                                                                                                                     |
|-------------------------------------------------------------------|----------------------------------------------------------------------------------------------------------------------------------------------------------------|
| Deep base-class hierarchy for static/live/controllable/player/NPC | Couples independent abilities and makes combinations or possession changes awkward; does not map naturally to Rust structs.                                    |
| One large object struct with every optional field                 | Simple initially, but every new domain expands a shared record and its validation. Migration from the current format should lead toward typed composition.     |
| `Record<string, unknown>` for every component                     | Flexible transport but loses built-in guarantees and encourages unvalidated field edits. Reserve dynamic values for bounded, registered script/plugin schemas. |
| An ECS framework as the public scene format                       | Leaks storage identities and couples persistent files to runtime implementation. Keep the authoring/transport contracts stable across storage choices.         |
| UI subjects are the engine components                             | Makes runtime behavior depend on card layout and form metadata. Subjects remain views over engine-owned data.                                                  |
| Every reusable item is a controller                               | Hides the difference between passive settings, resources, and active decision logic. Name the capability first.                                                |

## Implementation sequence and acceptance

1. Agree on names, component type IDs, base-field ownership, and the first small supported catalog. Start with
   transform,
   mesh/material, collider, light, and camera adapters over the existing engine; do not advertise unimplemented
   gameplay.
2. Implement Rust component records, definition registry, default factories, bounded validation, and representative
   serializer fixtures. Generate TypeScript contracts through ADR-0003. Keep wire definitions out of the UI subject
   store.
3. Add component commands, journal invalidation categories, reference checks, and transaction/history support. Prove
   stale writes fail, multi-edits are atomic, and add/remove/duplicate/reparent undo restores identities and values.
4. Add read-only projections and typed inspector adapters. Verify default creation, advanced fields, unresolved assets,
   repeated script/audio instances, multi-selection, and focus retention. Unknown capabilities stay unavailable.
5. Deliver versioned scene/project/prefab migration with golden fixtures covering old scenes, lights, camera
   orientation,
   identity collisions, missing dependencies, and local/world behavior semantics. Verify open/save/reload equivalence.
6. Add animation/audio/control systems incrementally, with explicit scheduling, resource ownership, lifecycle cleanup,
   and measured budgets. An authored graph is executable only after its runtime evaluator and validation exist.

Acceptance includes no mutation from card collapse/layout changes; no hidden graph evaluation in Angular; exact
component identity through reorder/undo; one transform owner per object; deterministic dependency errors; preserved
legacy scene semantics; and measured native memory/update cost under representative object counts. This ADR specifies
the target and its tests; it does not claim that any new component, migration, or performance threshold has passed.

## Consequences

The common Object model remains simple for users while component composition handles mixed capabilities. Strict
built-ins, explicit script schemas, separate runtime state, and reusable assets support a small UI and future growth.
The costs are migration tooling, component identity and dependency rules, an engine definition registry, and explicit
adapters between authored data and presentation. These are deliberate architecture requirements rather than work
hidden inside a new class hierarchy.
