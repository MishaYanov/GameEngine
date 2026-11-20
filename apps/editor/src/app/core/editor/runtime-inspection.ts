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
	readonly last_error: string | null;
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
						integer(state['elapsed_ticks']),
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
