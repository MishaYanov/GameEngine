import {TestBed} from '@angular/core/testing';
import {invoke, isTauri} from '@tauri-apps/api/core';
import {listen} from '@tauri-apps/api/event';
import {vi} from 'vitest';
import {EditorService} from './editor.service';
import type {
    Camera,
    CommandReceipt,
    EditorCapabilities,
    EditorCommandUpdate,
    EditorSnapshot,
    GameplayModuleDescriptor,
    Light,
    ModelAsset,
    SceneEntity,
    ViewportBounds,
    ViewportStatus,
} from './editor.models';
import {CommandBuilder} from './editor.models';

vi.mock('@tauri-apps/api/core', () => ({invoke: vi.fn(), isTauri: vi.fn(() => true)}));
vi.mock('@tauri-apps/api/event', () => ({listen: vi.fn()}));

it('preserves the native camera owner and rejects numbers that cannot cross IPC intact', () => {
    const sample = {
        workspace_generation: 14, run_id: 'run-7', scene_generation: 3,
        yaw_delta: 0.3, pitch_delta: -0.2, log_distance_delta: 0.1,
    };
    expect(CommandBuilder.playCameraInput(sample)).toEqual({type: 'set_play_camera_input', ...sample});
    expect(() => CommandBuilder.playCameraInput({
        ...sample,
        scene_generation: Number.MAX_SAFE_INTEGER + 1
    })).toThrow(RangeError);
    expect(() => CommandBuilder.playCameraInput({...sample, workspace_generation: -1})).toThrow(RangeError);
    expect(() => CommandBuilder.playCameraInput({...sample, yaw_delta: NaN})).toThrow(RangeError);
});

function snapshot(revision: number): EditorSnapshot {
    return {
        revision,
        scene_revision: revision,
        selection: [],
        mode: 'edit',
        paused: false,
        dirty: false,
        project_dirty: false,
        active_scene_id: 'main',
        scenes: [{id: 'main', asset_path: 'scenes/main.json', loaded: true, dirty: false}],
        project: {
            schema_version: 3,
            name: 'Service fixture',
            scenes: [{id: 'main', asset_path: 'scenes/main.json'}],
            startup_scene_id: 'main',
            fixed_hz: 60,
            max_catchup_steps: 8,
            behaviors: [],
            behavior_definitions: [],
        },
        can_undo: false,
        can_redo: false,
        transaction: null,
        snap: {enabled: false, translation: 0.25, rotation_radians: Math.PI / 12, scale: 0.1},
        document: {
            schema_version: 3,
            entities: [],
            bindings: {},
            lights: [],
            camera: {
                position: [0, 0, 3],
                yaw: 0,
                pitch: 0,
                vertical_fov: 1,
                near: 0.1,
                far: 100,
            },
        },
    };
}

function receipt(next: EditorSnapshot): CommandReceipt {
    return {
        revision: next.revision,
        scene_revision: next.scene_revision,
        change_cursor: next.revision,
        active_scene_id: next.active_scene_id,
        affected_scenes: [next.active_scene_id],
        state_changed: true,
        project_changed: false,
        changes: {
            changed_entities: [],
            removed_entities: [],
            camera_changed: false,
            lights_changed: false,
            topology_changed: false,
            full_resync: false,
        },
    };
}

function commandUpdate(next: EditorSnapshot): EditorCommandUpdate {
    return {kind: 'snapshot', snapshot: next, receipt: receipt(next)};
}

function delta(
    next: EditorSnapshot,
    entities: readonly SceneEntity[] = [],
    camera: Camera | null = null,
    lights: readonly Light[] | null = null,
): EditorCommandUpdate {
    const {document: _document, project: _project, ...state} = next;
    const result = receipt(next);
    return {
        kind: 'delta',
        state,
        entities,
        camera,
        lights,
        receipt: {
            ...result,
            changes: {
                ...result.changes,
                changed_entities: entities.map((entity) => entity.key),
                camera_changed: camera !== null,
                lights_changed: lights !== null,
            },
        },
    };
}

function entity(key: string): SceneEntity {
    return {
        key,
        name: key,
        model_path: 'props/robot.glb',
        renderable: true,
        transform: {position: [0, 0, 0], rotation: [0, 0, 0], scale: [1, 1, 1]},
    };
}

const capabilities: EditorCapabilities = {
    protocol_version: 2,
    scene_schema_version: 3,
    commands: [],
    max_selection: 4096,
    max_history_entries: 128,
    history_budget_bytes: 16 * 1024 * 1024,
    embedded_viewport_supported: true,
    incremental_commands: true,
};
const gameplayModule: GameplayModuleDescriptor = {
    id: 'alternate_game', label: 'Alternate game', description: 'Reach the goal.',
    default_roles: [{name: 'goal', label: 'Goal', requirement_hint: 'Choose the goal marker.'}],
    scene_roles: {finish: []},
};
const status: ViewportStatus = {
    attached: true,
    visible: true,
    width: 800,
    height: 600,
    draw_calls: 0,
    instances: 0,
    triangles: 0,
    gpu_time_ms: null,
    gpu_timestamps_supported: false,
    error: null,
};
const bounds: ViewportBounds = {x: 0, y: 0, width: 800, height: 600, visible: true};

function deferred<T>() {
    let resolve!: (value: T) => void;
    let reject!: (reason: unknown) => void;
    const promise = new Promise<T>((done, fail) => {
        resolve = done;
        reject = fail;
    });
    return {promise, resolve, reject};
}

describe('EditorService native command and viewport queues', () => {
    beforeEach(() => {
        vi.mocked(invoke).mockReset();
        vi.mocked(isTauri).mockReturnValue(true);
        vi.mocked(listen).mockReset().mockResolvedValue(vi.fn());
        TestBed.configureTestingModule({providers: [EditorService]});
    });

    it('serializes document commands using the revision returned by the preceding command', async () => {
        vi.mocked(invoke)
            .mockResolvedValueOnce(snapshot(4))
            .mockResolvedValueOnce(capabilities)
            .mockResolvedValueOnce(commandUpdate(snapshot(5)))
            .mockResolvedValueOnce(commandUpdate(snapshot(6)));
        const service = TestBed.inject(EditorService);
        await service.open('assets', 'scene.json');
        const first = service.execute({type: 'select', keys: ['a']});
        const second = service.execute({type: 'rename', key: 'a', name: 'Changed'});
        expect(service.pending()).toBe(2);
        expect(service.busy()).toBe(true);
        await Promise.all([first, second]);
        expect(invoke).toHaveBeenNthCalledWith(3, 'editor_execute_incremental', {
            request: {
                expected_revision: 4,
                command: {type: 'select', keys: ['a']},
            },
        });
        expect(invoke).toHaveBeenNthCalledWith(4, 'editor_execute_incremental', {
            request: {
                expected_revision: 5,
                command: {type: 'rename', key: 'a', name: 'Changed'},
            },
        });
        expect(service.snapshot()?.revision).toBe(6);
        expect(service.pending()).toBe(0);
        expect(service.busy()).toBe(false);
    });

    it('binds scene warming to its applied mode, workspace generation and document revision', async () => {
        vi.mocked(invoke).mockResolvedValueOnce(2).mockResolvedValueOnce(undefined);
        const service = TestBed.inject(EditorService);
        const modeRevision = await service.setSceneWarmingMode('models', 37);
        await service.warmScene('garden', 8, 37, modeRevision);
        expect(invoke).toHaveBeenNthCalledWith(1, 'editor_set_scene_warming_mode', {
            mode: 'models', expectedGeneration: 37,
        });
        expect(invoke).toHaveBeenNthCalledWith(2, 'editor_warm_scene', {
            sceneId: 'garden', expectedRevision: 8, expectedGeneration: 37, expectedModeRevision: 2,
        });
    });

    it.each(['reload', 'openProject'] as const)(
        'rejects queued role edits from the previous workspace after %s even when scene IDs match',
        async operation => {
            const base = snapshot(1);
            const initial = {...base, document: {...base.document, entities: [entity('a')]}};
            const reopened = {...initial, revision: 10, scene_revision: 10};
            const replacement = deferred<EditorSnapshot>();
            const assigned = {
                ...reopened,
                revision: 11,
                document: {...reopened.document, bindings: {player: 'a'}},
            };
            vi.mocked(invoke)
                .mockResolvedValueOnce(initial)
                .mockReturnValueOnce(replacement.promise)
                .mockResolvedValueOnce(capabilities)
                .mockResolvedValueOnce(commandUpdate(assigned));
            const service = TestBed.inject(EditorService);
            await service.openProject('first/project.json');
            const generation = service.workspaceGeneration();
            const replace = operation === 'reload'
                ? service.reload()
                : service.openProject('second/project.json');
            const command = {
                type: 'set_scene_binding', scene_id: 'main', name: 'player',
                target: {kind: 'assign', key: 'a'},
            } as const;
            const stale = service.execute(command, generation);
            const rejected = expect(stale).rejects.toThrow('The workspace changed');
            replacement.resolve(reopened);
            await replace;
            await rejected;
            expect(service.workspaceGeneration()).toBe(generation + 1);
            expect(service.snapshot()).toEqual(reopened);
            expect(service.error()).toBeNull();
            expect(vi.mocked(invoke).mock.calls.some(([name]) => name === 'editor_snapshot')).toBe(false);
            expect(vi.mocked(invoke).mock.calls.some(([name]) =>
                name === 'editor_execute' || name === 'editor_execute_incremental')).toBe(false);

            await service.execute(command, service.workspaceGeneration());
            expect(invoke).toHaveBeenLastCalledWith('editor_execute_incremental', {
                request: {expected_revision: 10, command},
            });
            expect(service.snapshot()?.document.bindings['player']).toBe('a');
            expect(service.error()).toBeNull();
        },
    );

    it('retains the captured revision for a queued whole-project update', async () => {
        const initial = snapshot(1);
        const changed = {...snapshot(2), project: {...initial.project, name: 'Concurrent project name'}};
        const preceding = deferred<EditorCommandUpdate>();
        vi.mocked(invoke)
            .mockResolvedValueOnce(initial)
            .mockResolvedValueOnce(capabilities)
            .mockReturnValueOnce(preceding.promise)
            .mockRejectedValueOnce({code: 'conflict', message: 'The project revision changed.'})
            .mockResolvedValueOnce(changed);
        const service = TestBed.inject(EditorService);
        await service.openProject('project.json');
        const first = service.execute({type: 'set_project', project: changed.project});
        const command = {type: 'set_project', project: {...initial.project, gameplay_module: 'alternate_game'}} as const;
        const moduleChange = service.execute(command, service.workspaceGeneration(), initial.revision);
        const rejected = expect(moduleChange).rejects.toThrow('The project revision changed.');
        preceding.resolve(commandUpdate(changed));
        await first;
        await rejected;
        expect(invoke).toHaveBeenNthCalledWith(4, 'editor_execute_incremental', {
            request: {expected_revision: 1, command},
        });
        expect(service.snapshot()?.project).toEqual(changed.project);
    });

    it('refreshes authoritative memory after a conflict without retrying the rejected edit', async () => {
        vi.mocked(invoke)
            .mockResolvedValueOnce(snapshot(1))
            .mockResolvedValueOnce(capabilities)
            .mockRejectedValueOnce({
                code: 'conflict',
                message: 'This document changed elsewhere.',
                current_revision: 9,
            })
            .mockResolvedValueOnce(snapshot(9))
            .mockResolvedValueOnce(commandUpdate(snapshot(10)));
        const service = TestBed.inject(EditorService);
        await service.open('assets', 'scene.json');
        await expect(service.execute({type: 'delete_selection'})).rejects.toMatchObject({
            kind: 'conflict',
        });
        expect(service.snapshot()?.revision).toBe(9);
        expect(invoke).toHaveBeenNthCalledWith(4, 'editor_snapshot', undefined);
        expect(
            vi
                .mocked(invoke)
                .mock.calls.filter(([operation]) => operation === 'editor_execute_incremental'),
        ).toHaveLength(1);
        expect(vi.mocked(invoke).mock.calls.some(([operation]) => operation === 'editor_reload')).toBe(
            false,
        );
        await service.execute({type: 'undo'});
        expect(invoke).toHaveBeenLastCalledWith('editor_execute_incremental', {
            request: {expected_revision: 9, command: {type: 'undo'}},
        });
        expect(service.error()).toBeNull();
    });

    it('applies changed entities in document order while retaining unchanged references', async () => {
        const first = entity('first');
        const second = entity('second');
        const third = entity('third');
        const current = {
            ...snapshot(3),
            document: {...snapshot(3).document, entities: [first, second, third]},
        };
        const changedFirst = {...first, name: 'New first'};
        const changedThird = {...third, name: 'New third'};
        vi.mocked(invoke)
            .mockResolvedValueOnce(current)
            .mockResolvedValueOnce(capabilities)
            .mockResolvedValueOnce(
                delta({...current, revision: 4, scene_revision: 4, dirty: true}, [
                    changedThird,
                    changedFirst,
                ]),
            );
        const service = TestBed.inject(EditorService);
        await service.open('assets', 'scene.json');
        const result = await service.execute({
            type: 'rename',
            key: first.key,
            name: changedFirst.name,
        });
        expect(result.document.entities).toEqual([changedFirst, second, changedThird]);
        expect(result.document.entities[1]).toBe(second);
        expect(result.document.camera).toBe(current.document.camera);
        expect(result.project).toBe(current.project);
        expect(current.document.entities[0]).toBe(first);
        expect(service.dirty()).toBe(true);
        expect(vi.mocked(invoke).mock.calls.map(([operation]) => operation)).toEqual([
            'editor_open',
            'editor_capabilities',
            'editor_execute_incremental',
        ]);
    });

    it('accepts shared behaviors and the gameplay module in a fallback snapshot', async () => {
        const current = snapshot(3);
        const next: EditorSnapshot = {
            ...snapshot(4),
            project: {
                ...current.project,
                name: 'Changed project',
                gameplay_module: 'campaign',
                behavior_definitions: [{id: 'walk', settings: {kind: 'controlled_move', speed: 2, space: 'world'}}],
                behaviors: [{scene_id: 'main', behavior: {kind: 'shared', entity: 'new', definition: 'walk'}}],
            },
            document: {...current.document, entities: [entity('new')]},
        };
        const result = receipt(next);
        vi.mocked(invoke)
            .mockResolvedValueOnce(current)
            .mockResolvedValueOnce(capabilities)
            .mockResolvedValueOnce({
                kind: 'snapshot',
                snapshot: next,
                receipt: {
                    ...result,
                    project_changed: true,
                    changes: {...result.changes, topology_changed: true, full_resync: true},
                },
            });
        const service = TestBed.inject(EditorService);
        await service.open('assets', 'scene.json');
        expect(await service.execute({type: 'undo'})).toBe(next);
        expect(service.snapshot()?.document.entities[0].key).toBe('new');
        expect(service.snapshot()?.project.name).toBe('Changed project');
        expect(service.snapshot()?.project.gameplay_module).toBe('campaign');
        expect(service.snapshot()?.project.behavior_definitions[0].id).toBe('walk');
        expect(vi.mocked(invoke).mock.calls.map(([operation]) => operation)).toEqual([
            'editor_open',
            'editor_capabilities',
            'editor_execute_incremental',
        ]);
    });

    it('recovers from a fallback snapshot with a malformed gameplay module', async () => {
        const current = snapshot(3);
        const recovered = snapshot(4);
        const malformed = {
            ...recovered,
            project: {...recovered.project, gameplay_module: 1},
        };
        vi.mocked(invoke)
            .mockResolvedValueOnce(current)
            .mockResolvedValueOnce(capabilities)
            .mockResolvedValueOnce({kind: 'snapshot', snapshot: malformed, receipt: receipt(recovered)})
            .mockResolvedValueOnce(recovered);

        const service = TestBed.inject(EditorService);
        await service.open('assets', 'scene.json');
        await expect(service.execute({type: 'undo'})).rejects.toMatchObject({kind: 'backend'});
        expect(service.snapshot()).toBe(recovered);
    });

    it('keeps the document for selection-only and no-op receipts while advancing queued revisions', async () => {
        const current = snapshot(3);
        const selected = {...current, revision: 4, selection: ['actor']};
        const noop = delta(selected);
        vi.mocked(invoke)
            .mockResolvedValueOnce(current)
            .mockResolvedValueOnce(capabilities)
            .mockResolvedValueOnce(delta(selected))
            .mockResolvedValueOnce({...noop, receipt: {...noop.receipt, state_changed: false}});
        const service = TestBed.inject(EditorService);
        await service.open('assets', 'scene.json');
        const first = await service.execute({type: 'select', keys: ['actor']});
        const second = await service.execute({type: 'select', keys: ['actor']});
        expect(first.document).toBe(current.document);
        expect(second.document).toBe(current.document);
        expect(service.selection()).toEqual(['actor']);
        expect(invoke).toHaveBeenLastCalledWith('editor_execute_incremental', {
            request: {expected_revision: 4, command: {type: 'select', keys: ['actor']}},
        });
    });

    it('keeps the Play session identity when applying a runtime state delta', async () => {
        const playing: EditorSnapshot = {...snapshot(3), mode: 'play', play_session_id: 'play-1'};
        vi.mocked(invoke)
            .mockResolvedValueOnce(playing)
            .mockResolvedValueOnce(capabilities)
            .mockResolvedValueOnce(delta({...playing, revision: 4, paused: true}));
        const service = TestBed.inject(EditorService);
        await service.open('assets', 'scene.json');
        const result = await service.execute({type: 'set_paused', paused: true});
        expect(result.play_session_id).toBe('play-1');
        expect(result.paused).toBe(true);
        expect(result.document).toBe(playing.document);
    });

    it('applies camera and light deltas without copying the entity array', async () => {
        const current = snapshot(3);
        const camera: Camera = {...current.document.camera, position: [1, 2, 3]};
        const lights: Light[] = [
            {
                key: 'sun',
                name: 'Sun',
                kind: 'directional',
                position: [0, 0, 0],
                direction: [0, -1, 0],
                color: [1, 1, 1],
                intensity: 2,
                range: 10,
            },
        ];
        vi.mocked(invoke)
            .mockResolvedValueOnce(current)
            .mockResolvedValueOnce(capabilities)
            .mockResolvedValueOnce(delta(snapshot(4), [], camera, lights));
        const service = TestBed.inject(EditorService);
        await service.open('assets', 'scene.json');
        const result = await service.execute({type: 'undo'});
        expect(result.document.camera).toBe(camera);
        expect(result.document.lights).toBe(lights);
        expect(result.document.entities).toBe(current.document.entities);
    });

    it.each([
        'unknown entity',
        'missing entity',
        'duplicate entity',
        'bad revision',
        'topology change',
        'missing camera',
        'bad state',
        'bad Play session identity',
    ])(
        'resynchronizes an invalid delta (%s) without replaying the accepted command',
        async (failure) => {
            const actor = entity('actor');
            const current = {...snapshot(3), document: {...snapshot(3).document, entities: [actor]}};
            const update = delta(snapshot(4), [{...actor, name: 'Accepted'}]);
            if (update.kind !== 'delta') throw new Error('Expected delta fixture');
            const invalid =
                failure === 'unknown entity'
                    ? delta(snapshot(4), [entity('missing')])
                    : failure === 'missing entity'
                        ? {...update, entities: []}
                        : failure === 'duplicate entity'
                            ? {...update, entities: [actor, actor]}
                            : failure === 'bad revision'
                                ? {...update, state: {...update.state, revision: 2}}
                                : failure === 'topology change'
                                    ? {
                                        ...update,
                                        receipt: {
                                            ...update.receipt,
                                            changes: {...update.receipt.changes, topology_changed: true},
                                        },
                                    }
                                    : failure === 'missing camera'
                                        ? {
                                            ...update,
                                            receipt: {
                                                ...update.receipt,
                                                changes: {...update.receipt.changes, camera_changed: true},
                                            },
                                        }
                                        : failure === 'bad state'
                                            ? {...update, state: {...update.state, mode: 'broken'}}
                                            : {...update, state: {...update.state, mode: 'play', play_session_id: 42}};
            const recovered = {...current, revision: 4, scene_revision: 4};
            vi.mocked(invoke)
                .mockResolvedValueOnce(current)
                .mockResolvedValueOnce(capabilities)
                .mockResolvedValueOnce(invalid)
                .mockResolvedValueOnce(recovered);
            const service = TestBed.inject(EditorService);
            await service.open('assets', 'scene.json');
            await expect(
                service.execute({type: 'rename', key: actor.key, name: 'Accepted'}),
            ).rejects.toMatchObject({kind: 'backend'});
            expect(service.snapshot()).toBe(recovered);
            expect(current.document.entities[0]).toBe(actor);
            expect(vi.mocked(invoke).mock.calls.map(([operation]) => operation)).toEqual([
                'editor_open',
                'editor_capabilities',
                'editor_execute_incremental',
                'editor_snapshot',
            ]);
            expect(service.pending()).toBe(0);
        },
    );

    it('retains known state when an accepted command loses its reply and recovery also fails', async () => {
        const current = snapshot(3);
        vi.mocked(invoke)
            .mockResolvedValueOnce(current)
            .mockResolvedValueOnce(capabilities)
            .mockRejectedValueOnce({
                code: 'internal',
                message: 'Reply serialization failed after the edit.',
            })
            .mockRejectedValueOnce('Recovery unavailable')
            .mockResolvedValueOnce(commandUpdate(snapshot(5)));
        const service = TestBed.inject(EditorService);
        await service.open('assets', 'scene.json');
        await expect(service.execute({type: 'undo'})).rejects.toMatchObject({kind: 'backend'});
        expect(service.snapshot()).toBe(current);
        expect(
            vi
                .mocked(invoke)
                .mock.calls.filter(([operation]) => operation === 'editor_execute_incremental'),
        ).toHaveLength(1);
        expect(service.pending()).toBe(0);
        await service.execute({type: 'redo'});
        expect(service.snapshot()?.revision).toBe(5);
    });

    it('uses structured error codes even when the message contains conflict wording', async () => {
        vi.mocked(invoke)
            .mockResolvedValueOnce(snapshot(3))
            .mockResolvedValueOnce(capabilities)
            .mockRejectedValueOnce({code: 'internal', message: 'Revision conflict reporting failed'})
            .mockResolvedValueOnce(snapshot(3));
        const service = TestBed.inject(EditorService);
        await service.open('assets', 'scene.json');
        await expect(service.execute({type: 'undo'})).rejects.toMatchObject({
            kind: 'backend',
            message: 'Revision conflict reporting failed',
        });
    });

    it('negotiates capabilities after opening before falling back to the legacy command path', async () => {
        vi.mocked(invoke)
            .mockResolvedValueOnce(snapshot(3))
            .mockResolvedValueOnce({...capabilities, incremental_commands: undefined})
            .mockResolvedValueOnce(snapshot(4));
        const service = TestBed.inject(EditorService);
        await service.open('assets', 'scene.json');
        await service.execute({type: 'undo'});
        expect(invoke).toHaveBeenLastCalledWith('editor_execute', {
            request: {expected_revision: 3, command: {type: 'undo'}},
        });
        expect(service.snapshot()?.revision).toBe(4);
    });

    it('preserves string conflict recovery for the legacy command endpoint', async () => {
        vi.mocked(invoke)
            .mockResolvedValueOnce(snapshot(3))
            .mockResolvedValueOnce({...capabilities, incremental_commands: false})
            .mockRejectedValueOnce('Revision conflict: current 9')
            .mockResolvedValueOnce(snapshot(9));
        const service = TestBed.inject(EditorService);
        await service.open('assets', 'scene.json');
        await expect(service.execute({type: 'undo'})).rejects.toMatchObject({
            kind: 'conflict',
            operation: 'editor_execute',
        });
        expect(service.snapshot()?.revision).toBe(9);
        expect(vi.mocked(invoke).mock.calls.map(([operation]) => operation)).toEqual([
            'editor_open',
            'editor_capabilities',
            'editor_execute',
            'editor_snapshot',
        ]);
    });

    it.each(['lights', 'document', 'project'])(
        'rejects malformed %s before publishing the command response',
        async (malformed) => {
            const current = snapshot(3);
            const next = snapshot(4);
            const invalid =
                malformed === 'lights'
                    ? {...delta(next, [], null, []), lights: [null]}
                    : commandUpdate({
                        ...next,
                        ...(malformed === 'document'
                            ? {document: {entities: []}}
                            : {project: {schema_version: 2}}),
                    } as unknown as EditorSnapshot);
            const recovered = snapshot(4);
            vi.mocked(invoke)
                .mockResolvedValueOnce(current)
                .mockResolvedValueOnce(capabilities)
                .mockResolvedValueOnce(invalid)
                .mockResolvedValueOnce(recovered);
            const service = TestBed.inject(EditorService);
            await service.open('assets', 'scene.json');
            await expect(service.execute({type: 'undo'})).rejects.toMatchObject({kind: 'backend'});
            expect(service.snapshot()).toBe(recovered);
            expect(vi.mocked(invoke).mock.calls.map(([operation]) => operation)).toEqual([
                'editor_open',
                'editor_capabilities',
                'editor_execute_incremental',
                'editor_snapshot',
            ]);
        },
    );

    it('queues scene switches before edits so the next request uses the new scene revision', async () => {
        const current: EditorSnapshot = {
            ...snapshot(7),
            project: {
                ...snapshot(7).project,
                scenes: [
                    {id: 'main', asset_path: 'scenes/main.json'},
                    {id: 'secondary', asset_path: 'scenes/secondary.json'},
                ],
            },
            scenes: [
                {id: 'main', asset_path: 'scenes/main.json', loaded: true, dirty: true},
                {id: 'secondary', asset_path: 'scenes/secondary.json', loaded: true, dirty: false},
            ],
            dirty: true,
        };
        const switched: EditorSnapshot = {...current, revision: 8, active_scene_id: 'secondary'};
        vi.mocked(invoke)
            .mockResolvedValueOnce(current)
            .mockResolvedValueOnce(capabilities)
            .mockResolvedValueOnce(commandUpdate(switched))
            .mockResolvedValueOnce(commandUpdate({...switched, revision: 9}));
        const service = TestBed.inject(EditorService);
        await service.open('assets', 'scenes/main.json');
        const switching = service.switchScene('secondary');
        const editing = service.execute({type: 'rename', key: 'actor', name: 'Changed secondary'});
        await Promise.all([switching, editing]);
        expect(invoke).toHaveBeenNthCalledWith(3, 'editor_execute_incremental', {
            request: {expected_revision: 7, command: {type: 'switch_scene', scene_id: 'secondary'}},
        });
        expect(invoke).toHaveBeenNthCalledWith(4, 'editor_execute_incremental', {
            request: {
                expected_revision: 8,
                command: {type: 'rename', key: 'actor', name: 'Changed secondary'},
            },
        });
        expect(service.snapshot()?.active_scene_id).toBe('secondary');
        expect(service.snapshot()?.scenes[0].dirty).toBe(true);
        expect(service.dirty()).toBe(true);
    });

    it('preserves the active scene and dirty drafts when a scene switch is rejected', async () => {
        const current: EditorSnapshot = {
            ...snapshot(3),
            dirty: true,
            scenes: [{id: 'main', asset_path: 'scenes/main.json', loaded: true, dirty: true}],
        };
        vi.mocked(invoke)
            .mockResolvedValueOnce(current)
            .mockResolvedValueOnce(capabilities)
            .mockRejectedValueOnce('Scene is not registered in the catalog')
            .mockResolvedValueOnce(current);
        const service = TestBed.inject(EditorService);
        await service.open('assets', 'scenes/main.json');
        await expect(service.switchScene('missing')).rejects.toMatchObject({kind: 'backend'});
        expect(service.snapshot()).toEqual(current);
        expect(invoke).toHaveBeenLastCalledWith('editor_snapshot', undefined);
        expect(
            vi
                .mocked(invoke)
                .mock.calls.filter(([operation]) => operation === 'editor_execute_incremental'),
        ).toHaveLength(1);
        expect(vi.mocked(invoke).mock.calls.some(([operation]) => operation === 'editor_reload')).toBe(
            false,
        );
    });

    it('rejects commands before opening a document and browser-only use before invoking native IPC', async () => {
        const service = TestBed.inject(EditorService);
        await expect(service.execute({type: 'undo'})).rejects.toMatchObject({kind: 'not_open'});
        expect(invoke).not.toHaveBeenCalled();
        TestBed.resetTestingModule();
        vi.mocked(isTauri).mockReturnValue(false);
        const browser = TestBed.inject(EditorService);
        await expect(browser.open('assets', 'scene.json')).rejects.toMatchObject({
            kind: 'unavailable',
        });
        expect(invoke).not.toHaveBeenCalled();
    });

    it('restores the retained native workspace and its unsaved state after a frontend reload', async () => {
        const retained = {...snapshot(8), dirty: true, can_undo: true};
        vi.mocked(invoke).mockResolvedValueOnce(retained);
        const service = TestBed.inject(EditorService);
        expect(await service.restoreWorkspace()).toEqual(retained);
        expect(service.snapshot()).toEqual(retained);
        expect(service.workspaceGeneration()).toBe(1);
        expect(service.dirty()).toBe(true);
        expect(await service.restoreWorkspace()).toEqual(retained);
        expect(invoke).toHaveBeenCalledExactlyOnceWith('editor_snapshot', undefined);
    });

    it('treats a cold native launch as an empty workspace', async () => {
        vi.mocked(invoke).mockRejectedValueOnce('Open a scene before using the editor');
        const service = TestBed.inject(EditorService);
        expect(await service.restoreWorkspace()).toBeNull();
        expect(service.snapshot()).toBeNull();
        expect(service.workspaceGeneration()).toBe(0);
        expect(service.error()).toBeNull();
    });

    it('uses the project already opened by setup instead of fetching over it', async () => {
        const opened = snapshot(9);
        vi.mocked(invoke).mockResolvedValueOnce(opened);
        const service = TestBed.inject(EditorService);
        const open = service.openProject('new/project.json');
        const restore = service.restoreWorkspace();
        expect(await restore).toEqual(await open);
        expect(service.workspaceGeneration()).toBe(1);
        expect(invoke).toHaveBeenCalledExactlyOnceWith('editor_open_project', {
            manifestPath: 'new/project.json', expectedRevision: null, discardUnsaved: false,
        });
    });

    it('opens the selected project manifest and invalidates cached assets only after success', async () => {
        const opened: EditorSnapshot = {
            ...snapshot(8),
            active_scene_id: 'startup',
            scenes: [{id: 'startup', asset_path: 'scenes/startup.json', loaded: true, dirty: false}],
            project: {
                ...snapshot(8).project,
                startup_scene_id: 'startup',
                scenes: [{id: 'startup', asset_path: 'scenes/startup.json'}],
            },
        };
        vi.mocked(invoke)
            .mockResolvedValueOnce(snapshot(3))
            .mockResolvedValueOnce([])
            .mockResolvedValueOnce(opened);
        const service = TestBed.inject(EditorService);
        await service.open('assets', 'scenes/main.json');
        await service.assets();
        expect(service.modelAssets()).toEqual([]);
        const manifest = 'D:/Projects/My Game/project.json';
        expect(await service.openProject(manifest)).toEqual(opened);
        expect(invoke).toHaveBeenLastCalledWith('editor_open_project', {
            manifestPath: manifest, expectedRevision: 3, discardUnsaved: false,
        });
        expect(service.snapshot()).toEqual(opened);
        expect(service.modelAssets()).toBeNull();
        expect(service.pending()).toBe(0);
    });

    it('preserves the current project and asset cache when opening another project fails', async () => {
        const current = {...snapshot(3), dirty: true};
        vi.mocked(invoke)
            .mockResolvedValueOnce(current)
            .mockResolvedValueOnce([])
            .mockRejectedValueOnce('Invalid project manifest')
            .mockResolvedValueOnce(snapshot(4));
        const service = TestBed.inject(EditorService);
        await service.openProject('current/project.json');
        await service.assets();
        await expect(service.openProject('invalid/project.json')).rejects.toMatchObject({
            kind: 'backend',
            operation: 'editor_open_project',
        });
        expect(service.snapshot()).toEqual(current);
        expect(service.modelAssets()).toEqual([]);
        expect(service.dirty()).toBe(true);
        await service.saveProject();
        expect(invoke).toHaveBeenLastCalledWith('editor_save_project', {expectedRevision: 3});
        expect(service.error()).toBeNull();
    });

    it('coalesces catalogue scans off the edit queue and ignores results from a replaced project', async () => {
        const oldScan = deferred<readonly ModelAsset[]>();
        const newScan = deferred<readonly ModelAsset[]>();
        vi.mocked(invoke)
            .mockResolvedValueOnce(snapshot(3))
            .mockReturnValueOnce(oldScan.promise)
            .mockResolvedValueOnce(snapshot(4))
            .mockReturnValueOnce(newScan.promise);
        const service = TestBed.inject(EditorService);
        await service.openProject('old/project.json');
        const old = service.assets();
        expect(service.assets(true)).toBe(old);
        expect(service.busy()).toBe(false);
        await service.openProject('new/project.json');
        const current = service.assets();
        oldScan.resolve([{name: 'Old model', path: 'props/old.glb'}]);
        await old;
        expect(service.modelAssets()).toBeNull();
        expect(service.assets()).toBe(current);
        const models = [{name: 'New model', path: 'props/new.glb'}];
        newScan.resolve(models);
        expect(await current).toEqual(models);
        expect(service.modelAssets()).toEqual(models);
    });

    it('queues scene registration, save-as, project save and reload with each returned revision', async () => {
        for (const revision of [10, 11, 12, 13, 14]) {
            vi.mocked(invoke).mockResolvedValueOnce(snapshot(revision));
        }
        const service = TestBed.inject(EditorService);
        await service.openProject('game/project.json');
        const registering = service.registerScene('scenes/second.json');
        const savingAs = service.saveSceneAs('scenes/renamed.json');
        const savingProject = service.saveProject();
        const reloading = service.reload();
        expect(service.pending()).toBe(4);
        await Promise.all([registering, savingAs, savingProject, reloading]);
        expect(vi.mocked(invoke).mock.calls.slice(1)).toEqual([
            ['editor_register_scene', {path: 'scenes/second.json', expectedRevision: 10}],
            ['editor_save_scene_as', {path: 'scenes/renamed.json', expectedRevision: 11}],
            ['editor_save_project', {expectedRevision: 12}],
            ['editor_reload', {policy: 'discard_unsaved', expectedRevision: 13}],
        ]);
        expect(service.snapshot()?.revision).toBe(14);
        expect(service.pending()).toBe(0);
    });

    it('uses the recovered revision for a queued project save after save-as conflicts', async () => {
        vi.mocked(invoke)
            .mockResolvedValueOnce(snapshot(3))
            .mockRejectedValueOnce('Revision conflict: current 9')
            .mockResolvedValueOnce(snapshot(9))
            .mockResolvedValueOnce(snapshot(10));
        const service = TestBed.inject(EditorService);
        await service.openProject('game/project.json');
        const savingAs = service.saveSceneAs('scenes/renamed.json');
        const savingProject = service.saveProject();
        await expect(savingAs).rejects.toMatchObject({kind: 'conflict'});
        await savingProject;
        expect(invoke).toHaveBeenNthCalledWith(2, 'editor_save_scene_as', {
            path: 'scenes/renamed.json',
            expectedRevision: 3,
        });
        expect(invoke).toHaveBeenNthCalledWith(3, 'editor_snapshot', undefined);
        expect(invoke).toHaveBeenLastCalledWith('editor_save_project', {expectedRevision: 9});
        expect(
            vi.mocked(invoke).mock.calls.filter(([operation]) => operation === 'editor_save_scene_as'),
        ).toHaveLength(1);
        expect(service.snapshot()?.revision).toBe(10);
    });

    it('rejects scene lifecycle persistence calls before a project is open without invoking IPC', async () => {
        const service = TestBed.inject(EditorService);
        await expect(service.registerScene('scenes/second.json')).rejects.toMatchObject({
            kind: 'not_open',
        });
        await expect(service.saveSceneAs('scenes/renamed.json')).rejects.toMatchObject({
            kind: 'not_open',
        });
        await expect(service.saveProject()).rejects.toMatchObject({kind: 'not_open'});
        expect(invoke).not.toHaveBeenCalled();
        expect(service.pending()).toBe(0);
    });

    it('deduplicates capabilities and rejects incompatible protocol/schema before caching', async () => {
        const result = deferred<EditorCapabilities>();
        vi.mocked(invoke).mockReturnValueOnce(result.promise);
        const service = TestBed.inject(EditorService);
        const first = service.loadCapabilities();
        const second = service.loadCapabilities();
        expect(second).toBe(first);
        result.resolve({...capabilities, protocol_version: 1});
        await expect(first).rejects.toMatchObject({kind: 'backend'});
        expect(service.capabilities()).toBeNull();
        vi.mocked(invoke).mockResolvedValueOnce({...capabilities, scene_schema_version: 2});
        await expect(service.loadCapabilities()).rejects.toMatchObject({kind: 'backend'});
        expect(service.capabilities()).toBeNull();
        vi.mocked(invoke).mockResolvedValueOnce(capabilities);
        expect(await service.loadCapabilities()).toEqual(capabilities);
        await service.loadCapabilities();
        expect(invoke).toHaveBeenCalledTimes(3);
    });

    it('caches module metadata for alternate modules and accepts older hosts without metadata', async () => {
        const service = TestBed.inject(EditorService);
        const advertised = {...capabilities, gameplay_modules: [gameplayModule]};
        vi.mocked(invoke).mockResolvedValueOnce(advertised);
        expect(await service.loadCapabilities()).toEqual(advertised);
        expect(service.capabilities()?.gameplay_modules?.[0].scene_roles['finish']).toEqual([]);
        await service.loadCapabilities();
        expect(invoke).toHaveBeenCalledTimes(1);
    });

    it.each([
        ['null catalog', null],
        ['object catalog', {}],
        ['empty module ID', [{...gameplayModule, id: ''}]],
        ['invalid module ID', [{...gameplayModule, id: 'Uppercase'}]],
        ['long module ID', [{...gameplayModule, id: 'a'.repeat(33)}]],
        ['duplicate module IDs', [gameplayModule, gameplayModule]],
        ['empty label', [{...gameplayModule, label: '  '}]],
        ['missing description', [{...gameplayModule, description: undefined}]],
        ['invalid role collection', [{...gameplayModule, default_roles: {}}]],
        ['empty role name', [{...gameplayModule, default_roles: [{...gameplayModule.default_roles[0], name: ''}]}]],
        ['long UTF-8 role name', [{...gameplayModule, default_roles: [{...gameplayModule.default_roles[0], name: 'é'.repeat(513)}]}]],
        ['role control character', [{...gameplayModule, default_roles: [{...gameplayModule.default_roles[0], name: 'bad\nrole'}]}]],
        ['missing role hint', [{...gameplayModule, default_roles: [{name: 'goal', label: 'Goal'}]}]],
        ['duplicate roles', [{...gameplayModule, default_roles: [...gameplayModule.default_roles, ...gameplayModule.default_roles]}]],
        ['invalid scene overrides', [{...gameplayModule, scene_roles: []}]],
        ['invalid scene ID', [{...gameplayModule, scene_roles: {'two scenes': []}}]],
        ['long scene ID', [{...gameplayModule, scene_roles: {['x'.repeat(257)]: []}}]],
        ['malformed scene roles', [{...gameplayModule, scene_roles: {finish: null}}]],
    ])('rejects malformed %s before caching gameplay metadata', async (_label, modules) => {
        const service = TestBed.inject(EditorService);
        vi.mocked(invoke).mockResolvedValueOnce({...capabilities, gameplay_modules: modules});
        await expect(service.loadCapabilities()).rejects.toThrow('invalid gameplay module metadata');
        expect(service.capabilities()).toBeNull();
        vi.mocked(invoke).mockResolvedValueOnce(capabilities);
        await service.loadCapabilities();
        expect(service.capabilities()?.gameplay_modules).toBeUndefined();
    });

    it('accepts an asset operation snapshot and retains the displayed catalog until refresh', async () => {
        const asset = {name: 'Imported', path: 'models/imported.glb', kind: 'model' as const,
            asset_id: 'imported-id', fingerprint: 'new-fingerprint'};
        const previous = [{name: 'Old model', path: 'models/old.glb'}];
        const next = snapshot(2);
        vi.mocked(invoke)
            .mockResolvedValueOnce(snapshot(1))
            .mockResolvedValueOnce(previous)
            .mockResolvedValueOnce({operation_id: 'import-operation', asset, snapshot: next});
        const service = TestBed.inject(EditorService);
        await service.openProject('project.json');
        await service.assets();
        const result = await service.importExternalGlb('C:/Models/source.glb', 'models/imported.glb', service.workspaceGeneration(), 1);
        expect(result.asset).toEqual(asset);
        expect(service.snapshot()).toBe(next);
        expect(service.modelAssets()).toEqual(previous);
        expect(invoke).toHaveBeenLastCalledWith('editor_import_external_glb', {
            sourcePath: 'C:/Models/source.glb', destinationPath: 'models/imported.glb', expectedRevision: 1,
        });
        vi.mocked(invoke).mockResolvedValueOnce([asset]);
        expect(await service.assets()).toEqual([asset]);
        expect(service.modelAssets()).toEqual([asset]);
    });

    it('ignores a pre-publication catalog reply and requests fresh entries after a move', async () => {
        const original: ModelAsset = {name: 'Actor', path: 'actor.glb', asset_id: 'actor-id'};
        const moved = {...original, path: 'moved.glb'};
        const stale = deferred<readonly ModelAsset[]>();
        const fresh = deferred<readonly ModelAsset[]>();
        vi.mocked(invoke)
            .mockResolvedValueOnce(snapshot(1))
            .mockResolvedValueOnce([original])
            .mockReturnValueOnce(stale.promise)
            .mockResolvedValueOnce({operation_id: 'move', asset: moved, snapshot: snapshot(2)})
            .mockReturnValueOnce(fresh.promise);
        const service = TestBed.inject(EditorService);
        await service.openProject('project.json');
        await service.assets();
        const oldRequest = service.assets(true);
        await service.moveAsset('actor-id', 'moved.glb', service.workspaceGeneration(), 1);
        const newRequest = service.assets();
        expect(newRequest).not.toBe(oldRequest);
        expect(service.modelAssets()).toEqual([original]);
        fresh.resolve([moved]);
        await newRequest;
        stale.resolve([original]);
        await oldRequest;
        expect(service.modelAssets()).toEqual([moved]);
        expect(vi.mocked(invoke).mock.calls.filter(([name]) => name === 'editor_assets')).toHaveLength(3);
    });

    it('carries reimport fingerprint and captured revision without rewriting the request', async () => {
        vi.mocked(invoke)
            .mockResolvedValueOnce(snapshot(5))
            .mockResolvedValueOnce({operation_id: 'reimport', asset: {name: 'Actor', path: 'actor.glb'}, snapshot: snapshot(6)});
        const service = TestBed.inject(EditorService);
        await service.openProject('project.json');
        await service.reimportAsset('actor-id', 'captured-fingerprint', 'C:/Models/new.glb', service.workspaceGeneration(), 4);
        expect(invoke).toHaveBeenLastCalledWith('editor_reimport_asset', {
            assetId: 'actor-id', expectedFingerprint: 'captured-fingerprint', sourcePath: 'C:/Models/new.glb', expectedRevision: 4,
        });
    });

    it('accepts schema 5 orbit cameras through a full snapshot and a camera-only delta', async () => {
        const start = snapshot(1);
        const orbit = {
            target: 'actor', pivot_offset: [0, 0.7, 0] as const, initial_yaw: 0, initial_pitch: -0.3,
            initial_distance: 5, min_distance: 1, max_distance: 8, min_pitch: -1, max_pitch: 1,
            follow_seconds: 0.1, recovery_seconds: 0.2, teleport_distance: 8,
            probe_radius: 0.25, obstruction_mask: 1,
        };
        const original = {
            ...start,
            document: {...start.document, schema_version: 5 as const, entities: [entity('actor')]}
        };
        const camera = {...original.document.camera, orbit};
        const authored = {...original, revision: 2, scene_revision: 2, document: {...original.document, camera}};
        const tuned = {...camera, orbit: {...orbit, initial_distance: 6}};
        const next = {...authored, revision: 3, scene_revision: 3};
        vi.mocked(invoke).mockResolvedValueOnce(original)
            .mockResolvedValueOnce({...capabilities, scene_schema_version: 5})
            .mockResolvedValueOnce(commandUpdate(authored))
            .mockResolvedValueOnce(delta(next, [], tuned));
        const service = TestBed.inject(EditorService);
        await service.openProject('project.json');
        await service.execute({type: 'set_camera', camera});
        expect(service.document()?.camera.orbit).toEqual(orbit);
        await service.execute({type: 'set_camera', camera: tuned});
        expect(service.document()?.camera).toEqual(tuned);
    });

    it('accepts schema4 component metadata and carries its per-entity views through deltas', async () => {
        const component = {id: 'mesh', schema_version: 1 as const, enabled: true,
            data: {kind: 'mesh_renderer' as const, model_path: 'actor.glb', asset_id: null, mesh_index: null, material: null}};
        const original = {...snapshot(1), document: {...snapshot(1).document, schema_version: 4 as const,
            entities: [{...entity('actor'), component_views: [component]}]}};
        const next = {...original, revision: 2, scene_revision: 2, document: {...original.document,
            entities: [{...original.document.entities[0], renderable: false, component_views: [{...component, enabled: false}]}]}};
        vi.mocked(invoke).mockResolvedValueOnce(original).mockResolvedValueOnce({...capabilities, scene_schema_version: 4,
            component_types: [{type_id: 'engine.mesh_renderer', kind: 'mesh_renderer', label: 'Mesh Renderer', description: 'Shared model', max_per_object: 1}]})
            .mockResolvedValueOnce(delta(next, next.document.entities));
        const service = TestBed.inject(EditorService);
        await service.openProject('project.json');
        await service.execute({type: 'set_component_enabled', key: 'actor', component_id: 'mesh', enabled: false});
        expect(service.document()?.schema_version).toBe(4);
        expect(service.document()?.entities[0].component_views?.[0].enabled).toBe(false);
        expect(service.document()?.entities[0].renderable).toBe(false);
    });

    it('adopts a source-save snapshot while preserving catalog selection until a fresh query', async () => {
        const prefab = {schema_version: 3 as const, gameplay: null, reference_slots: {}, entities: [{key: 'source', name: 'Source',
            transform: {position: [0, 0, 0] as const, rotation: [0, 0, 0] as const, scale: [1, 1, 1] as const}, components: []}]};
        const draft = {draft_id: 'draft', workspace_generation: 101, revision: 1, path: 'source.json', asset_id: 'source-id', fingerprint: 'updated', prefab};
        const old = [{name: 'Source', path: 'source.json', kind: 'prefab', asset_id: 'source-id', fingerprint: 'old'}];
        const current = snapshot(1);
        vi.mocked(invoke).mockResolvedValueOnce(current).mockResolvedValueOnce(old)
            .mockResolvedValueOnce({draft, snapshot: current}).mockResolvedValueOnce([{...old[0], fingerprint: 'updated'}]);
        const service = TestBed.inject(EditorService);
        await service.openProject('project.json'); await service.assets();
        await service.savePrefabSource('draft', prefab, service.workspaceGeneration(), 1);
        expect(invoke).toHaveBeenLastCalledWith('editor_prefab_source_save', {draftId: 'draft', prefab, expectedRevision: 1});
        expect(service.snapshot()).toBe(current);
        expect(service.modelAssets()).toEqual(old);
        expect((await service.assets())[0].fingerprint).toBe('updated');
    });

    it('carries reviewed prefab draft tokens and revisions through link and update publication', async () => {
        vi.mocked(invoke).mockResolvedValueOnce(snapshot(5))
            .mockResolvedValueOnce(snapshot(6)).mockResolvedValueOnce(snapshot(7));
        const service = TestBed.inject(EditorService);
        await service.openProject('project.json');
        const generation = service.workspaceGeneration();
        await service.instantiateLinkedPrefab('source.json', 'reviewed-link', [0, 0, 0], {Actor: 'player'}, generation, 4);
        expect(invoke).toHaveBeenLastCalledWith('editor_instantiate_linked_prefab', {
            path: 'source.json', draftId: 'reviewed-link', position: [0, 0, 0], bindings: {Actor: 'player'}, expectedRevision: 4,
        });
        await service.updateLinkedPrefab('anchor', 'reviewed-update', 'keep_local', {Actor: 'player'}, generation, 5);
        expect(invoke).toHaveBeenLastCalledWith('editor_update_linked_prefab', {
            rootKey: 'anchor', draftId: 'reviewed-update', resolution: 'keep_local', bindings: {Actor: 'player'}, expectedRevision: 5,
        });
        expect(service.snapshot()?.revision).toBe(7);
    });

    it('queries bounded runtime state without requesting an authored snapshot or changing global feedback', async () => {
        const page = {play_session_id: 'play', run_id: 'run', scene_id: 'main', tick: 0, topology_revision: 0,
            total_entities: 0, entities: [], timer_count: 0, pending_spawns: 0, last_error: null};
        vi.mocked(invoke).mockResolvedValueOnce(page);
        const service = TestBed.inject(EditorService);
        expect(await service.inspectRuntime(32, 32)).toEqual(page);
        expect(invoke).toHaveBeenCalledExactlyOnceWith('editor_runtime_inspect', {offset: 32, limit: 32});
        await expect(service.inspectRuntime(0, 129)).rejects.toThrow('1–128');
        expect(invoke).toHaveBeenCalledTimes(1);
        expect(service.error()).toBeNull();
    });

    it('rejects queued asset publication when its workspace was replaced', async () => {
        const replacement = deferred<EditorSnapshot>();
        vi.mocked(invoke)
            .mockResolvedValueOnce(snapshot(1))
            .mockReturnValueOnce(replacement.promise);
        const service = TestBed.inject(EditorService);
        await service.openProject('first/project.json');
        const generation = service.workspaceGeneration();
        const reopen = service.openProject('second/project.json');
        const stale = service.importExternalGlb('C:/Models/old.glb', 'old.glb', generation, 1);
        const rejected = expect(stale).rejects.toThrow('workspace changed');
        replacement.resolve(snapshot(10));
        await reopen;
        await rejected;
        expect(vi.mocked(invoke).mock.calls.some(([name]) => name === 'editor_import_external_glb')).toBe(false);
        expect(vi.mocked(invoke).mock.calls.some(([name]) => name === 'editor_snapshot')).toBe(false);
        expect(service.snapshot()?.revision).toBe(10);
        expect(service.error()).toBeNull();
    });

    it('does not replace current workspace feedback with a late source-picker rejection', async () => {
        const picker = deferred<string | null>();
        vi.mocked(invoke)
            .mockResolvedValueOnce(snapshot(1))
            .mockReturnValueOnce(picker.promise)
            .mockResolvedValueOnce(snapshot(10))
            .mockRejectedValueOnce('Current workspace picker failure');
        const service = TestBed.inject(EditorService);
        await service.openProject('first/project.json');
        const stale = service.chooseExternalGlb();
        const rejected = expect(stale).rejects.toThrow('Old workspace picker failure');
        await service.openProject('second/project.json');
        await expect(service.chooseExternalGlb()).rejects.toThrow('Current workspace picker failure');
        const currentError = service.error();
        picker.reject('Old workspace picker failure');
        await rejected;
        expect(service.error()).toBe(currentError);
        expect(service.snapshot()?.revision).toBe(10);
    });

    it('coalesces queued viewport bounds to the latest geometry', async () => {
        vi.mocked(invoke).mockResolvedValue(status);
        const service = TestBed.inject(EditorService);
        const first = service.updateViewportBounds(bounds);
        const second = service.updateViewportBounds({...bounds, x: 10});
        const third = service.updateViewportBounds({...bounds, x: 20, visible: false});
        expect(second).toBe(first);
        expect(third).toBe(first);
        await third;
        expect(invoke).toHaveBeenCalledExactlyOnceWith('editor_viewport_bounds', {
            bounds: {...bounds, x: 20, visible: false},
        });
    });

    it('orders detach after an in-flight attach and coalesces duplicate detach requests', async () => {
        const attachment = deferred<ViewportStatus>();
        const unlisten = vi.fn();
        vi.mocked(listen).mockResolvedValue(unlisten);
        vi.mocked(invoke)
            .mockResolvedValueOnce(capabilities)
            .mockReturnValueOnce(attachment.promise)
            .mockResolvedValueOnce({...status, attached: false, visible: false});
        const service = TestBed.inject(EditorService);
        const attached = service.attachViewport(bounds);
        await vi.waitFor(() =>
            expect(invoke).toHaveBeenCalledWith('editor_viewport_attach', {bounds}),
        );
        const first = service.detachViewport();
        expect(service.detachViewport()).toBe(first);
        expect(
            vi.mocked(invoke).mock.calls.some(([operation]) => operation === 'editor_viewport_detach'),
        ).toBe(false);
        attachment.resolve(status);
        await Promise.all([attached, first]);
        expect(invoke).toHaveBeenLastCalledWith('editor_viewport_detach', undefined);
        expect(unlisten).toHaveBeenCalledTimes(1);
        expect(service.viewport()?.attached).toBe(false);
    });

    it('releases the native viewport after an in-flight attachment finishes during teardown', async () => {
        const attachment = deferred<ViewportStatus>();
        const unlisten = vi.fn();
        vi.mocked(listen).mockResolvedValue(unlisten);
        vi.mocked(invoke)
            .mockResolvedValueOnce(capabilities)
            .mockReturnValueOnce(attachment.promise)
            .mockResolvedValueOnce({...status, attached: false, visible: false});

        const service = TestBed.inject(EditorService);
        const attached = service.attachViewport(bounds);
        await vi.waitFor(() =>
            expect(invoke).toHaveBeenLastCalledWith('editor_viewport_attach', {bounds}),
        );

        TestBed.resetTestingModule();
        expect(unlisten).toHaveBeenCalledTimes(1);
        expect(invoke).toHaveBeenCalledTimes(2);

        attachment.resolve(status);
        await attached;
        await vi.waitFor(() => expect(invoke).toHaveBeenLastCalledWith('editor_viewport_detach'));
        expect(invoke).toHaveBeenCalledTimes(3);
    });

    it('keeps old and new owner bounds on their respective sides of lifecycle boundaries', async () => {
        vi.mocked(invoke)
            .mockResolvedValueOnce(status)
            .mockResolvedValueOnce(capabilities)
            .mockResolvedValueOnce(status)
            .mockResolvedValueOnce(status);
        const service = TestBed.inject(EditorService);
        const oldBounds = service.updateViewportBounds({...bounds, x: 1});
        const attached = service.attachViewport(bounds);
        const newBounds = service.updateViewportBounds({...bounds, x: 2});
        expect(oldBounds).not.toBe(newBounds);
        await Promise.all([oldBounds, attached, newBounds]);
        expect(vi.mocked(invoke).mock.calls.map(([operation]) => operation)).toEqual([
            'editor_viewport_bounds',
            'editor_capabilities',
            'editor_viewport_attach',
            'editor_viewport_bounds',
        ]);
        expect(invoke).toHaveBeenLastCalledWith('editor_viewport_bounds', {
            bounds: {...bounds, x: 2},
        });
    });

    it('releases viewport ownership idempotently and allows the next owner to reserve it', () => {
        const service = TestBed.inject(EditorService);
        const release = service.reserveViewport();
        expect(() => service.reserveViewport()).toThrow('already owns');
        release();
        release();
        const next = service.reserveViewport();
        release(); // The first owner's stale release cannot release the new owner.
        expect(() => service.reserveViewport()).toThrow('already owns');
        next();
        expect(() => service.reserveViewport()).not.toThrow();
    });

    it('bounds document work at 256 pending calls without dropping queued requests', async () => {
        vi.mocked(invoke).mockResolvedValue(snapshot(0));
        const service = TestBed.inject(EditorService);
        const queued = Array.from({length: 256}, () => service.refresh());
        expect(service.pending()).toBe(256);
        await expect(service.refresh()).rejects.toMatchObject({kind: 'busy'});
        await Promise.all(queued);
        expect(invoke).toHaveBeenCalledTimes(256);
        expect(service.pending()).toBe(0);
    });

    it('keeps background compilation out of the document queue', async () => {
        const compiled = deferred<{path: string; asset_count: number; asset_bytes: number}>();
        vi.mocked(invoke).mockImplementation(async operation => {
            if (operation === 'editor_open_project') return snapshot(1);
            if (operation === 'editor_prepare_project_build') return {build_id: 4, path: 'D:/Exports/Game', asset_count: 4, asset_bytes: 1024};
            if (operation === 'editor_finish_project_build') return compiled.promise;
            if (operation === 'editor_capabilities') return capabilities;
            if (operation === 'editor_execute_incremental') return commandUpdate(snapshot(2));
            throw new Error(`Unexpected operation: ${operation}`);
        });
        const service = TestBed.inject(EditorService);
        await service.openProject('project.json');
        const prepared = await service.prepareBuild('D:/Exports');
        const build = service.finishBuild(prepared.build_id);
        expect(service.busy()).toBe(false);
        await service.execute({type: 'select', keys: []});
        expect(service.snapshot()?.revision).toBe(2);
        expect(invoke).toHaveBeenCalledWith('editor_prepare_project_build', {destination: 'D:/Exports', expectedRevision: 1});
        expect(invoke).toHaveBeenCalledWith('editor_finish_project_build', {buildId: 4});
        const result = {path: 'D:/Exports/Game', asset_count: 4, asset_bytes: 1024};
        compiled.resolve(result);
        expect(await build).toEqual(result);
        expect(service.snapshot()?.revision).toBe(2);
    });

    it('saves all documents through the revision queue and explicitly runs a recorded export', async () => {
        vi.mocked(invoke)
            .mockResolvedValueOnce({...snapshot(7), dirty: true, project_dirty: true})
            .mockResolvedValueOnce(snapshot(8))
            .mockResolvedValueOnce({path: 'D:/Exports/Game', process_id: 123});
        const service = TestBed.inject(EditorService);
        await service.openProject('project.json');
        await service.saveAll();
        expect(invoke).toHaveBeenLastCalledWith('editor_save_all', {expectedRevision: 7});
        expect(service.snapshot()?.revision).toBe(8);
        expect(service.dirty()).toBe(false);
        expect(await service.runBuild('D:/Exports/Game')).toEqual({path: 'D:/Exports/Game', process_id: 123});
        expect(invoke).toHaveBeenLastCalledWith('editor_run_build', {expectedPath: 'D:/Exports/Game'});
    });

    it('cleans up a native listener that resolves after its Angular owner was destroyed', async () => {
        const subscription = deferred<() => void>();
        vi.mocked(listen).mockReturnValueOnce(subscription.promise);
        const service = TestBed.inject(EditorService);
        const pending = service.listenViewportInput(vi.fn());
        TestBed.resetTestingModule();
        const unlisten = vi.fn();
        subscription.resolve(unlisten);
        const stop = await pending;
        expect(unlisten).toHaveBeenCalledTimes(1);
        stop();
        expect(unlisten).toHaveBeenCalledTimes(1);
    });
});
