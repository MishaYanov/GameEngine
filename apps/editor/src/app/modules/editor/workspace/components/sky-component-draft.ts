import {parseF32} from '../../../../core/editor/authoring-number';
import type {Vec3} from '../../../../core/editor/editor.models';
import {
    SKY_DEFAULT_HORIZON, SKY_DEFAULT_ZENITH, SKY_YAW_LIMIT, type SkySettings, validSkyColor,
} from '../../../../core/editor/sky.models';

export interface SkyColorDraft {
    readonly red: string;
    readonly green: string;
    readonly blue: string;
}

export interface SkyComponentDraft {
    readonly zenith: SkyColorDraft;
    readonly horizon: SkyColorDraft;
    readonly yawDegrees: string;
}

export const skyColorChannels = [
    {key: 'red', label: 'Red'},
    {key: 'green', label: 'Green'},
    {key: 'blue', label: 'Blue'},
] as const;

export function skyColorDraft(color: Vec3): SkyColorDraft {
    return {red: String(color[0]), green: String(color[1]), blue: String(color[2])};
}

export function skyComponentDraft(settings?: SkySettings): SkyComponentDraft {
    return {
        zenith: skyColorDraft(settings?.zenith ?? SKY_DEFAULT_ZENITH),
        horizon: skyColorDraft(settings?.horizon ?? SKY_DEFAULT_HORIZON),
        yawDegrees: String(settings?.yaw_degrees ?? 0),
    };
}

export function skyColorFromDraft(draft: SkyColorDraft, label: string): Vec3 {
    const channels = [parseF32(draft.red), parseF32(draft.green), parseF32(draft.blue)];
    if (!validSkyColor(channels)) {
        throw new Error(`${label} requires three linear color values from 0 to 1.`);
    }
    return channels;
}

export function skyComponentFromDraft(draft: SkyComponentDraft): SkySettings {
    const zenith = skyColorFromDraft(draft.zenith, 'Zenith color');
    const horizon = skyColorFromDraft(draft.horizon, 'Horizon color');
    const yaw = parseF32(draft.yawDegrees);
    if (yaw === null || Math.abs(yaw) > SKY_YAW_LIMIT) {
        throw new Error(`Yaw must be between −${SKY_YAW_LIMIT} and ${SKY_YAW_LIMIT} degrees.`);
    }
    return {zenith, horizon, yaw_degrees: yaw};
}
