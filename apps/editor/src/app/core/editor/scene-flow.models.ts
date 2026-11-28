import type {EntityKey, SceneId, Vec3} from './editor.models';

export type SceneRouteActivation =
    | { readonly kind: 'enter_sensor'; readonly sensor: EntityKey; readonly actor?: EntityKey }
    | {
    readonly kind: 'interact_in_sensor';
    readonly sensor: EntityKey;
    readonly actor?: EntityKey;
    readonly action: string
}
    | { readonly kind: 'sequence_completed'; readonly instance: EntityKey }
    | { readonly kind: 'manual' };

export type SceneRoutePreload =
    | { readonly kind: 'approach'; readonly distance: number; readonly release_distance: number }
    | { readonly kind: 'when_sequence_starts'; readonly instance: EntityKey };

export interface SceneRoute {
    readonly id: string;
    readonly from: SceneId;
    readonly to: SceneId;
    readonly activate: SceneRouteActivation;
    readonly condition?: { readonly kind: 'gameplay_flag'; readonly key: string };
    readonly preload?: SceneRoutePreload;
    readonly arrival: EntityKey;
    readonly arrival_actor?: EntityKey;
    readonly start_sequence?: EntityKey;
    readonly priority?: number;
}

export type SequenceAction =
    | { readonly kind: 'camera_cut' | 'audio'; readonly role: string }
    | { readonly kind: 'play_clip'; readonly role: string; readonly clip_index: number }
    | { readonly kind: 'subtitle'; readonly text: string; readonly duration_ms: number };

export interface SequenceCue {
    readonly at_ms: number;
    readonly action: SequenceAction;
}

export interface SequenceAsset {
    readonly id: string;
    readonly schema_version?: 1;
    readonly duration_ms: number;
    readonly cues: readonly SequenceCue[];
}

export interface SequenceInstance {
    readonly id: string;
    readonly scene_id: SceneId;
    readonly sequence_id: string;
    readonly bindings?: Readonly<Record<string, EntityKey>>;
    readonly trigger?: { readonly sensor: EntityKey; readonly actor: EntityKey };
    readonly once_per_entry?: boolean;
}

export interface SequenceLibrary {
    readonly assets?: readonly SequenceAsset[];
    readonly instances?: readonly SequenceInstance[];
}

export interface WorldRegion {
    readonly world: SceneId;
    readonly scene: SceneId;
    readonly bounds: { readonly min: Vec3; readonly max: Vec3 };
    readonly preload_distance: number;
    readonly release_distance: number;
}
