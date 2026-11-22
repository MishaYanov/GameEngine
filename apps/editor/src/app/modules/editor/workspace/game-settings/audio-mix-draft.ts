import type {SceneId} from '../../../../core/editor/editor.models';
import {
    type FeedbackBus,
    feedbackBuses,
    type FeedbackBusLevels,
    type FeedbackBusMix,
    maxSceneBusMixes,
    validFeedbackPercent,
} from '../../../../core/editor/feedback-audio.models';

type BusDraftLevels = Record<FeedbackBus, number>;

export interface AudioMixDraft {
    readonly category: BusDraftLevels;
    readonly scenes: readonly { readonly id: SceneId; readonly levels: BusDraftLevels }[];
}

const noMix: FeedbackBusMix = {bus_percent: {}, scene_bus_percent: {}};

function levels(table?: FeedbackBusLevels): BusDraftLevels {
    return {
        collection: table?.collection ?? 100,
        mechanisms: table?.mechanisms ?? 100,
        completion: table?.completion ?? 100,
    };
}

export function audioMixDraft(mix = noMix, sceneIds: readonly SceneId[] = []): AudioMixDraft {
    return {
        category: levels(mix.bus_percent),
        scenes: sceneIds.map(id => ({
            id,
            levels: levels(Object.hasOwn(mix.scene_bus_percent, id) ? mix.scene_bus_percent[id] : undefined),
        })),
    };
}

export function checkAudioMix(draft: AudioMixDraft): { mix: FeedbackBusMix } | { error: string } {
    const tables = [draft.category, ...draft.scenes.map(scene => scene.levels)];
    if (tables.some(table => feedbackBuses.some(bus => !validFeedbackPercent(table[bus])))) {
        return {error: 'Enter whole feedback levels from 0 to 100.'};
    }
    const overrides = (table: BusDraftLevels): FeedbackBusLevels =>
        Object.fromEntries(feedbackBuses.filter(bus => table[bus] !== 100).map(bus => [bus, table[bus]]));
    const rows = draft.scenes.flatMap(scene => {
        const authored = overrides(scene.levels);
        return Object.keys(authored).length ? [[scene.id, authored] as const] : [];
    });
    if (rows.length > maxSceneBusMixes) {
        return {error: `Set individual levels for at most ${maxSceneBusMixes} scenes.`};
    }
    return {mix: {bus_percent: overrides(draft.category), scene_bus_percent: Object.fromEntries(rows)}};
}
