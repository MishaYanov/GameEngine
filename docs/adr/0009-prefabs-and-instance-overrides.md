# ADR-0009: Linked prefabs, instance overrides, and update conflicts

Status: Proposed; current prefab copies remain unchanged until a new schema and resolver are implemented.

Date: 2026-10-27

Scope: Reusable object hierarchies, linked instances, overrides, reference remapping, and authoring operations.

## Question and example

A robot prefab contains a mesh, collider, animation controller, and weapon child. A scene needs ten robots, one with
a different speed and another without the weapon. When the asset's collider changes, which instances change, which
values stay local, and what happens if the asset deletes the weapon targeted by an override?

Recommend explicit linked instances with stable source identities and typed overrides. Preserve the existing copy
operation as an independent workflow. Source updates are reconciled through validated editor commands; unavailable
or incompatible source data must produce visible conflicts rather than silent data loss.

## Existing implementation inspected

At `fcb748b04853209b5e17e680c38c847c19f7f365` and the inspected working tree,
`crates/engine-core/src/editor/prefab.rs` defines schema-1 `PrefabData { schema_version, entities }`. It permits
1–4096 entities and at most 16 MiB JSON. Validation uses an isolated scene to reject external parents, cycles,
duplicate keys, and invalid entity data.

Export includes selected entities and descendants, detaches external roots while preserving world placement, and
recenters the roots. Instantiation creates fresh entity keys, remaps internal parent keys, places the roots, and
adds the expanded entities through one edit. Scene role bindings continue to refer to the original scene. The
host reads/writes prefab JSON; it does not retain a link from instantiated entities to that file.

These are reusable copies. There is no override table, source revision, nested source dependency, linked update, or
apply/revert operation. [Scene persistence](../scene-persistence.md) already describes that limit. This proposal does
not reinterpret existing scenes as linked instances or infer links from similar names and values.

## Dependencies and assumptions

This depends on [object/component identities](0002-object-composition-and-components.md),
[persistent asset IDs](0006-asset-identity-and-import-pipeline.md), and the authoritative
[command pipeline](0001-editor-command-pipeline.md). Inspector subjects remain views of component data, not prefab
storage units. References to [graphs](0007-graph-assets-and-execution.md) and
[modules](0008-game-modules-and-scripting.md) are asset references, not copied executable state.

[Authoring versus Play](0004-authoring-play-and-editor-state.md) determines which instance can be edited.
[Persistence](0011-persistence-autosave-and-recovery.md) governs saves affecting source and scene documents. The
first linked implementation assumes local assets and bounded hierarchies, without collaborative merge services.

## Proposed representation

A prefab asset has an `AssetId`, schema version, and hierarchy of stable local object/component IDs. Renaming or
reordering children preserves those IDs. An instance has an instance ID, source asset ID, last accepted source
digest, a local-to-scene identity map, placement, and overrides. Scene object keys remain stable across source
updates; resolving a linked instance must not recreate every key.

For the first implementation, persist one shared immutable base snapshot per accepted prefab digest used by a
scene, bounded by document limits. This is authoring recovery data, not a disposable import cache. Overrides resolve
against that base; updating to a new source is explicit. This costs storage but permits deterministic reload and
three-way comparison even after the source changes or the derived cache is deleted. Deduplication across documents
can follow only with a durable version store; do not make correctness depend on retaining a cache entry.

Use a closed union of operations, with stable schema property IDs rather than inspector labels or array offsets:

| Override                | Meaning                                                                                               |
|-------------------------|-------------------------------------------------------------------------------------------------------|
| Set property            | Replace a typed value at an object/component/property identity; retain the base value for comparison. |
| Add object/component    | Introduce an instance-owned identity and validated data.                                              |
| Remove object/component | Record a tombstone for an inherited identity; do not delete the source definition.                    |
| Change parent           | Record an explicit relationship, checked for scope, cycles, and transform validity.                   |

Null is an actual nullable value, not a deletion marker. Collection entries needing independent overrides require
stable entry IDs. A removed inherited subtree is hidden by its tombstone; adding a new source object with a new ID
does not resurrect the removed object. Removing an instance-owned addition deletes that addition and its dependent
patches as one validated command.

Unity illustrates the useful distinction between source properties and instance overrides, including added/removed
objects and components and explicit apply/revert operations. Its override precedence supports predictable local
variation. The storage and conflict policy here are our proposal, not a copy of Unity's serialization format.
[Unity instance overrides](https://docs.unity3d.com/6000.0/Documentation/Manual/PrefabInstanceOverrides.html).

## Resolution, updates, and references

Resolve the accepted base, nested instances, then outer instance overrides in deterministic order. Validate the
result before publishing any changes. A source modification advertises an available update; an update command
checks scene/source revisions, computes a candidate rebase, and commits it as one undoable authoring change.
Unchanged inherited values adopt the new base. Valid local overrides keep their values, even when equal to the new
base, until explicitly reverted. Updating the accepted base makes the scene dirty.

If both source and instance change a property, retain the local override and expose both values. If an overridden
property/object disappears or changes incompatibly, preserve the patch as a conflict. Offer retarget, retain as an
instance-owned addition where valid, or discard through an explicit command. Do not silently drop a patch because
its target cannot currently resolve. Unresolved required references/conflicts block Play and build; last accepted
data remains inspectable.

Internal object/component references remap through the complete instance identity map before validation. Asset
references retain asset IDs. A reusable asset cannot accidentally capture a scene-local object reference: require a
declared instance reference slot, explicit clearing when nullable, or reject export. Cross-scene reference support
remains governed by ADR-0002. Duplication creates new instance and scene identities while retaining source identity.

Keep root placement in an explicit instance placement record so moving one robot is clearly local. Transform-space
rules must agree with existing local TRS; preserve-world operations may fail for unsupported shear. Never silently
convert a local transform into a world transform during reconciliation.

## Nesting and override lifecycle

Nested instances retain their own source identity, as also illustrated by
[Unity nested prefabs](https://docs.unity3d.com/6000.0/Documentation/Manual/NestedPrefabs.html). Address nested targets
using stable instance-path segments plus local IDs, never child indices. Reject dependency cycles and enforce
depth, expanded-object, and byte limits before resolution. Support ordinary linked instances before enabling nested
editing; prefab variants and arbitrary inheritance chains remain a later decision.

Revert removes selected local overrides against the accepted base. Apply explicitly chooses a source target and
validates whether local values/references are legal there. In the first release, apply only to the direct source;
nested apply requires opening that asset. Update the source draft and instance draft together in one in-memory
command transaction. Their filesystem saves remain separate and report partial failure under ADR-0011.

Unpack materializes the resolved hierarchy, preserves scene identities/references, and removes its link metadata.
Define shallow unpack separately from recursively unpacking nested links. Undo restores links and overrides.
Saving, reloading, and undoing never implicitly apply overrides back to an asset. Play mutations never become
prefab overrides without an explicit authoring operation.

## Alternatives, implementation, and open gates

Copies are simple and stay useful for one-off templates, but cannot propagate deliberate updates. Full object diffs
are easy to generate but fragile under reorder and schema evolution. Typed patches and base snapshots cost more
storage and resolver logic, while making conflicts reviewable and load behavior reproducible.

Implement stable IDs and reference validation first, then a pure resolver with copy-to-linked migration, flat
overrides, update/revert/unpack commands, and inspector badges. Add apply only after transactions span asset drafts;
add nesting after cycle/budget tests. Migration must be explicit and leave schema-1 copies valid.

Acceptance covers repeated instantiation without identity collisions, internal reference remapping, source rename,
property and deletion conflicts, tombstone persistence, undo/redo, missing assets, cache deletion, and interrupted
multi-document saves. Resolver output must be deterministic and bounded for hostile nesting. Open gates include
snapshot retention limits, property-ID migrations, root placement details, and the exact nested override UI. No
linked resolver, native acceptance run, or migration result is claimed by this ADR.
