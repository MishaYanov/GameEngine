import type {NativeBehaviorSpec} from '../../../../core/editor/editor-components';
import type {SceneDocument, SceneEntity} from '../../../../core/editor/editor.models';
import {collectSceneReferences} from './scene-references';

const entity = (key: string, overrides: Partial<SceneEntity> = {}): SceneEntity => ({
    key, name: key, model_path: 'model.glb', renderable: true,
    transform: {position: [0, 0, 0], rotation: [0, 0, 0], scale: [1, 1, 1]},
    ...overrides,
});
const documentWith = (...entities: SceneEntity[]): SceneDocument => ({
    schema_version: 4, bindings: {player: 'Player'}, entities, lights: [],
    camera: {
        position: [0, 0, 0], yaw: 0, pitch: 0, vertical_fov: 45, near: 0.1, far: 100,
        follow: {target: 'Player', offset: [0, 3, 4], look_offset: [0, 1, 0], smoothing_seconds: 1},
    },
});
const machine: NativeBehaviorSpec = {
    kind: 'state_machine', definition: {
        schema_version: 1, initial: 'waiting', inputs: [],
        states: [{
            id: 'waiting',
            on_enter: [{kind: 'emit', target: {entity: 'Player', component: 'player-machine'}, signal: 'activated'}],
            transitions: [{
                next: 'waiting', when: {
                    kind: 'signal', signal: 'reset', source: {entity: 'Player', component: 'gone-machine'},
                }
            }],
        }],
    },
};

describe('collectSceneReferences', () => {
    it('finds distinct typed references and component failures across the active scene', () => {
        const player = entity('Player', {
            component_ids: {collider: 'gone-machine'},
            native_behaviors: [{
                id: 'player-machine', enabled: true, behavior: {
                    kind: 'state_machine', definition: {
                        schema_version: 1, initial: 'ready', inputs: [],
                        states: [{id: 'ready', on_enter: [], transitions: []}],
                    },
                }
            }],
        });
        const controller = entity('Controller', {
            parent: 'Player',
            linked_prefab: {
                source: {source_id: 'enemy', path: 'prefabs/enemy.json'},
                bindings: {Actor: 'Player', Exit: 'gone'}
            },
            native_behaviors: [{id: 'controller', enabled: true, behavior: machine}],
            component_views: [{
                id: 'controller', schema_version: 1, enabled: true,
                data: {kind: 'native_behavior', behavior: machine},
            }],
        });
        const results = collectSceneReferences(documentWith(player, controller));
        expect(results.filter(reference => reference.targetKey === 'Player').map(reference => reference.group))
            .toEqual(['Camera', 'Gameplay Roles', 'Hierarchy', 'Linked Prefabs', 'State Machines', 'State Machines']);
        expect(new Set(results.map(reference => reference.id)).size).toBe(results.length);
        expect(results.find(reference => reference.targetComponentId === 'player-machine')?.issue).toBeNull();
        expect(results.filter(reference => reference.issue !== null)).toMatchObject([
            {
                field: 'Slot: Exit',
                targetKey: 'gone',
                issue: 'missing-entity',
                location: {kind: 'entity', key: 'Controller', section: 'prefab'}
            },
            {
                field: 'State waiting, transition 1: Signal sender', targetComponentId: 'gone-machine',
                issue: 'missing-component', location: {kind: 'entity', key: 'Controller', componentId: 'controller'}
            },
        ]);
    });
});
