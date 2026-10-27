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
    Light,
    SceneEntity,
    ViewportBounds,
    ViewportStatus,
} from './editor.models';

vi.mock('@tauri-apps/api/core', () => ({invoke: vi.fn(), isTauri: vi.fn(() => true)}));
vi.mock('@tauri-apps/api/event', () => ({listen: vi.fn()}));

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
            schema_version: 2,
            name: 'Service fixture',
            scenes: [{id: 'main', asset_path: 'scenes/main.json'}],
            startup_scene_id: 'main',
            fixed_hz: 60,
            max_catchup_steps: 8,
            behaviors: [],
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
    const promise = new Promise<T>((done) => {
        resolve = done;
    });
    return {promise, resolve};
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

    it('accepts a complete fallback snapshot for topology and project changes in the same reply', async () => {
        const current = snapshot(3);
        const next = {
            ...snapshot(4),
            project: {...current.project, name: 'Changed project'},
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
        expect(vi.mocked(invoke).mock.calls.map(([operation]) => operation)).toEqual([
            'editor_open',
            'editor_capabilities',
            'editor_execute_incremental',
        ]);
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
                                        : {...update, state: {...update.state, mode: 'broken'}};
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
                    } as EditorSnapshot);
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
        expect(invoke).toHaveBeenLastCalledWith('editor_open_project', {manifestPath: manifest});
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
