import type {EntityKey, SceneId} from './editor.models';
import {validInputActionsId} from '../game-input/input-actions.models';

export type SceneIssueScope = 'intro' | 'arena' | 'scene' | 'project';

export interface SceneIssue {
    readonly scope: SceneIssueScope;
    readonly scene_id: SceneId | null;
    readonly message: string;
    readonly role: string | null;
    readonly suggestion: { readonly entity_key: EntityKey; readonly entity_name: string } | null;
    readonly dependency?: SceneDependencyProblem | null;
}

export interface SceneDependencyOwner {
    readonly key: EntityKey;
    readonly name: string;
    readonly component_id?: string | null;
}

export type SceneDependencyAction =
    | {readonly kind: 'inspector'}
    | {readonly kind: 'prop'; readonly key: EntityKey}
    | {readonly kind: 'animation'; readonly key?: EntityKey | null}
    | {readonly kind: 'animator' | 'controls_graph'; readonly asset_id: string};

export interface SceneDependencyProblem {
    readonly code: string;
    readonly severity: 'warning' | 'error';
    readonly owner?: SceneDependencyOwner | null;
    readonly target?: {readonly asset_id?: string | null; readonly path?: string | null; readonly key?: EntityKey | null} | null;
    readonly open?: SceneDependencyAction | null;
}

export interface SceneDependencyRequest {
    readonly scene: SceneId;
    readonly generation: number;
    readonly nativeGeneration: number;
    readonly revision: number;
    readonly owner: SceneDependencyOwner;
    readonly action: SceneDependencyAction;
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
    if (!record(value) || !text(value['message']) ||
        value['dependency'] != null && !validDependency(value['dependency'])) return false;
    const {scope, scene_id: sceneId, role, suggestion} = value;
    if (scope === 'project') return sceneId === null && role === null && suggestion === null;
    if (!(scope === 'intro' || scope === 'arena' || scope === 'scene') ||
        !text(sceneId) || !ids.has(sceneId) || !(role === null || text(role))) return false;
    return suggestion === null || (text(role) && record(suggestion) &&
        text(suggestion['entity_key']) && text(suggestion['entity_name']));
}

function entityKey(value: unknown): value is EntityKey {
    return text(value) && value.length <= 1024 && !/\s/.test(value);
}

function projectRelativePath(value: unknown): boolean {
    return text(value) && value.length <= 1024 && !/[:\\\p{Cc}]/u.test(value) &&
        value.split('/').every(segment => segment !== '' && segment !== '.' && segment !== '..');
}

function validDependency(value: unknown): value is SceneDependencyProblem {
    if (!record(value) || !text(value['code']) || !/^[a-z][a-z0-9_-]{0,63}$/.test(value['code']) ||
        value['severity'] !== 'warning' && value['severity'] !== 'error') return false;
    const {owner, target, open} = value;
    if (owner != null && (!record(owner) || !entityKey(owner['key']) || !text(owner['name']) ||
        owner['name'].length > 1024 || owner['component_id'] != null &&
        (!text(owner['component_id']) || !/^[A-Za-z0-9_-]{1,128}$/.test(owner['component_id'])))) return false;
    if (target != null && (!record(target) || target['path'] != null && !projectRelativePath(target['path']) ||
        target['asset_id'] != null && !validInputActionsId(target['asset_id']) ||
        target['key'] != null && !entityKey(target['key']))) return false;
    if (open == null) return true;
    if (owner == null || !record(open)) return false;
    switch (open['kind']) {
        case 'inspector': return true;
        case 'prop': return entityKey(open['key']);
        case 'animation': return open['key'] == null || entityKey(open['key']);
        case 'animator':
        case 'controls_graph': return validInputActionsId(open['asset_id']);
        default: return false;
    }
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
