import type { ComponentData, ComponentRecord } from '../../../../core/editor/editor-components';
import type { Collider, ModelAsset, Vec3, Vec4 } from '../../../../core/editor/editor.models';
import { parseF32 } from '../../../../core/editor/authoring-number';

export interface ComponentTarget {
	readonly key: string;
	readonly name: string;
}
export interface ComponentDraft {
	id: string;
	kind: ComponentData['kind'];
	enabled: boolean;
	model_path: string;
	mesh_index: string;
	tint: boolean;
	color: { r: string; g: string; b: string; a: string };
	behavior_kind: 'key_door' | 'collectible' | 'timed_spawner';
	actor: string;
	key_trigger: string;
	door: string;
	key_visual: string;
	trigger: string;
	visual: string;
	despawn_owner: boolean;
	prefab_path: string;
	interval_ticks: string;
	max_alive: string;
	lifetime_ticks: string;
	spawn_offset: { x: string; y: string; z: string };
	bindings: { name: string; target: string }[];
}

export function componentDraft(
	kind: ComponentData['kind'],
	id: string,
	owner: string,
	value?: ComponentRecord,
): ComponentDraft {
	const draft: ComponentDraft = {
		id,
		kind,
		enabled: value?.enabled ?? true,
		model_path: '',
		mesh_index: '',
		tint: false,
		color: { r: '1', g: '1', b: '1', a: '1' },
		behavior_kind: 'key_door',
		actor: '',
		key_trigger: '',
		door: owner,
		key_visual: '',
		trigger: '',
		visual: '',
		despawn_owner: false,
		prefab_path: '',
		interval_ticks: '60',
		max_alive: '4',
		lifetime_ticks: '300',
		spawn_offset: { x: '2', y: '0', z: '0' },
		bindings: [],
	};
	if (!value) return draft;
	if (value.data.kind === 'mesh_renderer') {
		draft.model_path = value.data.model_path;
		draft.mesh_index = value.data.mesh_index == null ? '' : String(value.data.mesh_index);
		draft.tint = !!value.data.material;
		const color = value.data.material?.base_color_factor ?? [1, 1, 1, 1];
		draft.color = {
			r: String(color[0]),
			g: String(color[1]),
			b: String(color[2]),
			a: String(color[3]),
		};
	} else if (value.data.kind === 'native_behavior') {
		const behavior = value.data.behavior;
		draft.behavior_kind = behavior.kind;
		if (behavior.kind === 'timed_spawner') {
			draft.prefab_path = behavior.prefab_path;
			draft.interval_ticks = String(behavior.interval_ticks);
			draft.max_alive = String(behavior.max_alive);
			draft.lifetime_ticks = String(behavior.lifetime_ticks);
			draft.spawn_offset = {
				x: String(behavior.spawn_offset[0]),
				y: String(behavior.spawn_offset[1]),
				z: String(behavior.spawn_offset[2]),
			};
			draft.bindings = Object.entries(behavior.bindings).map(([name, target]) => ({
				name,
				target,
			}));
		} else {
			draft.actor = behavior.actor;
			if (behavior.kind === 'key_door') {
				draft.key_trigger = behavior.key_trigger;
				draft.door = behavior.door;
				draft.key_visual = behavior.key_visual ?? '';
			} else {
				draft.trigger = behavior.trigger;
				draft.visual = behavior.visual ?? '';
				draft.despawn_owner = behavior.despawn_owner;
			}
		}
	}
	return draft;
}

export function relativeAssetPath(value: string): boolean {
	return (
		value.trim().length > 0 &&
		new TextEncoder().encode(value).length <= 4096 &&
		!/[:\\\p{Cc}]/u.test(value) &&
		!value.startsWith('/') &&
		value.split('/').every((part) => part !== '' && part !== '.' && part !== '..')
	);
}

function uint(value: string, label: string, min: number, max: number): number {
	if (!/^\d+$/.test(value.trim()))
		throw new Error(`${label} must be a whole number from ${min} to ${max}.`);
	const number = Number(value);
	if (!Number.isSafeInteger(number) || number < min || number > max)
		throw new Error(`${label} must be from ${min} to ${max}.`);
	return number;
}

function finite(value: string, label: string, min: number, max: number): number {
	const number = parseF32(value);
	if (number === null || number < min || number > max)
		throw new Error(`${label} must be a finite number from ${min} to ${max}.`);
	return number;
}

export function componentFromDraft(
	draft: ComponentDraft,
	collider: Collider,
	targets: readonly ComponentTarget[],
	models: readonly ModelAsset[],
	original?: ComponentRecord,
): ComponentRecord {
	const reference = (key: string, label: string): string => {
		if (!targets.some((target) => target.key === key))
			throw new Error(`Choose an existing ${label}.`);
		return key;
	};
	const optionalReference = (key: string, label: string) => (key ? reference(key, label) : null);
	let data: ComponentData;
	if (draft.kind === 'mesh_renderer') {
		if (!relativeAssetPath(draft.model_path) || draft.model_path === '__no_mesh__')
			throw new Error('Choose an asset-relative model path.');
		const color: Vec4 = draft.tint
			? [
					finite(draft.color.r, 'Material R', 0, 1),
					finite(draft.color.g, 'Material G', 0, 1),
					finite(draft.color.b, 'Material B', 0, 1),
					finite(draft.color.a, 'Material A', 0, 1),
				]
			: [1, 1, 1, 1];
		const previous =
			original?.data.kind === 'mesh_renderer' && original.data.model_path === draft.model_path
				? original.data.asset_id
				: null;
		data = {
			kind: 'mesh_renderer',
			model_path: draft.model_path,
			asset_id:
				models.find((model) => model.kind !== 'prefab' && model.path === draft.model_path)
					?.asset_id ??
				previous ??
				null,
			mesh_index: draft.mesh_index.trim()
				? uint(draft.mesh_index, 'Mesh index', 0, 1_000_000)
				: null,
			material: draft.tint ? { base_color_factor: color } : null,
		};
	} else if (draft.kind === 'collider') data = { kind: 'collider', collider };
	else if (draft.behavior_kind === 'key_door') {
		const actor = reference(draft.actor, 'actor');
		const trigger = reference(draft.key_trigger, 'key sensor');
		const door = reference(draft.door, 'door');
		if (new Set([actor, trigger, door]).size !== 3)
			throw new Error('Actor, key sensor and door must be different entities.');
		data = {
			kind: 'native_behavior',
			behavior: {
				kind: 'key_door',
				actor,
				key_trigger: trigger,
				door,
				key_visual: optionalReference(draft.key_visual, 'key visual'),
			},
		};
	} else if (draft.behavior_kind === 'collectible') {
		const actor = reference(draft.actor, 'actor');
		const trigger = reference(draft.trigger, 'trigger');
		if (actor === trigger) throw new Error('Actor and trigger must be different entities.');
		data = {
			kind: 'native_behavior',
			behavior: {
				kind: 'collectible',
				actor,
				trigger,
				visual: optionalReference(draft.visual, 'visual'),
				despawn_owner: draft.despawn_owner,
			},
		};
	} else {
		if (!relativeAssetPath(draft.prefab_path) || !/\.json$/i.test(draft.prefab_path))
			throw new Error('Choose an asset-relative JSON prefab path.');
		const offset: Vec3 = [
			finite(draft.spawn_offset.x, 'Spawn offset X', -1_000_000, 1_000_000),
			finite(draft.spawn_offset.y, 'Spawn offset Y', -1_000_000, 1_000_000),
			finite(draft.spawn_offset.z, 'Spawn offset Z', -1_000_000, 1_000_000),
		];
		if (
			draft.bindings.length > 32 ||
			draft.bindings.some((binding) => !/^[A-Za-z0-9_-]{1,64}$/.test(binding.name)) ||
			new Set(draft.bindings.map((binding) => binding.name)).size !== draft.bindings.length
		) {
			throw new Error(
				'Use up to 32 unique reference slot names with ASCII letters, digits, underscores or hyphens.',
			);
		}
		const bindings = Object.fromEntries(
			draft.bindings.map((binding) => [
				binding.name,
				reference(binding.target, `target for slot ${binding.name}`),
			]),
		);
		data = {
			kind: 'native_behavior',
			behavior: {
				kind: 'timed_spawner',
				prefab_path: draft.prefab_path,
				interval_ticks: uint(draft.interval_ticks, 'Spawn interval', 1, 1_000_000),
				max_alive: uint(draft.max_alive, 'Maximum alive', 1, 128),
				lifetime_ticks: uint(draft.lifetime_ticks, 'Lifetime', 1, 1_000_000),
				spawn_offset: offset,
				bindings,
			},
		};
	}
	return { id: draft.id, schema_version: 1, enabled: draft.enabled, data };
}
