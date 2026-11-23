export type AudioSourcePlayback = 'on_scene_enter' | 'on_event';
export type AudioSourceBus = 'effects' | 'music';

export type AudioSourceSpatial = {readonly mode: 'two_d'} | {
    readonly mode: 'spatial';
    readonly near: number;
    readonly far: number;
};

export const audioGameplaySignals = ['activated', 'released', 'collected', 'reset', 'jumped', 'landed'] as const;
export const legacyCombatCues = ['swing', 'hit', 'hurt', 'victory', 'defeat'] as const;
export type AudioGameplaySignal = typeof audioGameplaySignals[number];
export type LegacyCombatCue = typeof legacyCombatCues[number];
export type AudioEventBinding =
    | {readonly kind: 'gameplay_signal'; readonly signal: AudioGameplaySignal}
    | {readonly kind: 'animation_marker'; readonly clip: string; readonly marker: string};

export interface AudioSourceSettings {
    readonly clip: {readonly asset: string} | null;
    readonly gain: number;
    readonly loop: boolean;
    readonly playback: AudioSourcePlayback;
    readonly bus: AudioSourceBus;
    readonly spatial?: AudioSourceSpatial;
    readonly event?: AudioEventBinding;
    readonly replaces_cue?: LegacyCombatCue;
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

function validSpatial(value: unknown): value is AudioSourceSpatial {
    if (!record(value)) return false;
    const {mode, near, far} = value;
    const count = Object.keys(value).length;
    if (mode === 'two_d') return count === 1;
    return mode === 'spatial' && count === 3 && typeof near === 'number' && typeof far === 'number' &&
        Number.isFinite(near) && Number.isFinite(far) && near >= 0 && far > near && far <= 10_000;
}

function validEvent(value: unknown): value is AudioEventBinding {
    if (!record(value)) return false;
    const {kind, clip, marker, signal} = value;
    const count = Object.keys(value).length;
    return kind === 'gameplay_signal'
        ? count === 2 && audioGameplaySignals.some(candidate => candidate === signal)
        : kind === 'animation_marker' && count === 3 && validAudioIdentity(clip) &&
            typeof marker === 'string' && /^[a-zA-Z0-9_-]{1,128}$/.test(marker);
}

export function validAudioSourceSettings(value: unknown): value is AudioSourceSettings {
    if (!record(value)) return false;
    const {clip, gain, loop, playback, bus, spatial, event, replaces_cue} = value;
    const optionalCount = [spatial, event, replaces_cue].filter(entry => entry !== undefined).length;
    return Object.keys(value).length === 5 + optionalCount &&
        (event === undefined || (validEvent(event) && playback === 'on_event' && loop === false && bus === 'effects')) &&
        (replaces_cue === undefined || (event !== undefined && clip !== null && legacyCombatCues.some(cue => cue === replaces_cue))) &&
        (spatial === undefined || validSpatial(spatial)) &&
        (clip === null || (record(clip) && Object.keys(clip).length === 1 && validAudioIdentity(clip['asset']))) &&
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
