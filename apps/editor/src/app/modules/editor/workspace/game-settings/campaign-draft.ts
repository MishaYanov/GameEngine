import type {CampaignDefinition, SceneDescriptor, SceneId} from '../../../../core/editor/editor.models';
import {
    type CourtyardSettings,
    MAX_CAMPAIGN_CHAPTERS,
    validChapterId,
    validChapterLabel,
} from './courtyard-settings';

export interface ChapterDraft {
    row: number;
    id: string;
    label: string;
    course_scene: SceneId;
}

export interface CampaignDraft {
    chapters: ChapterDraft[];
    finish_scene: SceneId;
}

export function configuredScenes(
    settings: CourtyardSettings | null, scenes: readonly SceneDescriptor[], role: 'course' | 'finish',
): readonly SceneDescriptor[] {
    return settings ? scenes.filter(scene => Object.hasOwn(settings.scenes, scene.id) &&
        settings.scenes[scene.id].kind === role) : [];
}

export function campaignDefinition(draft: CampaignDraft): CampaignDefinition {
    return {
        chapters: draft.chapters.map(({id, label, course_scene}) => ({id, label, course_scene})),
        finish_scene: draft.finish_scene,
    };
}

export function validateCampaignDraft(
    draft: CampaignDraft, courses: readonly SceneDescriptor[], finishes: readonly SceneDescriptor[],
): string | null {
    if (courses.length < 2 || finishes.length === 0) {
        return 'Use Interaction Settings to configure at least two Course scenes and one Finish.';
    }
    if (finishes.length > 1) return 'Keep only one configured Finish before applying a campaign.';
    if (draft.chapters.length < 2 || draft.chapters.length > MAX_CAMPAIGN_CHAPTERS) {
        return 'Use two to eight chapters.';
    }
    const ids = new Set<string>();
    const scenes = new Set<SceneId>();
    for (const [index, chapter] of draft.chapters.entries()) {
        if (!validChapterId(chapter.id) || ids.has(chapter.id)) {
            return 'Chapter ' + (index + 1) + ' needs a unique ID of 1–32 lowercase letters, digits, hyphens, or underscores.';
        }
        if (!validChapterLabel(chapter.label)) {
            return 'Chapter ' + (index + 1) + ' needs a readable label of 1–48 printable ASCII characters.';
        }
        if (!courses.some(scene => scene.id === chapter.course_scene) || scenes.has(chapter.course_scene)) {
            return 'Choose a different configured Course for each chapter.';
        }
        ids.add(chapter.id);
        scenes.add(chapter.course_scene);
    }
    if (courses.some(scene => !scenes.has(scene.id))) return 'Include every configured Course in the chapter order.';
    return finishes.some(scene => scene.id === draft.finish_scene) ? null : 'Choose the configured Finish.';
}
