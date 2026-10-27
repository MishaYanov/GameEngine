import type {EntityKey, EntitySpec, SceneEntity, SceneId, Vec3} from '../editor/editor.models';

/** Entity keys are scoped to a scene. Names are editable and are never identifiers. */
export interface ObjectRef {
    readonly sceneId: SceneId;
    readonly key: EntityKey;
}

export interface ObjectQuery {
    readonly sceneId: SceneId;
    /** Omit for all parents; null selects root objects. */
    readonly parent?: EntityKey | null;
    readonly search?: string;
    readonly cursor?: string;
    readonly limit?: number;
}

export interface ObjectResult<T> {
    readonly value: T;
    /** Uses the same revision as EditorSnapshot.revision. */
    readonly revision: number;
}

export interface ObjectPage {
    readonly objects: readonly SceneEntity[];
    readonly nextCursor: string | null;
}

export interface ObjectWriteContext {
    readonly sceneId: SceneId;
    /** A backend must reject stale writes instead of silently overwriting newer edits. */
    readonly expectedRevision: number;
}

export interface CreateObjectRequest extends ObjectWriteContext {
    readonly object: EntitySpec;
}

/** Identity and parenting have dedicated operations; omitted fields remain unchanged. */
export type ObjectPatch = Partial<
    Pick<
        SceneEntity,
        'name' | 'model_path' | 'renderable' | 'transform' | 'material' | 'collider' | 'mesh_index'
    >
>;

export interface UpdateObjectRequest extends ObjectWriteContext {
    readonly key: EntityKey;
    readonly patch: ObjectPatch;
}

export interface DeleteObjectRequest extends ObjectWriteContext {
    readonly key: EntityKey;
    /** False rejects deletion when children exist; true deletes the subtree atomically. */
    readonly includeChildren: boolean;
}

export interface DuplicateObjectRequest extends ObjectWriteContext {
    readonly key: EntityKey;
    readonly includeChildren: boolean;
    readonly offset: Vec3;
}

export interface DuplicatedObjects {
    readonly root: EntityKey;
    readonly objects: readonly SceneEntity[];
    readonly keyMap: Readonly<Record<EntityKey, EntityKey>>;
}

export interface ReparentObjectRequest extends ObjectWriteContext {
    readonly key: EntityKey;
    readonly parent: EntityKey | null;
    readonly keepWorldTransform: boolean;
}

export interface ObjectWatchRequest {
    readonly sceneId: SceneId;
    readonly afterRevision?: number;
}

export type ObjectChange = {
    readonly sceneId: SceneId;
    readonly revision: number;
} & (
    | { readonly type: 'created' | 'updated'; readonly objects: readonly SceneEntity[] }
    | { readonly type: 'deleted'; readonly keys: readonly EntityKey[] }
    /** A reset replaces local object state after an event gap or reconnection. */
    | { readonly type: 'reset'; readonly objects: readonly SceneEntity[] }
    );

export interface ObjectApi {
    get(object: ObjectRef): Promise<ObjectResult<SceneEntity | null>>;

    list(query: ObjectQuery): Promise<ObjectResult<ObjectPage>>;

    create(request: CreateObjectRequest): Promise<ObjectResult<SceneEntity>>;

    update(request: UpdateObjectRequest): Promise<ObjectResult<SceneEntity>>;

    delete(request: DeleteObjectRequest): Promise<ObjectResult<readonly EntityKey[]>>;

    duplicate(request: DuplicateObjectRequest): Promise<ObjectResult<DuplicatedObjects>>;

    reparent(request: ReparentObjectRequest): Promise<ObjectResult<SceneEntity>>;

    watch(request: ObjectWatchRequest, changed: (event: ObjectChange) => void): Promise<() => void>;
}
