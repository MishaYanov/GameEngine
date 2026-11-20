import type { ComponentRecord } from './editor-components';
import type { EditorSnapshot, Transform, Vec3 } from './editor.models';

export interface PrefabSourceEntity {
	readonly key: string;
	readonly name: string;
	readonly transform: Transform;
	readonly parent?: string | null;
	readonly components: readonly ComponentRecord[];
}

export interface PrefabSourceDocument {
	readonly schema_version: 3;
	readonly entities: readonly PrefabSourceEntity[];
	readonly gameplay: null;
	readonly reference_slots: Readonly<Record<string, string>>;
}

export interface PrefabSourceDraft {
	readonly draft_id: string;
	readonly workspace_generation: number;
	readonly revision: number;
	readonly path: string;
	readonly asset_id: string;
	readonly fingerprint: string;
	readonly prefab: PrefabSourceDocument;
}

export interface PrefabSourceValidation {
	readonly draft_id: string;
	readonly entity_count: number;
	readonly bytes: number;
}

export interface PrefabSourceSave {
	readonly draft: PrefabSourceDraft;
	readonly snapshot: EditorSnapshot;
}

export interface LinkedPrefabOverrideTarget {
	readonly source_entity: string;
	readonly property: {
		readonly kind:
			| 'name'
			| 'transform_position'
			| 'transform_rotation'
			| 'transform_scale'
			| 'mesh_renderer'
			| 'collider'
			| 'native_behavior';
		readonly component_id?: string;
	};
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
	readonly bindings: Readonly<Record<string, string>>;
	readonly overrides: readonly {
		readonly target: LinkedPrefabOverrideTarget;
		readonly base: LinkedPrefabValue;
		readonly value: LinkedPrefabValue;
	}[];
}
