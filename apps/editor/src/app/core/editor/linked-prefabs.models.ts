import type { ComponentRecord } from './editor-components';
import type {EditorSnapshot, EntityKey, Transform, Vec3} from './editor.models';

/** The fields used for reference navigation from an authored linked instance root. */
export interface LinkedPrefabReferenceView {
    readonly source: { readonly source_id: string; readonly path: string; readonly prefab?: PrefabSourceDocument };
    readonly bindings: Readonly<Record<string, EntityKey>>;
}

export interface PrefabSourceEntity {
	readonly key: string;
	readonly name: string;
	readonly transform: Transform;
	readonly parent?: string | null;
	readonly components: readonly ComponentRecord[];
}

export interface PrefabSourceDocument {
	readonly schema_version: 3 | 4;
	readonly entities: readonly PrefabSourceEntity[];
	readonly gameplay: null;
	readonly reference_slots: Readonly<Record<string, string>>;
	readonly composition?: NestedPropComposition;
}

export interface PinnedPropVersion {
    readonly asset_id: string;
    readonly fingerprint: string;
}

export type NestedPropBinding =
    | {readonly kind: 'object'; readonly key: EntityKey}
    | {readonly kind: 'slot'; readonly name: string};

export interface NestedPropPlacement {
    readonly id: string;
    readonly name: string;
    readonly target: PinnedPropVersion;
    readonly parent: EntityKey;
    readonly local: Transform;
    readonly bindings?: Readonly<Record<string, NestedPropBinding>>;
}

export interface NestedPropPin {
    readonly version: PinnedPropVersion;
    readonly path: string;
    /** Pinned scene metadata round-trips to the native validator unchanged. */
    readonly entities: readonly unknown[];
    readonly reference_slots?: Readonly<Record<string, string>>;
    readonly placements?: readonly NestedPropPlacement[];
}

export interface NestedPropComposition {
    readonly placements: readonly NestedPropPlacement[];
    readonly pins: readonly NestedPropPin[];
}

export interface NestedPropSource extends PinnedPropVersion {
    readonly slots: readonly string[];
}

export interface NestedPropPlacementRequest extends PinnedPropVersion {
    readonly name: string;
    readonly parent: EntityKey;
    readonly local: Transform;
    readonly bindings: Readonly<Record<string, NestedPropBinding>>;
}

export interface PrefabSourceDraft {
	readonly draft_id: string;
	readonly workspace_generation: number;
	readonly revision: number;
	readonly path: string;
	readonly asset_id: string;
	readonly fingerprint: string;
	readonly prefab: PrefabSourceDocument;
	readonly draft_revision?: number;
	readonly dirty?: boolean;
	readonly can_undo?: boolean;
	readonly can_redo?: boolean;
}

export type PropDraftEdit =
    | { readonly type: 'apply'; readonly prefab: PrefabSourceDocument }
    | { readonly type: 'undo' | 'redo' | 'discard' };

export interface PrefabSourceValidation {
	readonly draft_id: string;
	readonly entity_count: number;
	readonly bytes: number;
}

export interface PrefabSourceSave {
	readonly draft: PrefabSourceDraft;
	readonly snapshot: EditorSnapshot;
}

export type PropAddress =
    | { readonly kind: 'object'; readonly placement_path: readonly string[]; readonly source_entity: EntityKey }
    | { readonly kind: 'placement'; readonly placement_path: readonly string[] };

export type LinkedPrefabPropertyKind =
    | 'name'
    | 'transform_position'
    | 'transform_rotation'
    | 'transform_scale'
    | 'mesh_renderer'
    | 'collider'
    | 'character_controller'
    | 'animation'
    | 'particle_emitter'
    | 'native_behavior'
    | 'script';

export type LinkedPrefabProperty =
    | { readonly kind: Exclude<LinkedPrefabPropertyKind, 'native_behavior' | 'script'> }
    | { readonly kind: 'native_behavior' | 'script'; readonly component_id: string };

export interface LinkedPrefabOverrideTarget {
    readonly source_entity: EntityKey;
    readonly property: LinkedPrefabProperty;
}

export interface LinkedPrefabOverride {
    readonly target: LinkedPrefabOverrideTarget;
    readonly address: PropAddress;
    readonly can_apply_to_root_source: boolean;
    readonly base: LinkedPrefabValue;
    readonly value: LinkedPrefabValue;
}

export interface LinkedPrefabOverrideReview {
    readonly target: LinkedPrefabOverrideTarget;
    readonly address: PropAddress;
    readonly base: LinkedPrefabValue;
    readonly local: LinkedPrefabValue;
    readonly incoming: LinkedPrefabValue | null;
    readonly conflict: boolean;
}

export type LinkedPrefabConflictChoice = 'keep_local' | 'use_source';

export interface LinkedPrefabOverrideResolution {
    readonly address: PropAddress;
    readonly property: LinkedPrefabProperty;
    readonly resolution: LinkedPrefabConflictChoice;
}

export type LinkedPrefabValue =
	| { readonly kind: 'name'; readonly value: string }
	| { readonly kind: 'vector'; readonly value: Vec3 }
	| { readonly kind: 'component'; readonly value: ComponentRecord | null };

export interface LinkedPrefabStatus {
	readonly root_key: string;
	readonly source_id: string;
	readonly source_path: string;
	readonly source_fingerprint: string;
	readonly source_changed: boolean;
	readonly source_error: string | null;
    readonly direct_placements?: number;
	readonly bindings: Readonly<Record<string, string>>;
	readonly overrides: readonly LinkedPrefabOverride[];
}

export interface LinkedPrefabRecoveryReview {
    readonly root_key: EntityKey;
    readonly prior_source_id: string;
    readonly prior_path: string;
    readonly prior_fingerprint: string;
    readonly source_id: string;
    readonly source_path: string;
    readonly source_fingerprint: string;
    readonly member_count: number;
    readonly binding_count: number;
    readonly override_count: number;
}
