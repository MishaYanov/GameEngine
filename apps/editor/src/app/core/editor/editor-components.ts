import type { Collider, MaterialOverride, Vec3 } from './editor.models';

export type NativeBehaviorSpec =
	| {
			readonly kind: 'key_door';
			readonly actor: string;
			readonly key_trigger: string;
			readonly door: string;
			readonly key_visual: string | null;
	  }
	| {
			readonly kind: 'collectible';
			readonly actor: string;
			readonly trigger: string;
			readonly visual: string | null;
			readonly despawn_owner: boolean;
	  }
	| {
			readonly kind: 'timed_spawner';
			readonly prefab_path: string;
			readonly interval_ticks: number;
			readonly max_alive: number;
			readonly lifetime_ticks: number;
			readonly spawn_offset: Vec3;
			readonly bindings: Readonly<Record<string, string>>;
	  };

export type ComponentData =
	| {
			readonly kind: 'mesh_renderer';
			readonly model_path: string;
			readonly asset_id?: string | null;
			readonly mesh_index?: number | null;
			readonly material?: MaterialOverride | null;
	  }
	| { readonly kind: 'collider'; readonly collider: Collider }
	| { readonly kind: 'native_behavior'; readonly behavior: NativeBehaviorSpec };

export interface ComponentRecord {
	readonly id: string;
	readonly schema_version: 1;
	readonly enabled: boolean;
	readonly data: ComponentData;
}

export interface ComponentTypeDescriptor {
	readonly type_id: string;
	readonly kind: ComponentData['kind'];
	readonly label: string;
	readonly description: string;
	readonly max_per_object: number;
}

export interface NativeBehaviorAttachment {
	readonly id: string;
	readonly enabled: boolean;
	readonly behavior: NativeBehaviorSpec;
}

function record(value: unknown): value is Record<string, unknown> {
	return typeof value === 'object' && value !== null && !Array.isArray(value);
}
function nonempty(value: unknown): value is string {
	return typeof value === 'string' && value.trim().length > 0;
}
function integer(value: unknown, min: number, max: number): value is number {
	return typeof value === 'number' && Number.isSafeInteger(value) && value >= min && value <= max;
}
function vector(value: unknown, size: number, min: number, max: number): boolean {
	return (
		Array.isArray(value) &&
		value.length === size &&
		value.every(
			(item) =>
				typeof item === 'number' &&
				Number.isFinite(Math.fround(item)) &&
				item >= min &&
				item <= max,
		)
	);
}
function nullableText(value: unknown): boolean {
	return value === null || nonempty(value);
}
function collider(value: unknown): boolean {
	if (!record(value) || !vector(value['offset'], 3, -Infinity, Infinity)) return false;
	const interaction = value['interaction'];
	if (
		interaction !== undefined &&
		(!record(interaction) ||
			(interaction['sensor'] !== undefined && typeof interaction['sensor'] !== 'boolean') ||
			(interaction['layers'] !== undefined && !integer(interaction['layers'], 0, 0xffffffff)) ||
			(interaction['mask'] !== undefined && !integer(interaction['mask'], 0, 0xffffffff)))
	)
		return false;
	return value['kind'] === 'box'
		? vector(value['half_extents'], 3, Number.MIN_VALUE, Infinity) &&
				(value['half_extents'] as number[]).every((value) => Math.fround(value) > 0)
		: value['kind'] === 'sphere' &&
				typeof value['radius'] === 'number' &&
				Math.fround(value['radius']) > 0 &&
				Number.isFinite(Math.fround(value['radius']));
}

export function validComponentRecord(value: unknown): value is ComponentRecord {
	if (
		!record(value) ||
		typeof value['id'] !== 'string' ||
		!/^[A-Za-z0-9_-]{1,128}$/.test(value['id']) ||
		value['schema_version'] !== 1 ||
		typeof value['enabled'] !== 'boolean' ||
		!record(value['data'])
	)
		return false;
	const data = value['data'];
	if (data['kind'] === 'mesh_renderer') {
		const material = data['material'];
		return (
			nonempty(data['model_path']) &&
			(data['asset_id'] == null || nonempty(data['asset_id'])) &&
			(data['mesh_index'] == null || integer(data['mesh_index'], 0, 1_000_000)) &&
			(material == null || (record(material) && vector(material['base_color_factor'], 4, 0, 1)))
		);
	}
	if (data['kind'] === 'collider') return collider(data['collider']);
	if (data['kind'] !== 'native_behavior' || !record(data['behavior'])) return false;
	const behavior = data['behavior'];
	switch (behavior['kind']) {
		case 'key_door':
			return (
				nonempty(behavior['actor']) &&
				nonempty(behavior['key_trigger']) &&
				nonempty(behavior['door']) &&
				nullableText(behavior['key_visual']) &&
				new Set([behavior['actor'], behavior['key_trigger'], behavior['door']]).size === 3
			);
		case 'collectible':
			return (
				nonempty(behavior['actor']) &&
				nonempty(behavior['trigger']) &&
				behavior['actor'] !== behavior['trigger'] &&
				nullableText(behavior['visual']) &&
				typeof behavior['despawn_owner'] === 'boolean'
			);
		case 'timed_spawner':
			return (
				nonempty(behavior['prefab_path']) &&
				integer(behavior['interval_ticks'], 1, 1_000_000) &&
				integer(behavior['max_alive'], 1, 128) &&
				integer(behavior['lifetime_ticks'], 1, 1_000_000) &&
				vector(behavior['spawn_offset'], 3, -1_000_000, 1_000_000) &&
				record(behavior['bindings']) &&
				Object.keys(behavior['bindings']).length <= 32 &&
				Object.entries(behavior['bindings']).every(
					([name, target]) => /^[A-Za-z0-9_-]{1,64}$/.test(name) && nonempty(target),
				)
			);
		default:
			return false;
	}
}

export function validComponentViews(value: unknown): value is readonly ComponentRecord[] {
	if (!Array.isArray(value) || value.length > 16 || !value.every(validComponentRecord))
		return false;
	return (
		new Set(value.map((component) => component.id)).size === value.length &&
		value.filter((component) => component.data.kind === 'mesh_renderer').length <= 1 &&
		value.filter((component) => component.data.kind === 'collider').length <= 1 &&
		value.filter((component) => component.data.kind === 'native_behavior').length <= 8
	);
}

export function validComponentTypes(value: unknown): value is readonly ComponentTypeDescriptor[] {
	if (
		!Array.isArray(value) ||
		!value.every(
			(item: unknown) =>
				record(item) &&
				nonempty(item['type_id']) &&
				['mesh_renderer', 'collider', 'native_behavior'].includes(String(item['kind'])) &&
				nonempty(item['label']) &&
				nonempty(item['description']) &&
				integer(item['max_per_object'], 1, item['kind'] === 'native_behavior' ? 8 : 1),
		)
	)
		return false;
	return (
		new Set(value.map((item) => item.type_id)).size === value.length &&
		new Set(value.map((item) => item.kind)).size === value.length
	);
}
