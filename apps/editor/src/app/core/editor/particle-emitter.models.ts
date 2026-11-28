import {validInputActionsId} from '../game-input/input-actions.models';

export interface ParticleEmitterSettings {
    readonly rate_per_second: number;
    readonly lifetime_seconds: number;
    readonly speed: number;
    readonly spread_degrees: number;
    readonly gravity: number;
    readonly size: number;
    readonly color: readonly [number, number, number];
    readonly seed: number;
    readonly sprite_asset_id?: string;
    readonly linger_on_despawn?: boolean;
}

export interface ParticleEmitterComponent {
    readonly id: string;
    readonly enabled: boolean;
    readonly settings: ParticleEmitterSettings;
}

export interface ParticleBurstSettings {
    readonly count: number;
    readonly settings: ParticleEmitterSettings;
}

export const DEFAULT_PARTICLE_EMITTER: ParticleEmitterSettings = {
    rate_per_second: 32,
    lifetime_seconds: 1.8,
    speed: 2,
    spread_degrees: 28,
    gravity: 0.5,
    size: 0.15,
    color: [1, 0.35, 0.08],
    seed: 1,
};

export const PARTICLE_NUMBER_FIELDS = [
    {key: 'rate_per_second', label: 'Emission rate (per second)', min: 0, max: 128},
    {key: 'lifetime_seconds', label: 'Lifetime (seconds)', min: 0.05, max: 10},
    {key: 'speed', label: 'Launch speed', min: 0, max: 40},
    {key: 'spread_degrees', label: 'Spread (degrees)', min: 0, max: 90},
    {key: 'gravity', label: 'Vertical acceleration', min: -30, max: 30},
    {key: 'size', label: 'Initial size', min: 0.01, max: 2},
    {key: 'seed', label: 'Random seed', min: 0, max: 4_294_967_295},
] as const;

export type ParticleNumberKey = typeof PARTICLE_NUMBER_FIELDS[number]['key'];

export function validParticleEmitterSettings(value: unknown): value is ParticleEmitterSettings {
    if (!isRecord(value) || Object.keys(value).some(key => key !== 'color' &&
        key !== 'sprite_asset_id' && key !== 'linger_on_despawn' && !PARTICLE_NUMBER_FIELDS.some(field => field.key === key))) return false;
    const color = value['color'];
    return PARTICLE_NUMBER_FIELDS.every(({key, min, max}) => {
        const field = value[key];
        return key === 'seed'
            ? typeof field === 'number' && Number.isInteger(field) && field >= min && field <= max
            : inRange(field, min, max);
        }) && Array.isArray(color) && color.length === 3 && color.every(channel => inRange(channel, 0, 1)) &&
        (value['sprite_asset_id'] === undefined || validInputActionsId(value['sprite_asset_id'])) &&
        (value['linger_on_despawn'] === undefined || typeof value['linger_on_despawn'] === 'boolean');
}

export function validParticleEmitterComponent(value: unknown): value is ParticleEmitterComponent {
    return isRecord(value) && Object.keys(value).length === 3 &&
        typeof value['id'] === 'string' && /^[A-Za-z0-9_-]{1,128}$/.test(value['id']) &&
        typeof value['enabled'] === 'boolean' && validParticleEmitterSettings(value['settings']);
}

export function validParticleBurstSettings(value: unknown): value is ParticleBurstSettings {
    return isRecord(value) && Object.keys(value).length === 2 && typeof value['count'] === 'number' &&
        Number.isInteger(value['count']) && value['count'] >= 1 && value['count'] <= 192 &&
        validParticleEmitterSettings(value['settings']);
}

function inRange(value: unknown, minimum: number, maximum: number): boolean {
    if (typeof value !== 'number') return false;
    const float = Math.fround(value);
    return float >= Math.fround(minimum) && float <= Math.fround(maximum);
}

function isRecord(value: unknown): value is Record<string, unknown> {
    return typeof value === 'object' && value !== null && !Array.isArray(value);
}
