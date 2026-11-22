import type {EntityKey, SceneId} from './editor.models';

export type SceneIssueScope = 'intro' | 'arena' | 'scene' | 'project';

export interface SceneIssue {
    readonly scope: SceneIssueScope;
    readonly scene_id: SceneId | null;
    readonly message: string;
    readonly role: string | null;
    readonly suggestion: { readonly entity_key: EntityKey; readonly entity_name: string } | null;
}

export interface CheckedScene {
    readonly id: SceneId;
    readonly content_stamp: string;
    readonly checked: boolean;
}

export interface SceneVerificationReport {
    readonly generation: number;
    readonly revision: number;
    readonly active_scene_id: SceneId;
    readonly scenes: readonly CheckedScene[];
    readonly issues: readonly SceneIssue[];
}

function record(value: unknown): value is Record<string, unknown> {
    return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function counter(value: unknown): value is number {
    return typeof value === 'number' && Number.isSafeInteger(value) && value >= 0;
}

function text(value: unknown): value is string {
    return typeof value === 'string' && value.length > 0;
}

function validScene(value: unknown): value is CheckedScene {
    return record(value) && text(value['id']) && typeof value['checked'] === 'boolean' &&
        typeof value['content_stamp'] === 'string' && /^[a-f0-9]{16}-[a-f0-9]{16}$/.test(value['content_stamp']);
}

function validIssue(value: unknown, ids: ReadonlySet<SceneId>): value is SceneIssue {
    if (!record(value) || !text(value['message'])) return false;
    const {scope, scene_id: sceneId, role, suggestion} = value;
    if (scope === 'project') return sceneId === null && role === null && suggestion === null;
    if (!(scope === 'intro' || scope === 'arena' || scope === 'scene') ||
        !text(sceneId) || !ids.has(sceneId) || !(role === null || text(role))) return false;
    return suggestion === null || (text(role) && record(suggestion) &&
        text(suggestion['entity_key']) && text(suggestion['entity_name']));
}

export function validSceneVerification(value: unknown): value is SceneVerificationReport {
    if (!record(value) || !counter(value['generation']) || !counter(value['revision']) || !text(value['active_scene_id'])) {
        return false;
    }
    const {scenes, issues} = value;
    if (!Array.isArray(scenes) || !scenes.every(validScene) || !Array.isArray(issues)) return false;
    const ids = new Set(scenes.map(scene => scene.id));
    return ids.size === scenes.length && ids.has(value['active_scene_id']) &&
        issues.every(issue => validIssue(issue, ids));
}
