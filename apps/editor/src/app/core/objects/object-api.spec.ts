import {TestBed} from '@angular/core/testing';
import {OBJECT_API, ObjectApiNotImplementedError, ObjectApiStub} from './object-api';
import type {ObjectApi} from './object-api.models';

describe('Object API contract', () => {
    it('rejects every stub operation explicitly instead of reporting a successful write', async () => {
        const api = TestBed.inject(OBJECT_API);
        const context = {sceneId: 'main', expectedRevision: 4};
        const target = {...context, key: 'cube'};
        const calls: readonly [keyof ObjectApi, () => Promise<unknown>][] = [
            ['get', () => api.get(target)],
            ['list', () => api.list({sceneId: 'main', parent: null, limit: 50})],
            [
                'create',
                () => api.create({...context, object: {name: 'Cube', model_path: 'cube.glb'}}),
            ],
            ['update', () => api.update({...target, patch: {name: 'Renamed'}})],
            ['delete', () => api.delete({...target, includeChildren: true})],
            ['duplicate', () => api.duplicate({...target, includeChildren: true, offset: [0, 0, 0]})],
            ['reparent', () => api.reparent({...target, parent: null, keepWorldTransform: true})],
            ['watch', () => api.watch({sceneId: 'main', afterRevision: 4}, () => undefined)],
        ];
        for (const [operation, call] of calls) {
            await expect(call()).rejects.toBeInstanceOf(ObjectApiNotImplementedError);
            await expect(call()).rejects.toMatchObject({code: 'not_implemented', operation});
        }
    });

    it('accepts a replacement adapter through the public injection token', async () => {
        class Adapter extends ObjectApiStub {
            override get() {
                return Promise.resolve({value: null, revision: 5});
            }
        }

        const adapter: ObjectApi = new Adapter();
        TestBed.configureTestingModule({providers: [{provide: OBJECT_API, useValue: adapter}]});
        expect(TestBed.inject(OBJECT_API)).toBe(adapter);
        expect(await TestBed.inject(OBJECT_API).get({sceneId: 'main', key: 'missing'})).toEqual({
            value: null,
            revision: 5,
        });
    });
});
