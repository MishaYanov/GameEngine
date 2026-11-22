export const feedbackCueEvents = ['key_collected', 'gate_opened', 'run_completed'] as const;
export type FeedbackCueEvent = typeof feedbackCueEvents[number];

export const feedbackBuses = ['collection', 'mechanisms', 'completion'] as const;
export type FeedbackBus = typeof feedbackBuses[number];
export type FeedbackBusLevels = Readonly<Partial<Record<FeedbackBus, number>>>;
export const maxSceneBusMixes = 16;

export interface FeedbackBusMix {
    readonly bus_percent: FeedbackBusLevels;
    readonly scene_bus_percent: Readonly<Record<string, FeedbackBusLevels>>;
}

export interface FeedbackCueAssignment {
    readonly event: FeedbackCueEvent;
    readonly path: string;
}

export interface FeedbackCueDraft {
    readonly workspace_generation: number;
    readonly catalog_digest: string | null;
    readonly cues: readonly FeedbackCueAssignment[];
    readonly mix: FeedbackBusMix;
}

export const validCueSlug = (slug: string): boolean =>
    slug.length >= 1 && slug.length <= 32 && !/[^a-z0-9-]/.test(slug);

export function validCuePath(path: unknown): path is string {
    return typeof path === 'string' && path.startsWith('audio/cue-') && path.endsWith('.wav') &&
        validCueSlug(path.slice('audio/cue-'.length, -'.wav'.length));
}

export function validFeedbackPercent(value: unknown): value is number {
    return typeof value === 'number' && Number.isInteger(value) && value >= 0 && value <= 100;
}

function tableEntries(value: unknown): [string, unknown][] {
    if (!value || typeof value !== 'object' || Array.isArray(value)) {
        throw new Error('The editor returned an invalid feedback audio mix.');
    }
    return Object.entries(value);
}

function readBusLevels(value: unknown): FeedbackBusLevels {
    const levels: Partial<Record<FeedbackBus, number>> = {};
    for (const [bus, percent] of tableEntries(value)) {
        if (!feedbackBuses.includes(bus as FeedbackBus) || !validFeedbackPercent(percent)) {
            throw new Error('The editor returned an invalid feedback audio level.');
        }
        levels[bus as FeedbackBus] = percent;
    }
    return levels;
}

function readBusMix(value: unknown): FeedbackBusMix {
    const tables = Object.fromEntries(tableEntries(value));
    const scenes = tableEntries(tables['scene_bus_percent']);
    if (Object.keys(tables).length !== 2 || scenes.length > maxSceneBusMixes) {
        throw new Error('The editor returned an invalid feedback audio mix.');
    }
    return {
        bus_percent: readBusLevels(tables['bus_percent']),
        scene_bus_percent: Object.fromEntries(scenes.map(([scene, levels]) => {
            if (scene === '' || new TextEncoder().encode(scene).length > 256 || /[\p{White_Space}\p{Cc}]/u.test(scene)) {
                throw new Error('The editor returned an invalid feedback audio scene.');
            }
            return [scene, readBusLevels(levels)];
        })),
    };
}

export function readFeedbackCueDraft(value: unknown): FeedbackCueDraft {
    const result = value as Partial<FeedbackCueDraft> | null;
    if (!result || typeof result.workspace_generation !== 'number' ||
        !Number.isSafeInteger(result.workspace_generation) || result.workspace_generation < 1 ||
        (result.catalog_digest !== null &&
            (typeof result.catalog_digest !== 'string' || result.catalog_digest.length !== 64 ||
                /[^a-f0-9]/.test(result.catalog_digest))) ||
        !Array.isArray(result.cues) || result.cues.length > feedbackCueEvents.length) {
        throw new Error('The editor returned an invalid feedback cue catalog.');
    }
    const seen = new Set<FeedbackCueEvent>();
    const cues: FeedbackCueAssignment[] = [];
    for (const cue of result.cues as readonly FeedbackCueAssignment[]) {
        if (!cue || !feedbackCueEvents.includes(cue.event) || seen.has(cue.event) || !validCuePath(cue.path)) {
            throw new Error('The editor returned an invalid feedback cue assignment.');
        }
        seen.add(cue.event);
        cues.push({event: cue.event, path: cue.path});
    }
    return {
        workspace_generation: result.workspace_generation,
        catalog_digest: result.catalog_digest,
        cues,
        mix: readBusMix(result.mix),
    };
}
