import {computed, signal} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import axe from 'axe-core';
import {vi} from 'vitest';

import type {EditorSnapshot, ProjectData} from '../../../../core/editor/editor.models';
import {EditorService} from '../../../../core/editor/editor.service';
import {ViewportEditorService} from '../../../../core/editor/viewport-editor.service';
import type {PlayStatus} from '../../../../core/editor/viewport-editor.models';
import {LAYOUT_PREFERENCES_STORAGE} from '../../../../shared/services/layout-preferences-storage';
import {SceneEditingService} from '../scene-editing.service';
import {SceneFlow} from './scene-flow';

function snapshot(): EditorSnapshot {
    const project: ProjectData = {
        schema_version: 3, name: 'Scene Flow test', startup_scene_id: 'hall',
        scenes: [{id: 'hall', asset_path: 'scenes/hall.json'}, {id: 'arena', asset_path: 'scenes/arena.json'}],
        fixed_hz: 60, max_catchup_steps: 8, behaviors: [], behavior_definitions: [],
    };
    return {
        active_scene_id: 'hall', revision: 3, scene_revision: 2, project,
        project_dirty: false, dirty: false, can_undo: false, can_redo: false,
        mode: 'edit', paused: false, selection: [], transaction: null,
        scenes: project.scenes.map(scene => ({...scene, dirty: false, loaded: scene.id === 'hall'})),
        document: {
            schema_version: 3, bindings: {}, entities: [], lights: [],
            camera: {position: [0, 0, 3], yaw: 0, pitch: 0, vertical_fov: 1, near: 0.1, far: 100},
        },
        snap: {enabled: false, translation: 1, rotation_radians: 0.1, scale: 0.1},
    };
}

async function createFlow(initial = snapshot(), playStatus: PlayStatus | null = null, worldGpuAssetBytes?: number) {
    const current = signal(initial);
    const status = signal(playStatus);
    const editingError = signal<string | null>(null);
    const execute = vi.fn<EditorService['execute']>(async () => current());
    TestBed.configureTestingModule({
        providers: [
            {provide: LAYOUT_PREFERENCES_STORAGE, useValue: {load: async () => null, save: async () => undefined}},
            {
                provide: ViewportEditorService, useValue: {
                    state: () => ({
                        game_status: status(), world_gpu_asset_bytes: worldGpuAssetBytes,
                    })
                }
            },
            {
                provide: EditorService, useValue: {
                    snapshot: current, document: computed(() => current().document),
                    playing: computed(() => current().mode === 'play'), workspaceGeneration: () => 7, execute,
                }
            },
            {
                provide: SceneEditingService, useValue: {
                    ready: () => true, error: editingError,
                    withCommittedEdit: async (action: () => Promise<void>) => {
                        try {
                            await action();
                            return true;
                        } catch (error) {
                            editingError.set(String(error));
                            return false;
                        }
                    },
                }
            },
        ]
    });
    const fixture = TestBed.createComponent(SceneFlow);
    expect(fixture.componentInstance.show()).toBe(true);
    await fixture.whenStable();
    const root: HTMLElement = fixture.nativeElement;
    const button = (label: string) => Array.from(root.querySelectorAll<HTMLButtonElement>('button'))
        .find(button => button.textContent?.trim() === label)!;
    const click = async (label: string) => {
        button(label).click();
        await fixture.whenStable();
    };
    const write = async (name: string, value: string) => {
        const label = Array.from(root.querySelectorAll('label'))
            .find(label => label.firstChild?.textContent?.trim() === name);
        const field = label?.querySelector<HTMLInputElement | HTMLSelectElement>('input, select');
        if (!field) throw new Error('Missing field ' + name);
        field.value = value;
        field.dispatchEvent(new Event('input', {bubbles: true}));
        if (field instanceof HTMLSelectElement) field.dispatchEvent(new Event('change', {bubbles: true}));
        await fixture.whenStable();
    };
    return {fixture, root, button, click, write, current, execute, status};
}

describe('Scene Flow', () => {
    const dialog = HTMLDialogElement.prototype;
    const originalShow = Object.getOwnPropertyDescriptor(dialog, 'showModal');
    const originalClose = Object.getOwnPropertyDescriptor(dialog, 'close');
    beforeAll(() => {
        Object.defineProperty(dialog, 'showModal', {
            configurable: true,
            value: function (this: HTMLDialogElement) {
                this.setAttribute('open', '');
            },
        });
        Object.defineProperty(dialog, 'close', {
            configurable: true,
            value: function (this: HTMLDialogElement) {
                this.removeAttribute('open');
                this.dispatchEvent(new Event('close'));
            },
        });
    });
    afterAll(() => {
        if (originalShow) Object.defineProperty(dialog, 'showModal', originalShow);
        else Reflect.deleteProperty(dialog, 'showModal');
        if (originalClose) Object.defineProperty(dialog, 'close', originalClose);
        else Reflect.deleteProperty(dialog, 'close');
    });

    it('commits a route with approach thresholds through the revision-checked project command', async () => {
        const {root, button, click, write, execute} = await createFlow();
        await click('Add Route');
        expect(button('Apply Scene Flow').disabled).toBe(true);
        expect(root.querySelector('[role="alert"]')?.textContent).toContain('unique route IDs');
        await write('Route ID', 'open-gate');
        await write('Source sensor', 'gate-exit');
        await write('Arrival marker', 'arena-arrival');
        await write('Preload', 'approach');
        expect(button('Apply Scene Flow').disabled).toBe(false);
        const accessibility = await axe.run(root, {rules: {'color-contrast': {enabled: false}}});
        expect(accessibility.violations).toEqual([]);
        await click('Apply Scene Flow');
        expect(execute).toHaveBeenCalledExactlyOnceWith({
            type: 'set_project',
            project: expect.objectContaining({
                scene_routes: [{
                    id: 'open-gate',
                    from: 'hall',
                    to: 'arena',
                    activate: {kind: 'enter_sensor', sensor: 'gate-exit'},
                    arrival: 'arena-arrival',
                    preload: {kind: 'approach', distance: 12, release_distance: 16},
                }],
            }),
        }, 7, 3);
        expect(root.querySelector('dialog[open]')).toBeNull();
    });

    it('searches long destinations and saves a route by scene ID', async () => {
        const initial = snapshot();
        const longName = 'scenes/northwestern-expedition/world-section-beyond-the-observatory-final-arrival';
        const {fixture, root, click, write, execute} = await createFlow({
            ...initial,
            project: {
                ...initial.project, scenes: [...initial.project.scenes, {
                    id: 'expedition', asset_path: longName + '.json',
                }]
            },
        });
        await click('Add Route');
        await write('Route ID', 'open-expedition');
        await write('Arrival marker', 'expedition-arrival');
        await write('Activation', 'manual');

        const label = [...root.querySelectorAll<HTMLLabelElement>('editor-search-picker label')]
            .find(item => item.textContent?.trim() === 'To scene')!;
        const field = label.control as HTMLInputElement;
        field.click();
        field.value = 'observatory final';
        field.dispatchEvent(new Event('input'));
        await fixture.whenStable();
        const matches = label.closest('editor-search-picker')!.querySelectorAll<HTMLElement>('[role="option"]');
        expect([...matches].map(option => option.textContent?.trim())).toEqual([longName]);
        field.dispatchEvent(new KeyboardEvent('keydown', {key: 'Enter', cancelable: true}));
        await fixture.whenStable();
        expect(field.value).toBe(longName);

        await click('Apply Scene Flow');
        expect(execute).toHaveBeenCalledExactlyOnceWith({
            type: 'set_project',
            project: expect.objectContaining({
                scene_routes: [expect.objectContaining({id: 'open-expedition', from: 'hall', to: 'expedition'})],
            }),
        }, 7, 3);
    });

    it('renders imported scene labels as text even when a label contains markup', async () => {
        const initial = snapshot();
        const markup = '<scene-probe>example</scene-probe>';
        const scenes = initial.project.scenes.map(scene => scene.id === 'hall'
            ? {...scene, asset_path: 'scenes/' + markup + '.json'}
            : scene);
        const {root} = await createFlow({
            ...initial,
            project: {
                ...initial.project, scenes, scene_routes: [
                    {id: 'to-arena', from: 'hall', to: 'arena', activate: {kind: 'manual'}, arrival: 'gate'},
                ]
            },
        });
        const graph = root.querySelector('[aria-label="Scene connection graph"]')!;
        expect(graph.textContent).toContain(markup);
        expect(graph.querySelector('scene-probe')).toBeNull();
    });

    it('draws unique scene nodes and filters route selection without changing the draft', async () => {
        const initial = snapshot();
        const cellar = {id: 'cellar', asset_path: 'scenes/cellar.json'};
        const {root, write, execute, button} = await createFlow({
            ...initial,
            project: {
                ...initial.project, scenes: [...initial.project.scenes, cellar], scene_routes: [
                    {id: 'to-arena', from: 'hall', to: 'arena', activate: {kind: 'manual'}, arrival: 'gate'},
                    {id: 'to-cellar', from: 'arena', to: 'cellar', activate: {kind: 'manual'}, arrival: 'stairs'},
                    {id: 'to-hall', from: 'cellar', to: 'hall', activate: {kind: 'manual'}, arrival: 'door'},
                ]
            },
        });
        const graph = root.querySelector('[aria-label="Scene connection graph"]')!;
        expect(graph.querySelectorAll('.node')).toHaveLength(3);
        expect(graph.querySelectorAll('.edge')).toHaveLength(3);
        await write('Filter by scene', 'arena');
        expect(graph.querySelectorAll('.edge')).toHaveLength(2);
        expect(Array.from(graph.querySelectorAll('select')[1].options).map(option => option.value))
            .toEqual(['', '0', '1']);
        await write('Route in graph', '1');
        expect(graph.querySelectorAll('.edge.selected')).toHaveLength(1);
        expect(Array.from(root.querySelectorAll('input')).some(field => field.value === 'to-cellar')).toBe(true);
        expect(root.querySelector('[aria-label="Scene routes"] button[aria-pressed="true"]')?.textContent)
            .toContain('to-cellar');
        expect(button('Apply Scene Flow').disabled).toBe(true);
        expect(execute).not.toHaveBeenCalled();
    });

    it('saves reusable cues and a scene binding and rejects missing role targets', async () => {
        const {button, click, write, execute} = await createFlow();
        await click('Timelines');
        await click('Add Timeline');
        await write('Timeline ID', 'gate-reveal');
        await write('Role ID', 'view');
        await click('Add Scene Instance');
        await write('Instance ID', 'reveal-from-hall');
        expect(button('Apply Scene Flow').disabled).toBe(true);
        await write('view object', 'gate-camera');
        await write('Entry sensor', 'gate-exit');
        await write('Triggering actor', 'knight');
        await click('Apply Scene Flow');
        const command = execute.mock.calls[0]?.[0];
        expect(command?.type).toBe('set_project');
        if (command?.type !== 'set_project') return;
        expect(command.project.sequences).toEqual({
            assets: [{
                id: 'gate-reveal', duration_ms: 3000, schema_version: 1, cues: [
                    {at_ms: 0, action: {kind: 'camera_cut', role: 'view'}},
                ]
            }],
            instances: [{
                id: 'reveal-from-hall', scene_id: 'hall', sequence_id: 'gate-reveal',
                bindings: {view: 'gate-camera'}, trigger: {sensor: 'gate-exit', actor: 'knight'}, once_per_entry: true,
            }],
        });
    });

    it('retains the draft on a stale project and never submits it over newer Scene Flow settings', async () => {
        const {fixture, root, click, write, execute, current} = await createFlow();
        await click('Add Route');
        await write('Route ID', 'open-gate');
        await write('Source sensor', 'door');
        await write('Arrival marker', 'arrival');
        current.update(snapshot => ({
            ...snapshot, revision: snapshot.revision + 1,
            project: {
                ...snapshot.project, scene_routes: [{
                    id: 'external', from: 'hall', to: 'arena', arrival: 'arrival', activate: {kind: 'manual'},
                }]
            },
        }));
        await click('Apply Scene Flow');
        expect(execute).not.toHaveBeenCalled();
        expect(root.querySelector('[role="alert"]')?.textContent).toContain('Scene Flow changed');
        expect(fixture.componentInstance.canLeaveProject()).toBe(false);
    });

    it('requests only a manual route from the current Play scene', async () => {
        const initial = snapshot();
        const {click, execute, button} = await createFlow({
            ...initial, mode: 'play', play_session_id: 'play-route',
            project: {
                ...initial.project, scene_routes: [
                    {id: 'manual', from: 'hall', to: 'arena', activate: {kind: 'manual'}, arrival: 'start'},
                ]
            },
        }, {
            play_session_id: 'play-route', run_id: 'run', scene_generation: 0, scene_id: 'hall',
            paused: false, pending_scene_id: null, gameplay: null,
        });
        expect(button('Apply Scene Flow').disabled).toBe(true);
        await click('Test Route');
        expect(execute).toHaveBeenCalledExactlyOnceWith({type: 'request_scene_route', route_id: 'manual'}, 7, 3);
    });

    it('shows region residency and retries the failed destination only during its Play session', async () => {
        const initial = snapshot();
        const {root, click, execute} = await createFlow({
            ...initial, mode: 'play', play_session_id: 'play-regions',
            project: {
                ...initial.project, world_regions: [{
                    world: 'hall', scene: 'arena', bounds: {min: [0, 0, 0], max: [20, 20, 20]},
                    preload_distance: 12, release_distance: 16,
                }]
            },
        }, {
            play_session_id: 'play-regions', run_id: 'run', scene_generation: 0, scene_id: 'hall',
            paused: false, pending_scene_id: null, gameplay: null, failed_scene_id: 'arena',
            failed_scene_attempt_id: 5, resident_regions: ['arena'],
        }, 4.5 * 1048576);
        await click('World Regions');
        expect(root.querySelector('.region-map .resident')?.getAttribute('aria-label')).toContain('loaded');
        expect(root.textContent).toContain('Active regions: 1');
        expect(root.textContent).toContain('GPU scene assets: 4.5 MiB');
        await click('Retry Transition');
        expect(execute).toHaveBeenCalledExactlyOnceWith({
            type: 'retry_scene_route', run_id: 'run', scene_generation: 0, attempt_id: 5,
        }, 7, 3);
    });

    it('ignores a retry from the previous Play session before the button refreshes', async () => {
        const failed: PlayStatus = {
            play_session_id: 'play-old', run_id: 'run-old', scene_generation: 0, scene_id: 'hall',
            paused: false, pending_scene_id: null, gameplay: null, failed_scene_id: 'arena',
            failed_scene_attempt_id: 1,
        };
        const {button, click, current, execute, fixture, status} = await createFlow({
            ...snapshot(), mode: 'play', play_session_id: 'play-old',
        }, failed);
        const oldButton = button('Retry Transition');

        current.update(current => ({...current, play_session_id: 'play-new', revision: 5}));
        status.set({...failed, play_session_id: 'play-new', run_id: 'run-new'});
        oldButton.click();
        await fixture.whenStable();
        expect(execute).not.toHaveBeenCalled();

        await click('Retry Transition');
        expect(execute).toHaveBeenCalledExactlyOnceWith({
            type: 'retry_scene_route', run_id: 'run-new', scene_generation: 0, attempt_id: 1,
        }, 7, 5);
    });

    it('ignores a retry from an earlier failure for the same destination in the same Play run', async () => {
        const failed: PlayStatus = {
            play_session_id: 'play-route', run_id: 'run', scene_generation: 0, scene_id: 'hall',
            paused: false, pending_scene_id: null, gameplay: null, failed_scene_id: 'arena',
            failed_scene_attempt_id: 1,
        };
        const {button, current, execute, fixture, status} = await createFlow({
            ...snapshot(), mode: 'play', play_session_id: 'play-route',
        }, failed);
        const oldButton = button('Retry Transition');

        current.update(current => ({...current, revision: 4}));
        status.set({...failed, failed_scene_attempt_id: 2});
        oldButton.click();
        await fixture.whenStable();
        expect(execute).not.toHaveBeenCalled();
    });

    it('validates region bounds and retains independent world and region scene IDs', async () => {
        const {button, click, write, execute} = await createFlow();
        await click('World Regions');
        await click('Add Region');
        await write('X', '30');
        expect(button('Apply Scene Flow').disabled).toBe(true);
        await write('X', '-10');
        await click('Apply Scene Flow');
        expect(execute).toHaveBeenCalledWith({
            type: 'set_project',
            project: expect.objectContaining({
                world_regions: [{
                    world: 'hall', scene: 'arena', bounds: {min: [-10, 0, 0], max: [20, 20, 20]},
                    preload_distance: 12, release_distance: 16,
                }],
            }),
        }, 7, 3);
    });
});
