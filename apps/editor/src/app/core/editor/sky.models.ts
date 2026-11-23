import type {EntityKey, Vec3} from './editor.models';
import {validInputActionsId} from '../game-input/input-actions.models';

export type SkyBackground = {readonly kind: 'gradient'} | {readonly kind: 'panorama'; readonly image: string};

export interface SkySettings {
    readonly zenith: Vec3;
    readonly horizon: Vec3;
    readonly yaw_degrees: number;
    readonly background?: SkyBackground;
}

export interface SkyAttachment {
    readonly id: string;
    readonly enabled: boolean;
    readonly settings: SkySettings;
}

export interface SceneEnvironment {
    readonly active_sky?: EntityKey | null;
}

export interface SkyPanoramaAsset {
    readonly id: string;
    readonly path: string;
    readonly width: number;
    readonly height: number;
}

export interface SkyPanoramaScope {
    readonly workspaceGeneration: number;
    readonly nativeGeneration: number;
    readonly revision: number;
}

export const SKY_YAW_LIMIT = 360;
export const SKY_DEFAULT_ZENITH: Vec3 = [0.035, 0.075, 0.18];
export const SKY_DEFAULT_HORIZON: Vec3 = [0.3, 0.46, 0.65];

export function validSkyColor(value: unknown): value is Vec3 {
    return Array.isArray(value) && value.length === 3 && value.every(channel =>
        typeof channel === 'number' && Number.isFinite(channel) && channel >= 0 && channel <= 1);
}

export function validSkySettings(value: unknown): value is SkySettings {
    return record(value) && Object.keys(value).every(key =>
        key === 'zenith' || key === 'horizon' || key === 'yaw_degrees' || key === 'background') &&
        validSkyColor(value['zenith']) && validSkyColor(value['horizon']) &&
        typeof value['yaw_degrees'] === 'number' && Math.abs(value['yaw_degrees']) <= SKY_YAW_LIMIT &&
        (value['background'] === undefined || validSkyBackground(value['background']));
}

function validSkyBackground(value: unknown): value is SkyBackground {
    if (!record(value)) return false;
    return value['kind'] === 'gradient' && Object.keys(value).length === 1 ||
        value['kind'] === 'panorama' && Object.keys(value).length === 2 && validInputActionsId(value['image']);
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

export function validPanoramaPath(path: string): boolean {
    return path.length > 0 && path.length <= 1024 && !/[:\\\p{Cc}]/u.test(path) &&
        path.split('/').every(part => part !== '' && part !== '..' && part !== '.') &&
        path.toLowerCase().endsWith('.png');
}

export function readSkyPanoramas(value: unknown): readonly SkyPanoramaAsset[] {
    const seen = new Set<string>();
    if (!Array.isArray(value) || value.length > 4096 ||
        !value.every((asset: unknown) => {
            if (!record(asset) || !validInputActionsId(asset['id']) || seen.has(asset['id']) ||
                typeof asset['path'] !== 'string' || !validPanoramaPath(asset['path']) ||
                typeof asset['height'] !== 'number' || !Number.isInteger(asset['height']) ||
                asset['height'] < 1 || asset['height'] > 1024 || asset['width'] !== asset['height'] * 2) return false;
            seen.add(asset['id']);
            return true;
        })) {
        throw new Error('The editor returned an invalid sky panorama catalog.');
    }
    return value as readonly SkyPanoramaAsset[];
}

function record(value: unknown): value is Record<string, unknown> {
    return value !== null && typeof value === 'object' && !Array.isArray(value);
}
