export interface CharacterControllerSettings {
	readonly walk_speed: number;
	readonly ground_acceleration: number;
	readonly air_acceleration: number;
	readonly gravity: number;
	readonly max_fall_speed: number;
	readonly jump_speed: number;
	readonly ground_probe: number;
	readonly step_height: number;
	readonly coyote_ticks: number;
	readonly jump_buffer_ticks: number;
	readonly jump_action: string;
}
export interface CharacterControllerAttachment {
	readonly id: string;
	readonly enabled: boolean;
	readonly settings: CharacterControllerSettings;
}
export const CONTROLLER_FIELDS = [
	{ key: 'walk_speed', label: 'Walk speed', min: 0, max: 100, initial: 4 },
	{ key: 'ground_acceleration', label: 'Ground acceleration', min: 0.01, max: 1000, initial: 30 },
	{ key: 'air_acceleration', label: 'Air acceleration', min: 0, max: 1000, initial: 10 },
	{ key: 'gravity', label: 'Gravity', min: 0.01, max: 1000, initial: 20 },
	{ key: 'max_fall_speed', label: 'Maximum fall speed', min: 0.01, max: 1000, initial: 30 },
	{ key: 'jump_speed', label: 'Jump speed', min: 0.01, max: 100, initial: 7 },
	{ key: 'ground_probe', label: 'Ground probe distance', min: 0.001, max: 0.1, initial: 0.02 },
	{ key: 'step_height', label: 'Maximum box step', min: 0, max: 1, initial: 0.25 },
	{ key: 'coyote_ticks', label: 'Coyote window (ticks)', min: 0, max: 120, initial: 6 },
	{ key: 'jump_buffer_ticks', label: 'Jump buffer (ticks)', min: 0, max: 120, initial: 6 },
] as const;
export type ControllerNumberKey = (typeof CONTROLLER_FIELDS)[number]['key'];
export function validControllerSettings(value: unknown): value is CharacterControllerSettings {
	if (typeof value !== 'object' || value === null || Array.isArray(value)) return false;
	const settings = value as Record<string, unknown>;
	return (
		CONTROLLER_FIELDS.every(({ key, min, max }) => {
			const v = settings[key];
			return (
				typeof v === 'number' &&
				Number.isFinite(Math.fround(v)) &&
				v >= min &&
				v <= max &&
				(!key.endsWith('_ticks') || Number.isSafeInteger(v))
			);
		}) &&
		typeof settings['jump_action'] === 'string' &&
		/^[A-Za-z0-9_-]{1,64}$/.test(settings['jump_action'])
	);
}
