export interface RuntimeHandle {
	readonly world_id: string;
	readonly key: string;
	readonly generation: number;
}

export interface RuntimeEntityInspection {
	readonly handle: RuntimeHandle;
	readonly name: string;
	readonly spawned: boolean;
	readonly instance_id: string | null;
	readonly behavior_states: readonly {
		readonly component_id: string;
		readonly kind: string;
		readonly activated: boolean;
		readonly elapsed_ticks: number;
		readonly machine_state?: string | null;
		readonly entered_step?: number | null;
	}[];
}

export interface RuntimeInspection {
	readonly play_session_id: string;
	readonly run_id: string;
	readonly scene_id: string;
	readonly tick: number;
	readonly topology_revision: number;
	readonly total_entities: number;
	readonly entities: readonly RuntimeEntityInspection[];
	readonly timer_count: number;
	readonly pending_spawns: number;
	readonly pending_events?: number;
	readonly event_sequence?: number;
	readonly machine_deadlines?: number;
	readonly stale_events_discarded?: number;
	readonly character_motion?: readonly CharacterMotion[];
	readonly animations?: readonly AnimationInspection[];
	readonly last_error: string | null;
}
export interface AnimationInspection {
	readonly entity: RuntimeHandle;
	readonly component_id: string;
	readonly clip_index: number;
	readonly elapsed_ticks: number;
	readonly mode: 'idle' | 'walk' | 'air';
	readonly playing: boolean;
	readonly blending: boolean;
}
export interface CharacterMotion {
	readonly entity: RuntimeHandle;
	readonly tick: number;
	readonly grounded: boolean;
	readonly planar_speed: number;
	readonly vertical_speed: number;
	readonly jumped_at_tick: number | null;
	readonly landed_at_tick: number | null;
}

function record(value: unknown): value is Record<string, unknown> {
	return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function integer(value: unknown): value is number {
	return typeof value === 'number' && Number.isSafeInteger(value) && value >= 0;
}

function text(value: unknown): value is string {
	return typeof value === 'string' && value.length > 0;
}
function motion(value: unknown): boolean {
	if (!record(value) || !record(value['entity'])) return false;
	const entity = value['entity'],
		tick = value['tick'];
	if (!integer(tick)) return false;
	return (
		text(entity['world_id']) &&
		text(entity['key']) &&
		integer(entity['generation']) &&
		typeof value['grounded'] === 'boolean' &&
		typeof value['planar_speed'] === 'number' &&
		Number.isFinite(value['planar_speed']) &&
		value['planar_speed'] >= 0 &&
		typeof value['vertical_speed'] === 'number' &&
		Number.isFinite(value['vertical_speed']) &&
		['jumped_at_tick', 'landed_at_tick'].every(
			(key) => value[key] === null || (integer(value[key]) && value[key] <= tick),
		)
	);
}
function animation(value: unknown): boolean {
	if (!record(value) || !record(value['entity'])) return false;
	const entity = value['entity'];
	return (
		text(entity['world_id']) &&
		text(entity['key']) &&
		integer(entity['generation']) &&
		text(value['component_id']) &&
		integer(value['clip_index']) &&
		value['clip_index'] < 32 &&
		integer(value['elapsed_ticks']) &&
		['idle', 'walk', 'air'].includes(String(value['mode'])) &&
		typeof value['playing'] === 'boolean' &&
		typeof value['blending'] === 'boolean'
	);
}

export function runtimeHandleKey(handle: RuntimeHandle): string {
	return JSON.stringify([handle.world_id, handle.key, handle.generation]);
}

/** Bound and validate a read-only page before making it visible in the editor. */
export function readRuntimeInspection(value: unknown, limit: number): RuntimeInspection {
	const valid =
		record(value) &&
		['play_session_id', 'run_id', 'scene_id'].every((key) => text(value[key])) &&
		['tick', 'topology_revision', 'total_entities', 'timer_count', 'pending_spawns'].every((key) =>
			integer(value[key]),
		) &&
		(value['last_error'] === null || typeof value['last_error'] === 'string') &&
		['pending_events', 'event_sequence', 'machine_deadlines', 'stale_events_discarded'].every(
			(key) => value[key] === undefined || integer(value[key]),
		) &&
		(value['character_motion'] === undefined ||
			(Array.isArray(value['character_motion']) &&
				value['character_motion'].length <= 64 &&
				value['character_motion'].every(motion))) &&
		(value['animations'] === undefined ||
			(Array.isArray(value['animations']) &&
				value['animations'].length <= 64 &&
				value['animations'].every(animation))) &&
		Array.isArray(value['entities']) &&
		value['entities'].length <= limit &&
		value['entities'].every((entity: unknown) => {
			if (!record(entity) || !record(entity['handle'])) return false;
			const handle = entity['handle'];
			return (
				text(handle['world_id']) &&
				text(handle['key']) &&
				integer(handle['generation']) &&
				typeof entity['name'] === 'string' &&
				typeof entity['spawned'] === 'boolean' &&
				(entity['instance_id'] === null || typeof entity['instance_id'] === 'string') &&
				Array.isArray(entity['behavior_states']) &&
				entity['behavior_states'].every(
					(state: unknown) =>
						record(state) &&
						text(state['component_id']) &&
						text(state['kind']) &&
						typeof state['activated'] === 'boolean' &&
						integer(state['elapsed_ticks']) &&
						(state['machine_state'] == null || text(state['machine_state'])) &&
						(state['entered_step'] == null || integer(state['entered_step'])),
				)
			);
		});
	if (!valid) throw new Error('The runtime returned an invalid inspection page.');
	const result = value as unknown as RuntimeInspection;
	if (
		result.entities.length > result.total_entities ||
		new Set(result.entities.map((entity) => runtimeHandleKey(entity.handle))).size !==
			result.entities.length
	) {
		throw new Error('The runtime returned an inconsistent inspection page.');
	}
	return result;
}
