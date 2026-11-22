import type {AnimationPreviewDocument} from './animation-workspace.models';
import type {EditorSnapshot, EntityKey, SceneId, Vec3} from './editor.models';
import type {PropAddress} from './linked-prefabs.models';

export type Quaternion = readonly [number, number, number, number];
export type ClipOwner =
    | {readonly kind: 'scene'; readonly scene_id: SceneId}
    | {readonly kind: 'prop'; readonly asset_id: string};
export type VisualClipTarget =
    | {readonly kind: 'scene'; readonly entity_key: EntityKey}
    | {readonly kind: 'prop'; readonly address: PropAddress};
type ClipKey<Value> = {readonly at: number; readonly value: Value};
type Interpolation = 'hold' | 'linear';
type VisualTrack =
    | {readonly kind: 'local_position'; readonly interpolation: Interpolation; readonly keys: readonly ClipKey<Vec3>[]}
    | {readonly kind: 'local_rotation'; readonly interpolation: Interpolation; readonly keys: readonly ClipKey<Quaternion>[]};

export interface AuthoredClip {
    readonly schema_version: 1;
    readonly name: string;
    readonly owner: ClipOwner;
    readonly duration_ticks: number;
    readonly tracks: readonly {readonly target: VisualClipTarget; readonly track: VisualTrack}[];
}

export interface ClipAssetIndex {
    readonly asset_id: string;
    readonly path: string;
    readonly fingerprint: string;
    readonly revision: number;
}

export interface ClipAssetDocument {
    readonly workspace_generation: number;
    readonly asset: ClipAssetIndex;
    readonly clip: AuthoredClip;
}

export interface ClipAssetSave {
    readonly document: ClipAssetDocument;
    readonly snapshot: EditorSnapshot;
}

export interface ClipAssetVersion {
    readonly asset_id: string;
    readonly fingerprint: string;
}

export interface VisualClipPreview {
    readonly token: string;
    readonly workspace_generation: number;
    readonly revision: number;
    readonly request_id: number;
    readonly duration_ticks: number;
    readonly tick: number;
    readonly poses: readonly {readonly entity_key: EntityKey; readonly position: Vec3; readonly rotation_xyzw: Quaternion; readonly scale: Vec3}[];
}

export interface AuthoredClipPreviewRequest {
    readonly assetId: string;
    readonly fingerprint: string;
    readonly scope: AnimationPreviewDocument | null;
    readonly expectedRevision: number;
    readonly expectedGeneration: number;
    readonly requestId: number;
}

export function readAuthoredClipPreview(value: unknown): VisualClipPreview {
    const preview = value as Partial<VisualClipPreview> | null;
    if (!preview || typeof preview.token !== 'string' || preview.token.length !== 36 ||
        typeof preview.duration_ticks !== 'number' || !Number.isSafeInteger(preview.duration_ticks) ||
        preview.duration_ticks < 1 || preview.duration_ticks > 600_000 ||
        typeof preview.tick !== 'number' || !Number.isSafeInteger(preview.tick) || preview.tick < 0 ||
        preview.tick > preview.duration_ticks || !Number.isSafeInteger(preview.workspace_generation) ||
        !Number.isSafeInteger(preview.revision) || !Number.isSafeInteger(preview.request_id) ||
        !Array.isArray(preview.poses) || preview.poses.length > 64 ||
        !preview.poses.every(pose => pose && typeof pose.entity_key === 'string' &&
            vector(pose.position, 3) && vector(pose.rotation_xyzw, 4) && vector(pose.scale, 3))) {
        throw new Error('The visual preview returned invalid animation data.');
    }
    return preview as VisualClipPreview;
}

function vector(value: unknown, length: number): boolean {
    return Array.isArray(value) && value.length === length && value.every(Number.isFinite);
}
