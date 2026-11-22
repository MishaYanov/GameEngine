import type {SceneEntity, SceneOrganization} from '../../../../core/editor/editor.models';

export interface SceneEntitiesModel {
    readonly projectOpen: boolean;
    readonly entities: readonly SceneEntity[];
    readonly organization?: SceneOrganization;
}
