export const feedbackCueEvents = ['key_collected', 'gate_opened', 'run_completed'] as const;
export type FeedbackCueEvent = typeof feedbackCueEvents[number];

export interface FeedbackCueAssignment {
    readonly event: FeedbackCueEvent;
    readonly path: string;
}

export interface FeedbackCueDraft {
    readonly workspace_generation: number;
    readonly catalog_digest: string | null;
    readonly cues: readonly FeedbackCueAssignment[];
}

export const validCueSlug = (slug: string): boolean =>
    slug.length >= 1 && slug.length <= 32 && !/[^a-z0-9-]/.test(slug);

export function validCuePath(path: unknown): path is string {
    return typeof path === 'string' && path.startsWith('audio/cue-') && path.endsWith('.wav') &&
        validCueSlug(path.slice('audio/cue-'.length, -'.wav'.length));
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
    return {workspace_generation: result.workspace_generation, catalog_digest: result.catalog_digest, cues};
}
