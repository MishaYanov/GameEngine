export interface AnimationSource {
	readonly asset_id: string;
	readonly expected_sha256: string;
	readonly scene_index: number;
	readonly mesh_node: number;
	readonly skin_index: number;
    readonly character?: CharacterIdentity;
}

export interface CharacterIdentity {
    readonly clips: readonly string[];
    readonly joints: Readonly<Record<string, string>>;
    readonly root_node?: number;
}
export type AnimationPlayback =
	| { readonly kind: 'single_clip'; readonly clip_index: number; readonly looping: boolean }
	| {
			readonly kind: 'locomotion3';
			readonly idle: number;
			readonly walk: number;
			readonly air: number;
	  };
export interface AnimationSettings {
	readonly source: AnimationSource;
	readonly playback: AnimationPlayback;
	readonly speed_numerator: number;
	readonly speed_denominator: number;
}
export interface AnimationAttachment {
	readonly id: string;
	readonly enabled: boolean;
	readonly settings: AnimationSettings;
}
export interface AnimationClipMetadata {
	readonly index: number;
	readonly name: string;
	readonly duration_seconds: number;
}

export interface AnimationJointMetadata {
    readonly source_node: number;
    readonly name: string;
    readonly stable_id?: string;
    readonly parent_source_node?: number;
}
export interface AnimationReview {
	readonly workspace_generation: number;
	readonly revision: number;
	readonly metadata: {
		readonly source: AnimationSource;
		readonly mesh_index: number;
		readonly nodes: number;
		readonly joints: number;
        readonly joint_metadata?: readonly AnimationJointMetadata[];
		readonly clips: readonly AnimationClipMetadata[];
	};
}
export interface AnimationPreview {
	readonly token: string;
	readonly workspace_generation: number;
	readonly revision: number;
	readonly entity_key: string;
	readonly clip_index: number;
	readonly duration_seconds: number;
	readonly time_seconds: number;
}
export function readAnimationPreview(value: unknown): AnimationPreview {
	if (
		!record(value) ||
		typeof value['token'] !== 'string' ||
		value['token'].length !== 36 ||
		!uint(value['workspace_generation'], Number.MAX_SAFE_INTEGER) ||
		!uint(value['revision'], Number.MAX_SAFE_INTEGER) ||
		typeof value['entity_key'] !== 'string' ||
		!value['entity_key'] ||
		!uint(value['clip_index'], 31) ||
		typeof value['duration_seconds'] !== 'number' ||
		!Number.isFinite(value['duration_seconds']) ||
		value['duration_seconds'] < 0 ||
		value['duration_seconds'] > 600 ||
		typeof value['time_seconds'] !== 'number' ||
		!Number.isFinite(value['time_seconds']) ||
		value['time_seconds'] < 0 ||
		value['time_seconds'] > value['duration_seconds']
	)
		throw new Error('Animation preview returned invalid native state.');
	return value as unknown as AnimationPreview;
}
function record(value: unknown): value is Record<string, unknown> {
	return typeof value === 'object' && value !== null && !Array.isArray(value);
}
function uint(value: unknown, max: number): value is number {
	return typeof value === 'number' && Number.isSafeInteger(value) && value >= 0 && value <= max;
}

function stableId(value: unknown): value is string {
    return typeof value === 'string' && /^[a-zA-Z0-9_.-]{1,128}$/.test(value);
}

function validJointMetadata(value: unknown, source: AnimationSource, count: number): boolean {
    if (!Array.isArray(value) || value.length !== count) return false;
    const seen = new Set<number>();
    for (const joint of value) {
        if (!record(joint) || !uint(joint['source_node'], 255) || seen.has(joint['source_node']) ||
            typeof joint['name'] !== 'string' || joint['name'].length === 0 || joint['name'].length > 512 ||
            (joint['stable_id'] !== undefined && !stableId(joint['stable_id'])) ||
            (joint['parent_source_node'] !== undefined && !uint(joint['parent_source_node'], 255)) ||
            joint['stable_id'] !== source.character?.joints[String(joint['source_node'])]) return false;
        seen.add(joint['source_node']);
    }
    return true;
}

function validCharacter(value: unknown): boolean {
    if (!record(value) || !Array.isArray(value['clips']) || value['clips'].length === 0 ||
        value['clips'].length > 32 || !value['clips'].every(stableId) ||
        new Set(value['clips']).size !== value['clips'].length || !record(value['joints'])) return false;
    const joints = Object.entries(value['joints']);
    return joints.length > 0 && joints.length <= 128 &&
        joints.every(([index, id]) => /^\d+$/.test(index) && uint(Number(index), 255) && stableId(id)) &&
        new Set(joints.map(([, id]) => id)).size === joints.length &&
        (value['root_node'] === undefined || uint(value['root_node'], 255));
}

export function sameAnimationRevision(a: AnimationSource, b: AnimationSource): boolean {
    return a.asset_id === b.asset_id && a.expected_sha256 === b.expected_sha256 &&
        a.scene_index === b.scene_index && a.mesh_node === b.mesh_node && a.skin_index === b.skin_index;
}

export function canResolveAnimationSource(saved: AnimationSource, current: AnimationSource): boolean {
    if (sameAnimationRevision(saved, current)) return true;
    if (saved.asset_id !== current.asset_id || !saved.character || !current.character) return false;
    const jointIds = new Set(Object.values(current.character.joints));
    return saved.character.clips.every(id => current.character!.clips.includes(id)) &&
        Object.values(saved.character.joints).every(id => jointIds.has(id));
}

export function resolveAnimationClip(saved: AnimationSource, current: AnimationSource, index: number): number | undefined {
    if (sameAnimationRevision(saved, current)) return index;
    if (!canResolveAnimationSource(saved, current)) return undefined;
    const resolved = current.character!.clips.indexOf(saved.character!.clips[index]);
    return resolved < 0 ? undefined : resolved;
}
export function validAnimationSettings(value: unknown): value is AnimationSettings {
	if (!record(value) || !record(value['source']) || !record(value['playback'])) return false;
	const source = value['source'],
		playback = value['playback'];
	if (
		typeof source['asset_id'] !== 'string' ||
		!source['asset_id'] ||
		typeof source['expected_sha256'] !== 'string' ||
		!/^[0-9a-f]{64}$/.test(source['expected_sha256']) ||
		!uint(source['scene_index'], 0xffffffff) ||
		!uint(source['mesh_node'], 255) ||
		!uint(source['skin_index'], 127) ||
        (source['character'] !== undefined && !validCharacter(source['character'])) ||
		!uint(value['speed_numerator'], 16) ||
		!uint(value['speed_denominator'], 16) ||
		value['speed_numerator'] === 0 ||
		value['speed_denominator'] === 0 ||
		value['speed_numerator'] * 4 < value['speed_denominator'] ||
		value['speed_numerator'] > value['speed_denominator'] * 4
	)
		return false;
	return playback['kind'] === 'single_clip'
		? uint(playback['clip_index'], 31) && typeof playback['looping'] === 'boolean'
		: playback['kind'] === 'locomotion3' &&
				['idle', 'walk', 'air'].every((key) => uint(playback[key], 31));
}
export function readAnimationReview(value: unknown): AnimationReview {
	if (
		!record(value) ||
		!uint(value['workspace_generation'], Number.MAX_SAFE_INTEGER) ||
		!uint(value['revision'], Number.MAX_SAFE_INTEGER) ||
		!record(value['metadata'])
	)
		throw new Error('Animation review returned an invalid context.');
	const metadata = value['metadata'];
	const settings = {
		source: metadata['source'],
		playback: { kind: 'single_clip', clip_index: 0, looping: true },
		speed_numerator: 1,
		speed_denominator: 1,
	};
	if (
		!validAnimationSettings(settings) ||
		!uint(metadata['nodes'], 256) ||
		!uint(metadata['joints'], 128) ||
		metadata['joints'] === 0 ||
        (metadata['joint_metadata'] !== undefined &&
            !validJointMetadata(metadata['joint_metadata'], settings.source, metadata['joints'])) ||
		!uint(metadata['mesh_index'], Number.MAX_SAFE_INTEGER) ||
		!Array.isArray(metadata['clips']) ||
		metadata['clips'].length === 0 ||
		metadata['clips'].length > 32 ||
		!metadata['clips'].every(
			(item: unknown, index: number) =>
				record(item) &&
				item['index'] === index &&
				typeof item['name'] === 'string' &&
				item['name'].length <= 1024 &&
				typeof item['duration_seconds'] === 'number' &&
				Number.isFinite(item['duration_seconds']) &&
				item['duration_seconds'] >= 0 &&
				item['duration_seconds'] <= 600,
		)
	)
		throw new Error('Animation review returned invalid source or clip metadata.');
	return value as unknown as AnimationReview;
}
