/** Authored tables only. Live queues and current states remain native runtime data. */
export const GAMEPLAY_SIGNALS = [
	'activated',
	'released',
	'collected',
	'reset',
	'jumped',
	'landed',
] as const;
export type GameplaySignal = (typeof GAMEPLAY_SIGNALS)[number];
export interface MachineDescriptor {
	readonly definition_version: number;
	readonly signals: readonly GameplaySignal[];
	readonly action_kinds: readonly MachineAction['kind'][];
	readonly max_states: number;
	readonly max_inputs: number;
	readonly max_transitions: number;
	readonly max_entry_actions: number;
	readonly max_timer_ticks: number;
	readonly max_definition_bytes: number;
}
/** This UI implements definition v1; reject incompatible native bounds instead of guessing. */
export const MACHINE_DESCRIPTOR: MachineDescriptor = {
	definition_version: 1,
	signals: GAMEPLAY_SIGNALS,
	action_kinds: ['emit', 'set_visible', 'set_collision_enabled', 'despawn_owner'],
	max_states: 32,
	max_inputs: 8,
	max_transitions: 128,
	max_entry_actions: 8,
	max_timer_ticks: 1_000_000,
	max_definition_bytes: 65536,
};
export function validMachineDescriptor(value: unknown): value is MachineDescriptor {
	if (!object(value)) return false;
	return Object.entries(MACHINE_DESCRIPTOR).every(([key, expected]) => {
		const actual = value[key];
		return Array.isArray(expected)
			? Array.isArray(actual) && expected.length === actual.length && expected.every((item) => actual.includes(item))
			: actual === expected;
	});
}
export interface ComponentReference {
	readonly entity: string;
	readonly component: string;
}
export type MachineTrigger =
	| {
			readonly kind: 'signal';
			readonly signal: GameplaySignal;
			readonly source: ComponentReference | null;
	  }
	| { readonly kind: 'after_ticks'; readonly ticks: number };
export type MachineAction =
	| { readonly kind: 'emit'; readonly target: ComponentReference; readonly signal: GameplaySignal }
	| { readonly kind: 'set_visible'; readonly entity: string; readonly visible: boolean }
	| { readonly kind: 'set_collision_enabled'; readonly entity: string; readonly enabled: boolean }
	| { readonly kind: 'despawn_owner' };
export interface MachineTransition {
	readonly when: MachineTrigger;
	readonly next: string;
}
export interface MachineState {
	readonly id: string;
	readonly on_enter: readonly MachineAction[];
	readonly transitions: readonly MachineTransition[];
}
export interface SensorInput {
	readonly id: string;
	readonly sensor: string;
	readonly actor: string;
	readonly signal: GameplaySignal;
}
export interface MachineDefinition {
	readonly schema_version: 1;
	readonly initial: string;
	readonly inputs: readonly SensorInput[];
	readonly states: readonly MachineState[];
}
export function defaultMachine(): MachineDefinition {
	return {
		schema_version: 1,
		initial: 'ready',
		inputs: [],
		states: [{ id: 'ready', on_enter: [], transitions: [] }],
	};
}
function object(value: unknown): value is Record<string, unknown> {
	return typeof value === 'object' && value !== null && !Array.isArray(value);
}
function id(value: unknown, max = 64): value is string {
	return typeof value === 'string' && value.length <= max && /^[A-Za-z0-9_-]+$/.test(value);
}
function entity(value: unknown): value is string {
	return typeof value === 'string' && value.length > 0 && value.length <= 128;
}
function signal(value: unknown): value is GameplaySignal {
	return GAMEPLAY_SIGNALS.some((item) => item === value);
}
function reference(value: unknown): value is ComponentReference {
	return object(value) && entity(value['entity']) && id(value['component'], 128);
}
function action(value: unknown): value is MachineAction {
	if (!object(value)) return false;
	switch (value['kind']) {
		case 'emit':
			return reference(value['target']) && signal(value['signal']);
		case 'set_visible':
			return entity(value['entity']) && typeof value['visible'] === 'boolean';
		case 'set_collision_enabled':
			return entity(value['entity']) && typeof value['enabled'] === 'boolean';
		case 'despawn_owner':
			return true;
		default:
			return false;
	}
}
function trigger(value: unknown): value is MachineTrigger {
	if (!object(value)) return false;
	if (value['kind'] === 'signal')
		return signal(value['signal']) && (value['source'] === null || reference(value['source']));
	return (
		value['kind'] === 'after_ticks' &&
		typeof value['ticks'] === 'number' &&
		Number.isSafeInteger(value['ticks']) &&
		value['ticks'] >= 1 &&
		value['ticks'] <= 1_000_000
	);
}
/** UI feedback; native validation remains authoritative for capabilities and ownership. */
export function machineError(value: unknown): string | null {
	if (
		!object(value) ||
		value['schema_version'] !== 1 ||
		!id(value['initial']) ||
		!Array.isArray(value['states']) ||
		value['states'].length < 1 ||
		value['states'].length > 32 ||
		!Array.isArray(value['inputs']) ||
		value['inputs'].length > 8
	)
		return 'Use 1–32 states and at most 8 sensor inputs.';
	const states = value['states'] as unknown[];
	const inputs = value['inputs'] as unknown[];
	const ids = new Set<string>();
	let transitions = 0;
	for (const state of states) {
		if (
			!object(state) ||
			!id(state['id']) ||
			ids.has(state['id']) ||
			!Array.isArray(state['on_enter']) ||
			state['on_enter'].length > 8 ||
			!state['on_enter'].every(action) ||
			!Array.isArray(state['transitions'])
		)
			return 'Use unique state IDs (1–64 letters, digits, _ or -) and up to 8 valid entry actions per state.';
		ids.add(state['id']);
		const seen: MachineTrigger[] = [];
		for (const item of state['transitions'] as unknown[]) {
			if (!object(item) || !id(item['next']) || !trigger(item['when']))
				return `Finish the transition fields in ${state['id']}. Timers require 1–1000000 ticks.`;
			const current = item['when'];
			for (const previous of seen) {
				if (current.kind === 'after_ticks' && previous.kind === 'after_ticks')
					return `State ${state['id']} permits one timer.`;
				if (
					current.kind === 'signal' &&
					previous.kind === 'signal' &&
					current.signal === previous.signal &&
					(!current.source ||
						!previous.source ||
						(current.source.entity === previous.source.entity &&
							current.source.component === previous.source.component))
				)
					return `State ${state['id']} has overlapping signal transitions.`;
			}
			seen.push(current);
			transitions++;
		}
	}
	if (transitions > 128) return 'A machine permits at most 128 transitions.';
	if (!ids.has(value['initial'])) return 'Choose an initial state that exists.';
	for (const state of states as unknown as readonly MachineState[]) {
		if (state.transitions.some((item) => !ids.has(item.next)))
			return `Choose existing transition destinations in ${state.id}.`;
	}
	const inputIds = new Set<string>();
	for (const input of inputs) {
		if (
			!object(input) ||
			!id(input['id']) ||
			inputIds.has(input['id']) ||
			!entity(input['actor']) ||
			!entity(input['sensor']) ||
			input['actor'] === input['sensor'] ||
			!signal(input['signal'])
		)
			return 'Sensor inputs need unique IDs, distinct actor/sensor entities, and a signal.';
		inputIds.add(input['id']);
	}
	if (new TextEncoder().encode(JSON.stringify(value)).length > 65536)
		return 'Machine definition exceeds 64 KiB.';
	return null;
}
export function validMachine(value: unknown): value is MachineDefinition {
	return machineError(value) === null;
}

export function machineReferences(value: MachineDefinition): readonly ComponentReference[] {
	return value.states.flatMap((state) => [
		...state.on_enter.flatMap((item) => (item.kind === 'emit' ? [item.target] : [])),
		...state.transitions.flatMap((item) =>
			item.when.kind === 'signal' && item.when.source ? [item.when.source] : [],
		),
	]);
}
