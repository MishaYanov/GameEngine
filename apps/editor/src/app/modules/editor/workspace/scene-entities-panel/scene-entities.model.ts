import type {SceneEntity} from '../../../../core/editor/editor.models';

export interface SceneEntitiesModel {
    readonly projectOpen: boolean;
    readonly entities: readonly SceneEntity[];
}
