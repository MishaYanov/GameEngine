export interface AudioClipReference {
    readonly asset_id: string;
    readonly path: string;
}

export interface AudioClipDetails {
    readonly sample_rate_hz: number;
    readonly frames: number;
    readonly channels: number;
    readonly references: readonly AudioClipReference[];
}

export function readAudioClipDetails(value: unknown): AudioClipDetails {
    if (!value || typeof value !== 'object') throw new Error('The editor returned invalid Audio Clip details.');
    const details = value as Partial<AudioClipDetails>;
    const {sample_rate_hz, frames, channels, references} = details;
    if (sample_rate_hz !== 16000 || channels !== 1 || typeof frames !== 'number' ||
        !Number.isSafeInteger(frames) || frames < 1 || frames > 480000 ||
        !Array.isArray(references) || references.length > 4096 || !references.every(isReference)) {
        throw new Error('The editor returned invalid Audio Clip details.');
    }
    return {sample_rate_hz, frames, channels, references};
}

function isReference(value: unknown): value is AudioClipReference {
    if (!value || typeof value !== 'object') return false;
    const reference = value as Partial<AudioClipReference>;
    return typeof reference.asset_id === 'string' && typeof reference.path === 'string';
}
