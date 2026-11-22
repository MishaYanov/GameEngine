import type {EntityKey, SceneId} from './editor.models';
import type {PropAddress} from './linked-prefabs.models';
import {readAnimationPreview, type AnimationPreview} from './animation.models';

export type AnimationPreviewDocument =
    | {readonly kind: 'scene'; readonly scene_id: SceneId}
    | {readonly kind: 'source'; readonly asset_id: string; readonly draft_id: string; readonly draft_revision: number}
    | {readonly kind: 'instance'; readonly scene_id: SceneId; readonly root_key: EntityKey};

export interface AnimationWorkspaceTarget {
    readonly document: AnimationPreviewDocument;
    readonly entity_key: EntityKey;
    readonly component_id: string;
    readonly address?: PropAddress;
}

export interface ScopedAnimationPreview extends AnimationPreview {
    readonly target: AnimationWorkspaceTarget;
    readonly request_id: number;
}

export function readScopedAnimationPreview(value: unknown): ScopedAnimationPreview {
    const preview = readAnimationPreview(value);
    const scoped = value as Partial<ScopedAnimationPreview>;
    if (!scoped.target || typeof scoped.target !== 'object' ||
        !Number.isSafeInteger(scoped.request_id) || (scoped.request_id ?? 0) < 1) {
        throw new Error('Animation preview returned an invalid document scope.');
    }
    return {...preview, target: scoped.target, request_id: scoped.request_id!};
}

export function matchesAnimationTarget(actual: AnimationWorkspaceTarget, expected: AnimationWorkspaceTarget): boolean {
    const actualDocument = actual.document, expectedDocument = expected.document;
    if (!actualDocument || actualDocument.kind !== expectedDocument.kind ||
        actual.entity_key !== expected.entity_key || actual.component_id !== expected.component_id) return false;
    const sameAddress = !expected.address || JSON.stringify(actual.address) === JSON.stringify(expected.address);
    if (!sameAddress) return false;
    switch (expectedDocument.kind) {
        case 'source':
            return actualDocument.kind === 'source' && actualDocument.asset_id === expectedDocument.asset_id &&
                actualDocument.draft_id === expectedDocument.draft_id && actualDocument.draft_revision === expectedDocument.draft_revision;
        case 'instance':
            return actualDocument.kind === 'instance' && actualDocument.scene_id === expectedDocument.scene_id &&
                actualDocument.root_key === expectedDocument.root_key;
        case 'scene':
            return actualDocument.kind === 'scene' && actualDocument.scene_id === expectedDocument.scene_id;
    }
}
