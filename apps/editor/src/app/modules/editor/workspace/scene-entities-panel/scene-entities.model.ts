import type {SceneEntity, SceneOrganization} from '../../../../core/editor/editor.models';

export type HierarchyEntity = Pick<SceneEntity,
    'key' | 'name' | 'parent' | 'model_path' | 'linked_prefab' | 'player_controller' | 'camera'>;

export interface SceneEntitiesModel {
    readonly projectOpen: boolean;
    readonly entities: readonly HierarchyEntity[];
    readonly organization?: SceneOrganization;
}

export function sameHierarchyScene(previous: SceneEntitiesModel, next: SceneEntitiesModel): boolean {
    return previous.projectOpen === next.projectOpen && previous.organization === next.organization &&
        (previous.entities === next.entities || previous.entities.length === next.entities.length &&
            previous.entities.every((entity, index) => {
                const candidate = next.entities[index];
                return entity === candidate || entity.key === candidate.key && entity.name === candidate.name &&
                    entity.model_path === candidate.model_path && (entity.parent ?? null) === (candidate.parent ?? null) &&
                    Boolean(entity.linked_prefab) === Boolean(candidate.linked_prefab) &&
                    Boolean(entity.player_controller) === Boolean(candidate.player_controller) &&
                    Boolean(entity.camera) === Boolean(candidate.camera);
            }));
}
