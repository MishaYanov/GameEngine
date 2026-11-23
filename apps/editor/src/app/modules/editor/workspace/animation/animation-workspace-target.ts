import type {AnimationSettings} from '../../../../core/editor/animation.models';
import type {AnimationPreviewDocument, AnimationWorkspaceTarget} from '../../../../core/editor/animation-workspace.models';
import type {ComponentRecord} from '../../../../core/editor/editor-components';
import type {EntityKey, SceneEntity, SceneId} from '../../../../core/editor/editor.models';
import type {NestedPropPlacement, PrefabSourceEntity} from '../../../../core/editor/linked-prefabs.models';
import type {PropWorkspaceDocument, PropWorkspaceObject} from '../prop-workspace/prop-workspace.service';
import {readPrefabSource} from '../prefabs/prefab-source-values';
import type {InspectorAddress} from '../inspector-panel/inspector-subject';

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

/** Resolves a selected component without letting the visible tab choose another object. */
export function boundAnimationTarget(
    address: InspectorAddress, componentId: string, sceneId: SceneId, sceneSelection: SceneEntity | null,
    entities: readonly SceneEntity[], prop: PropWorkspaceDocument | null, propObject: PropWorkspaceObject | null,
    propVisible: boolean,
): {readonly option: AnimationTargetOption; readonly scope: 'scene' | 'prop'} | null {
    if (address.kind === 'prop') {
        const option = prop?.kind === 'source' && prop.draft.draft_id === address.draftId &&
            propObject?.key === address.sourceObject
            ? objectOption(propObject, {kind: 'source', asset_id: prop.draft.asset_id, draft_id: address.draftId,
                draft_revision: prop.draft.draft_revision ?? 0})
            : null;
        return option?.target.component_id === componentId ? {option, scope: 'prop'} : null;
    }
    if (address.scene !== sceneId) return null;
    if (address.kind === 'scene') {
        const option = sceneSelection?.key === address.object
            ? sceneAnimationTarget(sceneSelection, entities, sceneId) : null;
        return option?.target.component_id === componentId && option.target.document.kind === 'scene'
            ? {option, scope: 'scene'} : null;
    }
    if (address.address.kind !== 'object') return null;
    const useProp = propVisible && prop?.kind === 'instance' && prop.root === address.root;
    const option = useProp && propObject
        ? objectOption(propObject, {kind: 'instance', scene_id: sceneId, root_key: address.root})
        : sceneAnimationTarget(sceneSelection, entities, sceneId);
    if (option?.target.component_id !== componentId || option.target.document.kind !== 'instance' ||
        option.target.document.root_key !== address.root) return null;
    return {option: {...option, target: {...option.target, address: address.address}}, scope: useProp ? 'prop' : 'scene'};
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
