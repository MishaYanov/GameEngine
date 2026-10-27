import {InjectionToken} from '@angular/core';
import type {
    CreateObjectRequest,
    DeleteObjectRequest,
    DuplicatedObjects,
    DuplicateObjectRequest,
    ObjectApi,
    ObjectChange,
    ObjectPage,
    ObjectQuery,
    ObjectRef,
    ObjectResult,
    ObjectWatchRequest,
    ReparentObjectRequest,
    UpdateObjectRequest,
} from './object-api.models';
import type {EntityKey, SceneEntity} from '../editor/editor.models';

export class ObjectApiNotImplementedError extends Error {
    readonly code = 'not_implemented';

    constructor(readonly operation: keyof ObjectApi) {
        super(`Object API "${operation}" has no backend adapter yet.`);
        this.name = 'ObjectApiNotImplementedError';
    }
}

/** Replace OBJECT_API with a native adapter when object commands are wired. */
export class ObjectApiStub implements ObjectApi {
    get(_object: ObjectRef): Promise<ObjectResult<SceneEntity | null>> {
        return this.unavailable('get');
    }

    list(_query: ObjectQuery): Promise<ObjectResult<ObjectPage>> {
        return this.unavailable('list');
    }

    create(_request: CreateObjectRequest): Promise<ObjectResult<SceneEntity>> {
        return this.unavailable('create');
    }

    update(_request: UpdateObjectRequest): Promise<ObjectResult<SceneEntity>> {
        return this.unavailable('update');
    }

    delete(_request: DeleteObjectRequest): Promise<ObjectResult<readonly EntityKey[]>> {
        return this.unavailable('delete');
    }

    duplicate(_request: DuplicateObjectRequest): Promise<ObjectResult<DuplicatedObjects>> {
        return this.unavailable('duplicate');
    }

    reparent(_request: ReparentObjectRequest): Promise<ObjectResult<SceneEntity>> {
        return this.unavailable('reparent');
    }

    watch(
        _request: ObjectWatchRequest,
        _changed: (event: ObjectChange) => void,
    ): Promise<() => void> {
        return this.unavailable('watch');
    }

    private unavailable<T>(operation: keyof ObjectApi): Promise<T> {
        return Promise.reject(new ObjectApiNotImplementedError(operation));
    }
}

export const OBJECT_API = new InjectionToken<ObjectApi>('OBJECT_API', {
    providedIn: 'root',
    factory: () => new ObjectApiStub(),
});
