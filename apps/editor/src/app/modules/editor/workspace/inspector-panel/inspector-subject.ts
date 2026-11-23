import {computed, effect, type Signal, signal, untracked} from '@angular/core';
import type {EditorSnapshot, EntityKey, SceneEntity, SceneId} from '../../../../core/editor/editor.models';
import type {EditorService, WorkspaceAssetScope} from '../../../../core/editor/editor.service';
import type {LinkedPropAddress, PrefabSourceDraft, PropAddress} from '../../../../core/editor/linked-prefabs.models';

export type InspectorAddress =
    | {readonly kind: 'scene'; readonly scene: SceneId; readonly object: EntityKey}
    | {readonly kind: 'prop'; readonly draftId: string; readonly sourceObject: EntityKey}
    | {readonly kind: 'linked'; readonly scene: SceneId; readonly root: EntityKey; readonly address: PropAddress};

export interface InspectorSubject {
    readonly address: InspectorAddress;
    readonly scope: WorkspaceAssetScope;
    readonly contentRevision: number;
    readonly access: 'scene-edit' | 'prop-edit' | 'linked-overrides' | 'read-only';
}

export interface LinkedInspectorTarget {
    readonly scene: SceneId;
    readonly root: EntityKey;
    readonly object: EntityKey;
    readonly scope: WorkspaceAssetScope;
}

export function sceneInspectorSubject(
    snapshot: EditorSnapshot, object: EntityKey, scope: WorkspaceAssetScope, editable: boolean,
): InspectorSubject {
    return {
        address: {kind: 'scene', scene: snapshot.active_scene_id, object},
        scope,
        contentRevision: snapshot.scene_revision,
        access: editable ? 'scene-edit' : 'read-only',
    };
}

export function sourceInspectorSubject(
    draft: PrefabSourceDraft, object: EntityKey, scope: WorkspaceAssetScope, editable: boolean,
): InspectorSubject | null {
    if (draft.workspace_generation !== scope.nativeGeneration ||
        !draft.prefab.entities.some(entity => entity.key === object)) return null;
    return {
        address: {kind: 'prop', draftId: draft.draft_id, sourceObject: object},
        scope,
        contentRevision: draft.draft_revision ?? draft.revision,
        access: editable && draft.draft_revision !== undefined ? 'prop-edit' : 'read-only',
    };
}

export function linkedInspectorSubject(
    snapshot: EditorSnapshot, target: LinkedInspectorTarget, result: LinkedPropAddress | null, editable: boolean,
): InspectorSubject | null {
    if (!result || result.entity_key !== target.object || result.root_key !== target.root ||
        result.scene_id !== target.scene || snapshot.active_scene_id !== target.scene ||
        snapshot.revision !== target.scope.revision) return null;
    return {
        address: {kind: 'linked', scene: target.scene, root: target.root, address: result.address},
        scope: target.scope,
        contentRevision: snapshot.scene_revision,
        access: editable ? 'linked-overrides' : 'read-only',
    };
}

/** Rendering identity is stable across revision updates and scoped to a workspace lifetime. */
export function inspectorAddressKey(
    address: InspectorAddress, workspaceGeneration: number, nativeGeneration: number | null,
): string {
    switch (address.kind) {
        case 'scene':
            return JSON.stringify([workspaceGeneration, nativeGeneration, address.kind, address.scene, address.object]);
        case 'prop':
            return JSON.stringify([workspaceGeneration, nativeGeneration, address.kind, address.draftId, address.sourceObject]);
        case 'linked': {
            const member = address.address;
            const source = member.kind === 'object' ? member.source_entity : null;
            return JSON.stringify([workspaceGeneration, nativeGeneration, address.kind, address.scene, address.root,
                member.kind, member.placement_path, source]);
        }
    }
}

/** The native scene validator requires all descendants of a linked root to be its mapped members. */
export function linkedInspectorRoot(entity: SceneEntity, entities: readonly SceneEntity[]): EntityKey | null {
    if (!entity.parent || entity.linked_prefab) return null;
    const byKey = new Map(entities.map(candidate => [candidate.key, candidate]));
    let parent = byKey.get(entity.parent);
    for (let depth = 0; parent && depth < entities.length; depth++) {
        if (parent.linked_prefab) return parent.key;
        parent = parent.parent ? byKey.get(parent.parent) : undefined;
    }
    return null;
}

export function sameLinkedInspectorTarget(a: LinkedInspectorTarget | null, b: LinkedInspectorTarget | null): boolean {
    return a === b || (!!a && !!b && a.scene === b.scene && a.root === b.root && a.object === b.object &&
        a.scope.workspaceGeneration === b.scope.workspaceGeneration && a.scope.nativeGeneration === b.scope.nativeGeneration &&
        a.scope.revision === b.scope.revision);
}

export function watchLinkedInspectorAddress(
    editor: Pick<EditorService, 'inspectLinkedPropAddress'> | null, target: Signal<LinkedInspectorTarget | null>,
): Signal<LinkedPropAddress | null> {
    const resolved = signal<{readonly target: LinkedInspectorTarget; readonly result: LinkedPropAddress | null} | null>(null);
    effect(onCleanup => {
        const capture = target();
        if (!editor || !capture) return;
        let active = true;
        onCleanup(() => { active = false; });
        untracked(() => {
            void editor.inspectLinkedPropAddress(capture.object, capture.scope).then(result => {
                if (active && target() === capture) resolved.set({target: capture, result});
            }).catch(() => {
                if (active && target() === capture) resolved.set(null);
            });
        });
    });
    return computed(() => {
        const reply = resolved();
        return reply && reply.target === target() ? reply.result : null;
    });
}
