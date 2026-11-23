import type {EntityKey, Vec3} from './editor.models';

export interface SkySettings {
    readonly zenith: Vec3;
    readonly horizon: Vec3;
    readonly yaw_degrees: number;
}

export interface SkyAttachment {
    readonly id: string;
    readonly enabled: boolean;
    readonly settings: SkySettings;
}

export interface SceneEnvironment {
    readonly active_sky?: EntityKey | null;
}

export const SKY_YAW_LIMIT = 360;
export const SKY_DEFAULT_ZENITH: Vec3 = [0.035, 0.075, 0.18];
export const SKY_DEFAULT_HORIZON: Vec3 = [0.3, 0.46, 0.65];

export function validSkyColor(value: unknown): value is Vec3 {
    return Array.isArray(value) && value.length === 3 && value.every(channel =>
        typeof channel === 'number' && Number.isFinite(channel) && channel >= 0 && channel <= 1);
}

export function validSkySettings(value: unknown): value is SkySettings {
    return record(value) && Object.keys(value).length === 3 &&
        validSkyColor(value['zenith']) && validSkyColor(value['horizon']) &&
        typeof value['yaw_degrees'] === 'number' && Math.abs(value['yaw_degrees']) <= SKY_YAW_LIMIT;
}

export function validSkyAttachment(value: unknown): value is SkyAttachment {
    return record(value) && typeof value['id'] === 'string' && /^[A-Za-z0-9_-]{1,128}$/.test(value['id']) &&
        typeof value['enabled'] === 'boolean' && validSkySettings(value['settings']);
}

export function validSceneEnvironment(value: unknown): value is SceneEnvironment {
    return record(value) && Object.keys(value).every(key => key === 'active_sky') &&
        (value['active_sky'] == null ||
            typeof value['active_sky'] === 'string' && value['active_sky'].length > 0 && !/\s/.test(value['active_sky']));
}

function record(value: unknown): value is Record<string, unknown> {
    return value !== null && typeof value === 'object' && !Array.isArray(value);
}
