export type AudioSourcePlayback = 'on_scene_enter' | 'on_event';
export type AudioSourceBus = 'effects' | 'music';

export interface AudioSourceSettings {
    readonly clip: {readonly asset: string} | null;
    readonly gain: number;
    readonly loop: boolean;
    readonly playback: AudioSourcePlayback;
    readonly bus: AudioSourceBus;
}

export type AudioAuditionOwner =
    | {readonly kind: 'scene'; readonly scene_id: string; readonly key: string}
    | {readonly kind: 'prop'; readonly preview_token: string; readonly key: string};

export interface AudioAuditionScope {
    readonly owner: AudioAuditionOwner;
    readonly generation: number;
    readonly nativeGeneration: number;
    readonly revision: number;
    readonly contentRevision: number;
}

export interface AudioAuditionReply {
    readonly token: string;
    readonly bytes: readonly number[];
    readonly gain: number;
}

export const maxAudioAuditionWaveSize = 64_044;

export function validAudioIdentity(value: unknown): value is string {
    return typeof value === 'string' && /^[0-9a-f]{8}-(?:[0-9a-f]{4}-){3}[0-9a-f]{12}$/.test(value) &&
        value !== '00000000-0000-0000-0000-000000000000';
}

function record(value: unknown): value is Record<string, unknown> {
    return !!value && typeof value === 'object' && !Array.isArray(value);
}

function validGain(value: unknown): value is number {
    return typeof value === 'number' && Number.isFinite(value) && value >= 0 && value <= 1;
}

export function validAudioSourceSettings(value: unknown): value is AudioSourceSettings {
    if (!record(value) || Object.keys(value).length !== 5) return false;
    const {clip, gain, loop, playback, bus} = value;
    return (clip === null || (record(clip) && Object.keys(clip).length === 1 && validAudioIdentity(clip['asset']))) &&
        validGain(gain) && typeof loop === 'boolean' &&
        (playback === 'on_scene_enter' || playback === 'on_event') && (bus === 'effects' || bus === 'music');
}

export function readAudioAuditionReply(value: unknown): AudioAuditionReply {
    if (!record(value) || !validAudioIdentity(value['token']) || !validGain(value['gain']) ||
        !Array.isArray(value['bytes']) || value['bytes'].length < 46 || value['bytes'].length > maxAudioAuditionWaveSize) {
        throw new Error('The editor returned invalid Audio Source audition data.');
    }
    return {token: value['token'], bytes: value['bytes'], gain: value['gain']};
}
