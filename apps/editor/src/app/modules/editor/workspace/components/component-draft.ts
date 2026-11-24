import type {AudioEventBinding, AudioGameplaySignal, AudioSourceBus, AudioSourcePlayback, LegacyCombatCue} from '../../../../core/editor/audio-source.models';
import {
    validInteractionReach,
    type ComponentData,
    type ComponentRecord
} from '../../../../core/editor/editor-components';
import {validGameActionId} from '../../../../core/game-input/game-input.models';
import type { Collider, ModelAsset, Vec3, Vec4 } from '../../../../core/editor/editor.models';
import { parseF32 } from '../../../../core/editor/authoring-number';
import {
    DEFAULT_PARTICLE_EMITTER, PARTICLE_NUMBER_FIELDS, validParticleEmitterSettings,
    type ParticleEmitterSettings, type ParticleNumberKey,
} from '../../../../core/editor/particle-emitter.models';
import {cameraComponentDraft, cameraComponentFromDraft, type CameraComponentDraft} from './camera-component-draft';
import {skyComponentDraft, skyComponentFromDraft, type SkyComponentDraft} from './sky-component-draft';
import type {InputActionsRef, PlayerMovementSpace} from '../../../../core/editor/player-controller.models';
import {
	defaultMachine,
	machineError,
	machineReferences,
	type MachineDefinition,
} from '../../../../core/editor/state-machine.models';
import {
	CONTROLLER_FIELDS,
	type CharacterControllerSettings,
	type ControllerNumberKey,
} from '../../../../core/editor/character-controller.models';
import {
	validAnimationSettings,
	type AnimationSettings,
} from '../../../../core/editor/animation.models';

import type {ScriptDescriptor} from '../../../../core/editor/object-scripts.models';
import {scriptDraft, scriptSettings, type ScriptDraft} from './script-component-draft';

export interface ComponentTarget {
	readonly key: string;
	readonly name: string;
	readonly collider?: Collider | null;
	readonly collision_enabled?: boolean;
    readonly parent?: string | null;
	readonly component_views?: readonly ComponentRecord[];
	readonly components?: readonly ComponentRecord[];
}
export interface ComponentDraft {
	id: string;
	kind: ComponentData['kind'];
	enabled: boolean;
	model_path: string;
	script: ScriptDraft;
	mesh_index: string;
	mesh_id: string;
	tint: boolean;
	color: { r: string; g: string; b: string; a: string };
	behavior_kind: 'key_door' | 'collectible' | 'timed_spawner' | 'state_machine';
	machine: MachineDefinition;
	controller: Record<ControllerNumberKey, string> & { jump_action: string };
	playerController: {input: InputActionsRef; space: PlayerMovementSpace; control_graph?: string};
	animation: AnimationSettings | null;
	camera: CameraComponentDraft;
	sky: SkyComponentDraft;
	light: {kind: 'directional' | 'point'; color: Record<'red' | 'green' | 'blue', string>; intensity: string; range: string};
    particle: Record<ParticleNumberKey | 'r' | 'g' | 'b', string>;
    audioSource: {
        clipId: string; gain: string; loop: boolean; playback: AudioSourcePlayback; bus: AudioSourceBus;
        spatial: 'two_d' | 'spatial'; near: string; far: string;
        eventKind: AudioEventBinding['kind'] | 'none'; signal: AudioGameplaySignal;
        eventClip: string; eventMarker: string; replacesCue: LegacyCombatCue | '';
    };
    interactable: { action: string; reach: string };
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
		script: scriptDraft(),
		mesh_index: '',
		mesh_id: '',
		tint: false,
		color: { r: '1', g: '1', b: '1', a: '1' },
		behavior_kind: 'key_door',
		machine: defaultMachine(),
		controller: {
			...(Object.fromEntries(
				CONTROLLER_FIELDS.map((field) => [field.key, String(field.initial)]),
			) as Record<ControllerNumberKey, string>),
			jump_action: 'jump',
		},
		playerController: {input: {kind: 'project_default'}, space: 'camera_relative'},
		animation: null,
		camera: cameraComponentDraft(),
		sky: skyComponentDraft(),
		light: {kind: 'point', color: {red: '1', green: '1', blue: '1'}, intensity: '0.75', range: '10'},
        particle: particleDraft(value?.data.kind === 'particle_emitter' ? value.data.settings : DEFAULT_PARTICLE_EMITTER),
        audioSource: {clipId: '', gain: '1', loop: false, playback: 'on_scene_enter', bus: 'effects',
            spatial: 'two_d', near: '1', far: '20', eventKind: 'none', signal: 'activated',
            eventClip: '', eventMarker: '', replacesCue: ''},
        interactable: {action: 'interact', reach: '1'},
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
		draft.mesh_id = value.data.mesh_id ?? '';
		draft.tint = !!value.data.material;
		const color = value.data.material?.base_color_factor ?? [1, 1, 1, 1];
		draft.color = {
			r: String(color[0]),
			g: String(color[1]),
			b: String(color[2]),
			a: String(color[3]),
		};
	} else if (value.data.kind === 'character_controller') {
		draft.controller = {
			...(Object.fromEntries(
				CONTROLLER_FIELDS.map((field) => [
					field.key,
					String(
						value.data.kind === 'character_controller'
							? value.data.settings[field.key]
							: field.initial,
					),
				]),
			) as Record<ControllerNumberKey, string>),
			jump_action: value.data.settings.jump_action,
		};
	} else if (value.data.kind === 'animation') {
		draft.animation = structuredClone(value.data.settings);
	} else if (value.data.kind === 'player_controller') {
		const {input, space, control_graph} = value.data.settings;
		draft.playerController = {input, space, ...(control_graph ? {control_graph} : {})};
	} else if (value.data.kind === 'audio_source') {
        const {clip, gain, loop, playback, bus, spatial, event, replaces_cue} = value.data.settings;
        const distance = spatial?.mode === 'spatial' ? spatial : undefined;
        draft.audioSource = {clipId: clip?.asset ?? '', gain: String(gain), loop, playback, bus,
            spatial: spatial?.mode ?? 'two_d', near: String(distance?.near ?? 1), far: String(distance?.far ?? 20),
            eventKind: event?.kind ?? 'none', signal: event?.kind === 'gameplay_signal' ? event.signal : 'activated',
            eventClip: event?.kind === 'animation_marker' ? event.clip : '',
            eventMarker: event?.kind === 'animation_marker' ? event.marker : '', replacesCue: replaces_cue ?? ''};
	} else if (value.data.kind === 'camera') {
		draft.camera = cameraComponentDraft(value.data.settings);
    } else if (value.data.kind === 'light') {
        const {kind, color: [r, g, b], intensity, range} = value.data.settings;
        draft.light = {kind, color: {red: String(r), green: String(g), blue: String(b)}, intensity: String(intensity), range: String(range)};
	} else if (value.data.kind === 'sky') {
		draft.sky = skyComponentDraft(value.data.settings);
    } else if (value.data.kind === 'interactable') {
        draft.interactable = {action: value.data.settings.action, reach: String(value.data.settings.reach)};
	} else if (value.data.kind === 'script') {
		draft.script = scriptDraft(value.data.settings);
	} else if (value.data.kind === 'native_behavior') {
		const behavior = value.data.behavior;
		draft.behavior_kind = behavior.kind;
		if (behavior.kind === 'state_machine') {
			draft.machine = structuredClone(behavior.definition);
		} else if (behavior.kind === 'timed_spawner') {
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

function particleDraft(settings: ParticleEmitterSettings): ComponentDraft['particle'] {
    return {
        rate_per_second: String(settings.rate_per_second),
        lifetime_seconds: String(settings.lifetime_seconds),
        speed: String(settings.speed),
        spread_degrees: String(settings.spread_degrees),
        gravity: String(settings.gravity),
        size: String(settings.size),
        seed: String(settings.seed),
        r: String(settings.color[0]),
        g: String(settings.color[1]),
        b: String(settings.color[2]),
    };
}

function particleSettings(draft: ComponentDraft['particle']): ParticleEmitterSettings {
    const value = {
        ...Object.fromEntries(PARTICLE_NUMBER_FIELDS.map(({key, label, min, max}) => [
            key,
            key === 'seed' ? uint(draft[key], label, min, max) : finite(draft[key], label, Math.fround(min), Math.fround(max)),
        ])),
        color: [finite(draft.r, 'Red', 0, 1), finite(draft.g, 'Green', 0, 1), finite(draft.b, 'Blue', 0, 1)],
    };
    if (!validParticleEmitterSettings(value)) throw new Error('Review the Particle Emitter settings.');
    return value;
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

export function selectedModelAsset(
	models: readonly ModelAsset[],
	path: string,
	previous?: ComponentRecord,
): ModelAsset | undefined {
	const existing = previous?.data;
	const retainedId = existing?.kind === 'mesh_renderer' && existing.model_path === path
		? existing.asset_id : null;
	return models.find(model => (!model.kind || model.kind === 'model') &&
		(retainedId ? model.asset_id === retainedId : model.path === path));
}

export function componentFromDraft(
	draft: ComponentDraft,
	collider: Collider,
	targets: readonly ComponentTarget[],
	models: readonly ModelAsset[],
	original?: ComponentRecord,
	scriptTypes: readonly ScriptDescriptor[] = [],
	scriptSlots: readonly string[] = [],
): ComponentRecord {
	const reference = (key: string, label: string): string => {
		if (!targets.some((target) => target.key === key))
			throw new Error(`Choose an existing ${label}.`);
		return key;
	};
	const optionalReference = (key: string, label: string) => (key ? reference(key, label) : null);
	let data: ComponentData;
	if (draft.kind === 'script') {
        const previous = original?.data.kind === 'script' ? original.data.settings : undefined;
        data = {kind: 'script', settings: scriptSettings(draft.script, scriptTypes, targets, previous, scriptSlots)};
    } else if (draft.kind === 'particle_emitter') {
        data = {kind: 'particle_emitter', settings: particleSettings(draft.particle)};
    } else if (draft.kind === 'light') {
        const {kind, color: {red, green, blue}, intensity, range: rangeText} = draft.light;
        const parsedRange = parseF32(rangeText);
        const range = parsedRange != null && parsedRange > 0 ? parsedRange : null;
        if (kind === 'point' && range == null) throw new Error('Light range must be positive.');
        data = {kind: 'light', settings: {
            kind, color: [finite(red, 'Red', 0, 1), finite(green, 'Green', 0, 1), finite(blue, 'Blue', 0, 1)],
            intensity: finite(intensity, 'Intensity', 0, 1_000_000), range: range ?? 10,
        }};
    } else if (draft.kind === 'sky') {
		data = {kind: 'sky', settings: skyComponentFromDraft(draft.sky)};
	} else if (draft.kind === 'audio_source') {
        const {clipId, gain, loop, playback, bus, spatial, near, far, eventKind, signal, eventClip, eventMarker, replacesCue} = draft.audioSource;
        if (clipId && !models.some(asset => asset.kind === 'audio_clip' && asset.asset_id === clipId)) {
            throw new Error('Assign an Audio Clip from this project.');
        }
        const distances = spatial === 'spatial' ?
            {mode: 'spatial' as const, near: finite(near, 'Near distance', 0, 10_000),
                far: finite(far, 'Far distance', 0, 10_000)} : undefined;
        if (distances && distances.far <= distances.near) throw new Error('Far distance must exceed near distance.');
        const event: AudioEventBinding | undefined = playback !== 'on_event' || eventKind === 'none' ? undefined :
            eventKind === 'gameplay_signal' ? {kind: eventKind, signal} :
                {kind: eventKind, clip: eventClip, marker: eventMarker};
        if (event?.kind === 'animation_marker' && (!eventClip || !eventMarker)) throw new Error('Choose a saved clip and marker.');
        if (event && (loop || bus !== 'effects')) throw new Error('Event audio uses Effects with Loop off.');
        if (event && replacesCue && !clipId) throw new Error('Assign an Audio Clip to replace a legacy cue.');
        data = {kind: 'audio_source', settings: {
            clip: clipId ? {asset: clipId} : null, gain: finite(gain, 'Gain', 0, 1), loop, playback, bus,
            ...(distances ? {spatial: distances} : {}), ...(event ? {event} : {}),
            ...(event && replacesCue ? {replaces_cue: replacesCue} : {}),
        }};
    } else if (draft.kind === 'mesh_renderer') {
		if (!relativeAssetPath(draft.model_path) || draft.model_path === '__no_mesh__')
			throw new Error('Choose an asset-relative model path.');
		const sourceMaterial = original?.data.kind === 'mesh_renderer' && original.data.material?.source_graph
			? original.data.material : null;
		const color: Vec4 = sourceMaterial
			? [sourceMaterial.base_color_factor[0], sourceMaterial.base_color_factor[1],
				sourceMaterial.base_color_factor[2], finite(draft.color.a, 'Material A', 0, 1)]
			: draft.tint
			? [
					finite(draft.color.r, 'Material R', 0, 1),
					finite(draft.color.g, 'Material G', 0, 1),
					finite(draft.color.b, 'Material B', 0, 1),
					finite(draft.color.a, 'Material A', 0, 1),
				]
			: [1, 1, 1, 1];
		const previous = original?.data.kind === 'mesh_renderer' && original.data.model_path === draft.model_path
			? original.data : undefined;
		const model = selectedModelAsset(models, draft.model_path, original);
		const mesh = model?.meshes?.find(candidate => candidate.id === draft.mesh_id);
		const assetId = model?.asset_id ?? previous?.asset_id ?? null;
		if (draft.mesh_id && (!assetId || (!mesh && draft.mesh_id !== previous?.mesh_id)))
			throw new Error('Choose a mesh from the selected model.');
		data = {
			kind: 'mesh_renderer',
			model_path: draft.model_path,
			asset_id: assetId,
			mesh_index: mesh?.index ?? (draft.mesh_index.trim()
				? uint(draft.mesh_index, 'Mesh index', 0, 1_000_000)
				: null),
			...(draft.mesh_id ? {mesh_id: draft.mesh_id} : {}),
			material: sourceMaterial ? {
				base_color_factor: color,
				source_graph: sourceMaterial.source_graph,
			} : draft.tint ? {base_color_factor: color} : null,
		};
	} else if (draft.kind === 'collider') data = { kind: 'collider', collider };
	else if (draft.kind === 'character_controller') {
		const numbers = Object.fromEntries(
			CONTROLLER_FIELDS.map((field) => [
				field.key,
				field.key.endsWith('_ticks')
					? uint(draft.controller[field.key], field.label, field.min, field.max)
					: finite(draft.controller[field.key], field.label, field.min, field.max),
			]),
		);
		if (!/^[A-Za-z0-9_-]{1,64}$/.test(draft.controller.jump_action))
			throw new Error('Choose the jump action ID from Game Input.');
		data = {
			kind: 'character_controller',
			settings: {
				...numbers,
				jump_action: draft.controller.jump_action,
			} as unknown as CharacterControllerSettings,
		};
	} else if (draft.kind === 'player_controller') {
		const {input, space, control_graph} = draft.playerController;
		if (space !== 'world' && space !== 'camera_relative') throw new Error('Choose a Player movement space.');
		const local = original?.data.kind === 'player_controller' && original.data.settings.local;
		data = {kind: 'player_controller', settings: {
			input, space, local: draft.enabled && local, ...(control_graph ? {control_graph} : {}),
		}};
	} else if (draft.kind === 'animation') {
		if (!validAnimationSettings(draft.animation))
			throw new Error(
				'Review an animation asset, choose its clips, and enter a valid speed ratio.',
			);
		data = { kind: 'animation', settings: structuredClone(draft.animation) };
	} else if (draft.kind === 'camera') {
		const settings = original?.data.kind === 'camera' ? original.data.settings : undefined;
		data = {kind: 'camera', settings: cameraComponentFromDraft(draft.camera, targets.map(target => target.key), settings)};
    } else if (draft.kind === 'interactable') {
        const {action, reach: reachText} = draft.interactable;
        const reach = parseF32(reachText);
        if (!validGameActionId(action)) throw new Error('Choose a valid game input action ID.');
        if (!validInteractionReach(reach)) throw new Error('Reach must be positive and at most 3 metres.');
        data = {kind: 'interactable', settings: {action, reach}};
	} else if (draft.behavior_kind === 'state_machine') {
		const problem = machineError(draft.machine);
		if (problem) throw new Error(problem);
		for (const input of draft.machine.inputs) {
			reference(input.actor, 'sensor actor');
			reference(input.sensor, 'sensor');
		}
		for (const state of draft.machine.states)
			for (const action of state.on_enter) {
				if (action.kind === 'set_visible' || action.kind === 'set_collision_enabled')
					reference(action.entity, 'action target');
			}
		for (const target of machineReferences(draft.machine))
			reference(target.entity, 'component target');
		data = {
			kind: 'native_behavior',
			behavior: { kind: 'state_machine', definition: structuredClone(draft.machine) },
		};
	} else if (draft.behavior_kind === 'key_door') {
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
