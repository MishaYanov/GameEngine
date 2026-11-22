import type {AnimationSettings} from '../../../../core/editor/animation.models';
import type {AnimationPreviewDocument, AnimationWorkspaceTarget} from '../../../../core/editor/animation-workspace.models';
import type {ComponentRecord} from '../../../../core/editor/editor-components';
import type {EntityKey, SceneEntity, SceneId} from '../../../../core/editor/editor.models';
import type {NestedPropPlacement, PrefabSourceEntity} from '../../../../core/editor/linked-prefabs.models';
import type {PropWorkspaceDocument, PropWorkspaceObject} from '../prop-workspace/prop-workspace.service';
import {readPrefabSource} from '../prefabs/prefab-source-values';

export interface AnimationTargetOption {
    readonly key: string;
    readonly name: string;
    readonly settings: AnimationSettings;
    readonly target: AnimationWorkspaceTarget;
}

export function sceneAnimationTarget(
    entity: SceneEntity | null, entities: readonly SceneEntity[], sceneId: SceneId,
): AnimationTargetOption | null {
    if (!entity?.animation || !entity.renderable) return null;
    const byKey = new Map(entities.map(object => [object.key, object]));
    let parent = entity.parent;
    let document: AnimationPreviewDocument = {kind: 'scene', scene_id: sceneId};
    for (let depth = 0; parent && depth < entities.length; depth++) {
        const owner = byKey.get(parent);
        if (owner?.linked_prefab) {
            document = {kind: 'instance', scene_id: sceneId, root_key: owner.key};
            break;
        }
        parent = owner?.parent;
    }
    return option(entity.name, entity.animation.settings, {
        document, entity_key: entity.key, component_id: entity.animation.id,
    });
}

export function propAnimationTargets(
    document: PropWorkspaceDocument, object: PropWorkspaceObject | null,
    placement: NestedPropPlacement | null, sceneId: SceneId,
): readonly AnimationTargetOption[] {
    if (document.kind === 'instance') {
        const target = object && objectOption(object, {kind: 'instance', scene_id: sceneId, root_key: document.root});
        return target ? [target] : [];
    }
    const source: AnimationPreviewDocument = {
        kind: 'source', asset_id: document.draft.asset_id, draft_id: document.draft.draft_id,
        draft_revision: document.draft.draft_revision ?? 0,
    };
    if (object) {
        const target = objectOption(object, source);
        return target ? [target] : [];
    }
    const composition = document.draft.prefab.composition;
    if (!placement || !composition) return [];
    const byVersion = new Map(composition.pins.map(pin => [
        pin.version.asset_id + ':' + pin.version.fingerprint, pin,
    ]));
    const frames = new Map<string, readonly PrefabSourceEntity[]>();
    const result: AnimationTargetOption[] = [];
    const visit = (item: NestedPropPlacement, path: readonly string[], labels: readonly string[]): void => {
        const version = item.target.asset_id + ':' + item.target.fingerprint;
        const pin = byVersion.get(version);
        if (!pin || path.length >= 8) return;
        const placementPath = [...path, item.id], names = [...labels, item.name];
        let child = frames.get(version);
        if (!child) {
            child = readPrefabSource({
                schema_version: 3, gameplay: null, entities: pin.entities, reference_slots: pin.reference_slots ?? {},
            }).entities;
            frames.set(version, child);
        }
        for (const entity of child) {
            const target = objectOption(entity, source, placementPath, names);
            if (target) result.push(target);
        }
        for (const nested of pin.placements ?? []) visit(nested, placementPath, names);
    };
    visit(placement, [], []);
    return result;
}

function objectOption(
    object: {readonly key: EntityKey; readonly name: string; readonly components: readonly ComponentRecord[]},
    document: AnimationPreviewDocument, path?: readonly string[], labels: readonly string[] = [],
): AnimationTargetOption | null {
    const component = object.components.find(entry => entry.data.kind === 'animation');
    if (!component || component.data.kind !== 'animation') return null;
    return option([...labels, object.name].join(' › '), component.data.settings, {
        document,
        entity_key: object.key,
        component_id: component.id,
        ...(path ? {address: {kind: 'object', placement_path: path, source_entity: object.key} as const} : {}),
    });
}

function option(name: string, settings: AnimationSettings, target: AnimationWorkspaceTarget): AnimationTargetOption {
    return {key: JSON.stringify(target), name, settings, target};
}
