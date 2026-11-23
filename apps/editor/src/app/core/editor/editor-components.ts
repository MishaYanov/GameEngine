import {validAudioSourceSettings, type AudioSourceSettings} from './audio-source.models';
import {validSkySettings, type SkySettings} from './sky.models';
import type { CameraComponentSettings, Collider, MaterialOverride, Vec3 } from './editor.models';
import { validMachine, type MachineDefinition } from './state-machine.models';
import {
	validControllerSettings,
	type CharacterControllerSettings,
} from './character-controller.models';
import { validAnimationSettings, type AnimationSettings } from './animation.models';
import {validAnimatorSettings, type AnimatorSettings} from './animator.models';
import {validPlayerControllerSettings, type PlayerControllerSettings} from './player-controller.models';

import {validGameActionId} from '../game-input/game-input.models';

export interface InteractableSettings {
    readonly action: string;
    readonly reach: number;
}

export const MAX_INTERACTION_REACH = 3;

export function validInteractionReach(value: unknown): value is number {
    return typeof value === 'number' && Math.fround(value) > 0 && Math.fround(value) <= MAX_INTERACTION_REACH;
}

export type NativeBehaviorSpec =
	| { readonly kind: 'state_machine'; readonly definition: MachineDefinition }
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
    | { readonly kind: 'audio_source'; readonly settings: AudioSourceSettings }
	| {
			readonly kind: 'mesh_renderer';
			readonly model_path: string;
			readonly asset_id?: string | null;
			readonly mesh_index?: number | null;
			readonly material?: MaterialOverride | null;
	  }
	| { readonly kind: 'collider'; readonly collider: Collider }
	| { readonly kind: 'character_controller'; readonly settings: CharacterControllerSettings }
	| { readonly kind: 'player_controller'; readonly settings: PlayerControllerSettings }
	| { readonly kind: 'animation'; readonly settings: AnimationSettings }
	| { readonly kind: 'animator'; readonly settings: AnimatorSettings }
	| { readonly kind: 'camera'; readonly settings: CameraComponentSettings }
	| { readonly kind: 'sky'; readonly settings: SkySettings }
    | { readonly kind: 'interactable'; readonly settings: InteractableSettings }
	| { readonly kind: 'script'; readonly settings: ScriptSettings }
	| { readonly kind: 'native_behavior'; readonly behavior: NativeBehaviorSpec };

export type ScriptReference = {readonly entity: string} | {readonly slot: string};
export type ScriptProperty = boolean | number | string;

export interface ScriptSettings {
    readonly script_id: string;
    readonly config_version: number;
    readonly properties?: Readonly<Record<string, ScriptProperty>>;
    readonly references?: Readonly<Record<string, ScriptReference>>;
}

export function validScriptSettings(value: unknown): value is ScriptSettings {
    if (!record(value) || Object.keys(value).some(key =>
        !['script_id', 'config_version', 'properties', 'references'].includes(key))) return false;
    const id = value['script_id'];
    const field = (name: string) => /^[A-Za-z0-9_-]{1,64}$/.test(name);
    if (typeof id !== 'string' || id.length > 128 || !id.split('.').every(field) ||
        !integer(value['config_version'], 1, 4_294_967_295)) return false;
    const properties = value['properties'];
    if (properties !== undefined && (!record(properties) || Object.keys(properties).length > 32 ||
        !Object.entries(properties).every(([name, property]) => field(name) &&
            (typeof property === 'boolean' ||
                (typeof property === 'number' && Number.isFinite(Math.fround(property))) ||
                (typeof property === 'string' && new TextEncoder().encode(property).length <= 256))))) return false;
    const references = value['references'];
    return references === undefined || (record(references) && Object.keys(references).length <= 32 &&
        Object.entries(references).every(([name, reference]) => field(name) && record(reference) &&
            Object.keys(reference).length === 1 &&
            (nonempty(reference['entity']) || (typeof reference['slot'] === 'string' && field(reference['slot'])))));
}

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

export function validCameraComponentSettings(value: unknown): value is CameraComponentSettings {
	if (!record(value) || !record(value['lens']) || !record(value['rig'])) return false;
	const {lens, rig} = value;
	if (typeof lens['vertical_fov'] !== 'number' || !(lens['vertical_fov'] > 0 && lens['vertical_fov'] < Math.PI) ||
		typeof lens['near'] !== 'number' || typeof lens['far'] !== 'number' ||
		!Number.isFinite(Math.fround(lens['near'])) || !Number.isFinite(Math.fround(lens['far'])) ||
		!(lens['near'] > 0 && lens['far'] > lens['near'])) return false;
	if (rig['mode'] === 'fixed') return rig['settings'] === undefined;
	const settings = rig['settings'];
	if (!record(settings) || !nonempty(settings['target'])) return false;
	if (rig['mode'] === 'follow') return vector(settings['offset'], 3, -Infinity, Infinity) &&
		vector(settings['look_offset'], 3, -Infinity, Infinity) &&
		typeof settings['smoothing_seconds'] === 'number' &&
		settings['smoothing_seconds'] >= 0 && settings['smoothing_seconds'] <= 10;
	return rig['mode'] === 'orbit' &&
		vector(settings['pivot_offset'], 3, -Infinity, Infinity) &&
		['initial_yaw', 'initial_pitch', 'initial_distance', 'min_distance', 'max_distance', 'min_pitch', 'max_pitch',
			'follow_seconds', 'recovery_seconds', 'teleport_distance', 'probe_radius'].every(
			key => typeof settings[key] === 'number' && Number.isFinite(Math.fround(settings[key])),
		) && integer(settings['obstruction_mask'], 1, 0xffff_ffff);
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
	if (data['kind'] === 'audio_source') return validAudioSourceSettings(data['settings']);
	if (data['kind'] === 'sky') return validSkySettings(data['settings']);
	if (data['kind'] === 'script') return validScriptSettings(data['settings']);
	if (data['kind'] === 'collider') return collider(data['collider']);
	if (data['kind'] === 'character_controller') return validControllerSettings(data['settings']);
	if (data['kind'] === 'player_controller') return validPlayerControllerSettings(data['settings']) &&
		(value['enabled'] || !data['settings'].local);
	if (data['kind'] === 'animation') return validAnimationSettings(data['settings']);
	if (data['kind'] === 'animator') return validAnimatorSettings(data['settings']);
	if (data['kind'] === 'camera') return validCameraComponentSettings(data['settings']);
    if (data['kind'] === 'interactable') {
        const settings = data['settings'];
        return Object.keys(data).length === 2 && record(settings) && Object.keys(settings).length === 2 &&
            validGameActionId(settings['action']) && validInteractionReach(settings['reach']);
    }
	if (data['kind'] !== 'native_behavior' || !record(data['behavior'])) return false;
	const behavior = data['behavior'];
	switch (behavior['kind']) {
		case 'state_machine':
			return validMachine(behavior['definition']);
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
		value.filter((component) => component.data.kind === 'audio_source').length <= 1 &&
		value.filter((component) => component.data.kind === 'character_controller').length <= 1 &&
		value.filter((component) => component.data.kind === 'player_controller').length <= 1 &&
		value.filter((component) => component.data.kind === 'animation').length <= 1 &&
		value.filter((component) => component.data.kind === 'animator').length <= 1 &&
		value.filter((component) => component.data.kind === 'camera').length <= 1 &&
        value.filter((component) => component.data.kind === 'interactable').length <= 1 &&
		value.filter((component) => component.data.kind === 'script').length <= 8 &&
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
				[
					'mesh_renderer',
					'collider',
					'audio_source',
					'native_behavior',
					'script',
					'character_controller',
					'player_controller',
					'animation',
					'animator',
					'camera',
					'sky',
                    'interactable',
				].includes(String(item['kind'])) &&
				nonempty(item['label']) &&
				nonempty(item['description']) &&
				integer(item['max_per_object'], 1, item['kind'] === 'native_behavior' || item['kind'] === 'script' ? 8 : 1),
		)
	)
		return false;
	return (
		new Set(value.map((item) => item.type_id)).size === value.length &&
		new Set(value.map((item) => item.kind)).size === value.length
	);
}
