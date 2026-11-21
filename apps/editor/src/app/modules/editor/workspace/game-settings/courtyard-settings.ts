import {validInteractionReach} from '../../../../core/editor/editor-components';
import {validGameActionId} from '../../../../core/game-input/game-input.models';
import type {CampaignDefinition} from '../../../../core/editor/editor.models';

export interface CourtyardRouteGate {
    readonly scene_id: string;
    readonly lever: string;
    readonly gate: string;
    readonly action: string;
    readonly radius: number;
}

export type CourtyardSceneSettings =
    | {readonly kind: 'course'; readonly key_trigger: string; readonly exit_trigger: string; readonly destination_scene: string; readonly exit_blocker?: string | null}
    | {readonly kind: 'finish'; readonly finish_trigger: string};

interface CourtyardSettingsBase {
    readonly scenes: Readonly<Record<string, CourtyardSceneSettings>>;
    readonly route_gate?: CourtyardRouteGate | null;
    readonly project_save_id?: string | null;
    readonly checkpoints?: readonly {
        readonly id: string;
        readonly scene_id: string;
        readonly player: string;
        readonly spawn: readonly [number, number, number];
        readonly activation_radius: number;
        readonly requires_key?: boolean;
        readonly requires_gate?: boolean;
    }[];
}

export type CourtyardSettings = CourtyardSettingsBase & (
    | { readonly schema_version: 1; readonly campaign?: null }
    | { readonly schema_version: 2; readonly campaign: CampaignDefinition }
    );

export const MAX_CAMPAIGN_CHAPTERS = 8;

function record(value: unknown): value is Record<string, unknown> {
    return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function identifier(value: unknown): value is string {
    return typeof value === 'string' && value.length > 0 && new TextEncoder().encode(value).length <= 256 &&
        !/[\s\p{Cc}]/u.test(value);
}

function keys(value: Record<string, unknown>, allowed: readonly string[]): boolean {
    return Object.keys(value).every(key => allowed.includes(key));
}

function within(value: unknown, min: number, max: number): boolean {
    return typeof value === 'number' && Number.isFinite(value) && value >= min && value <= max;
}

function route(value: unknown): boolean {
    return value == null || (record(value) && keys(value, ['scene_id', 'lever', 'gate', 'action', 'radius']) &&
        identifier(value['scene_id']) && identifier(value['lever']) && identifier(value['gate']) &&
        value['lever'] !== value['gate'] && validGameActionId(value['action']) && validInteractionReach(value['radius']));
}

function checkpoint(value: unknown): boolean {
    if (!record(value) || !keys(value, ['id', 'scene_id', 'player', 'spawn', 'activation_radius', 'requires_key', 'requires_gate'])) return false;
    const spawn: unknown = value['spawn'];
    return typeof value['id'] === 'string' && /^[A-Za-z0-9_-]{1,64}$/.test(value['id']) &&
        identifier(value['scene_id']) && identifier(value['player']) &&
        Array.isArray(spawn) && spawn.length === 3 && spawn.every(position => within(position, -1_000_000, 1_000_000)) &&
        within(value['activation_radius'], 0.01, 8) &&
        (value['requires_key'] === undefined || typeof value['requires_key'] === 'boolean') &&
        (value['requires_gate'] === undefined || typeof value['requires_gate'] === 'boolean');
}

export function validChapterId(value: unknown): value is string {
    return typeof value === 'string' && value.length >= 1 && value.length <= 32 && !/[^a-z0-9_-]/.test(value);
}

export function validChapterLabel(value: unknown): value is string {
    return typeof value === 'string' && value.length <= 48 && !/[^ -~]/.test(value) && value.trim().length > 0;
}

function validCampaign(value: unknown): value is CampaignDefinition {
    if (!record(value) || !keys(value, ['chapters', 'finish_scene']) || !identifier(value['finish_scene'])) return false;
    const chapters: unknown = value['chapters'];
    if (!Array.isArray(chapters) || chapters.length < 2 || chapters.length > MAX_CAMPAIGN_CHAPTERS) return false;
    const ids = new Set<string>();
    const scenes = new Set<string>();
    for (const chapter of chapters as readonly unknown[]) {
        if (!record(chapter) || !keys(chapter, ['id', 'label', 'course_scene']) ||
            !validChapterId(chapter['id']) || !validChapterLabel(chapter['label']) || !identifier(chapter['course_scene']) ||
            chapter['course_scene'] === value['finish_scene'] || ids.has(chapter['id']) || scenes.has(chapter['course_scene'])) return false;
        ids.add(chapter['id']);
        scenes.add(chapter['course_scene']);
    }
    return true;
}

export function readCourtyardSettings(value: unknown): CourtyardSettings | null {
    if (value === undefined || value === null) return {schema_version: 1, scenes: {}};
    if (!record(value) || !keys(value, ['schema_version', 'scenes', 'route_gate', 'project_save_id', 'checkpoints', 'campaign']) ||
        !record(value['scenes']) || !route(value['route_gate'])) return null;
    const campaign = value['campaign'];
    if (value['schema_version'] === 1 ? campaign != null :
        value['schema_version'] !== 2 || !validCampaign(campaign)) return null;
    const identity = value['project_save_id'];
    if (identity != null && (typeof identity !== 'string' ||
        !/^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/.test(identity))) return null;
    const checkpoints = value['checkpoints'];
    if (checkpoints !== undefined && (!Array.isArray(checkpoints) || !checkpoints.every(checkpoint))) return null;
    const valid = Object.entries(value['scenes']).every(([scene, settings]) => identifier(scene) && record(settings) &&
        (settings['kind'] === 'finish' ? keys(settings, ['kind', 'finish_trigger']) && identifier(settings['finish_trigger'])
            : settings['kind'] === 'course' && keys(settings, ['kind', 'key_trigger', 'exit_trigger', 'destination_scene', 'exit_blocker']) &&
            identifier(settings['key_trigger']) && identifier(settings['exit_trigger']) &&
            identifier(settings['destination_scene']) && (settings['exit_blocker'] == null || identifier(settings['exit_blocker']))));
    if (!valid) return null;
    const settings = value as unknown as CourtyardSettings;
    if (settings.schema_version === 2) {
        const {chapters, finish_scene} = settings.campaign;
        if (courtyardScene(settings, finish_scene)?.kind !== 'finish' ||
            Object.keys(settings.scenes).length !== chapters.length + 1 ||
            chapters.some(({course_scene}, index) => {
                const role = courtyardScene(settings, course_scene);
                return role?.kind !== 'course' ||
                    role.destination_scene !== (chapters[index + 1]?.course_scene ?? finish_scene);
            })) return null;
    }
    return settings;
}

export function courtyardScene(settings: CourtyardSettings, sceneId: string): CourtyardSceneSettings | undefined {
    return Object.hasOwn(settings.scenes, sceneId) ? settings.scenes[sceneId] : undefined;
}

export function updateCourtyardScene(
    settings: CourtyardSettings, sceneId: string, value: CourtyardSceneSettings | null,
): CourtyardSettings {
    const scenes = {...settings.scenes};
    if (value === null) delete scenes[sceneId];
    else Object.defineProperty(scenes, sceneId, {value, enumerable: true, configurable: true, writable: true});
    return {...settings, scenes};
}
