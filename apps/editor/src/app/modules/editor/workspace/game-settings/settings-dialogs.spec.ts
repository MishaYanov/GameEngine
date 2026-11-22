import {computed, signal} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import axe from 'axe-core';
import {vi} from 'vitest';
import type {

    EditorCommand,
    EditorSnapshot
} from '../../../../core/editor/editor.models';
import {EditorService} from '../../../../core/editor/editor.service';
import {
    CameraPreviewController,
    type PreviewDefinition,
    type PreviewIntent
} from '../../../../core/editor/camera-preview';
import {ViewportEditorService} from '../../../../core/editor/viewport-editor.service';
import {LAYOUT_PREFERENCES_STORAGE} from '../../../../shared/services/layout-preferences-storage';
import {SceneEditingService} from '../scene-editing.service';
import {CameraSettings} from './camera-settings';

function snapshot(): EditorSnapshot {
    return {
        revision: 7, scene_revision: 3, active_scene_id: 'main', mode: 'edit', paused: false,
        dirty: false, project_dirty: false, transaction: null, selection: ['player'], can_undo: false, can_redo: false,
        snap: {enabled: false, translation: 1, rotation_radians: 0.1, scale: 0.1},
        scenes: ['main', 'ending'].map(id => ({id, asset_path: `scenes/${id}.json`, loaded: true, dirty: false})),
        project: {
            schema_version: 3, name: 'Settings fixture',
            scenes: ['main', 'ending'].map(id => ({id, asset_path: `scenes/${id}.json`})),
            startup_scene_id: 'main', fixed_hz: 60, max_catchup_steps: 8, behaviors: [], behavior_definitions: [],
        },
        document: {
            schema_version: 3, bindings: {player: 'player'}, lights: [],
            camera: {position: [0, 0, 5], yaw: 0, pitch: 0, vertical_fov: 1, near: 0.1, far: 100},
            entities: ['player', 'key-sensor', 'exit-sensor', 'blocker'].map((key, index) => ({
                key, name: key, renderable: true, model_path: 'props/cube.glb',
                transform: {position: [0, 0, 0], rotation: [0, 0, 0], scale: [1, 1, 1]},
                collider: {kind: 'sphere', radius: 0.5, offset: [0, 0, 0],
                    interaction: {sensor: index === 1 || index === 2, layers: 1, mask: 4294967295}},
            })),
        },
    };
}

describe('game camera settings', () => {
    const prototype = HTMLDialogElement.prototype;
    const show = Object.getOwnPropertyDescriptor(prototype, 'showModal');
    const close = Object.getOwnPropertyDescriptor(prototype, 'close');
    beforeAll(() => {
        Object.defineProperty(prototype, 'showModal', {configurable: true, value(this: HTMLDialogElement) { this.setAttribute('open', ''); }});
        Object.defineProperty(prototype, 'close', {configurable: true, value(this: HTMLDialogElement) {
            this.removeAttribute('open'); this.dispatchEvent(new Event('close'));
        }});
    });
    afterAll(() => {
        if (show) Object.defineProperty(prototype, 'showModal', show);
        else Reflect.deleteProperty(prototype, 'showModal');
        if (close) Object.defineProperty(prototype, 'close', close);
        else Reflect.deleteProperty(prototype, 'close');
    });

    async function setup() {
        const original = snapshot();
        const current = signal(original);
        const generation = signal(1);
        const editingReady = signal(true);
        const execute = vi.fn(async (command: EditorCommand, _generation?: number, _revision?: number) => {
            current.update(value => command.type === 'set_project' ? {...value, project: command.project}
                : command.type === 'set_camera' ? {...value, document: {...value.document, camera: command.camera}} : value);
            return current();
        });
        const committed = vi.fn(async (action: () => Promise<void>) => { await action(); return true; });
        const reply = {
            token: 'test-token', workspace_generation: 42, scene_id: 'main', revision: 7,
            sample: 0, effective_distance: 3.5, required_probe_radius: 0.2, target_lost: false, blocker: 'wall'
        };
        const previewApi = {
            open: vi.fn(async (_: PreviewDefinition) => reply),
            sample: vi.fn(async (_: string, sample: number, _intent: PreviewIntent) => ({...reply, sample})),
            close: vi.fn(async (_: string) => undefined),
        };
        TestBed.configureTestingModule({providers: [
            {provide: LAYOUT_PREFERENCES_STORAGE, useValue: {load: async () => null, save: async () => undefined}},
            {provide: EditorService, useValue: {
                nativeAvailable: true, snapshot: current, workspaceGeneration: generation, busy: signal(false),
                    playing: computed(() => current().mode === 'play'),
                    execute,

                    cameraPreview: new CameraPreviewController(previewApi),
            }},
                {provide: ViewportEditorService, useValue: {state: signal({workspace_generation: 42})}},
                {
                    provide: SceneEditingService,
                    useValue: {ready: editingReady, error: () => null, withCommittedEdit: committed}
                },
        ]});
        const fixture = TestBed.createComponent(CameraSettings);
        await fixture.whenStable();
        const root: HTMLElement = fixture.nativeElement;
        const button = (text: string) => [...root.querySelectorAll('button')].find(button => button.textContent.trim() === text)!;
        button('Game Camera…').click();
        await fixture.whenStable();
        const field = (text: string) => {
            const label = [...root.querySelectorAll('label')].find(label => label.textContent?.trim().startsWith(text))!;
            return label.htmlFor ? root.querySelector<HTMLInputElement>(`#${label.htmlFor}`)!
                : label.querySelector<HTMLInputElement | HTMLSelectElement>('input,select')!;
        };
        const change = async (text: string, value: string) => {
            const input = field(text); input.value = value; input.dispatchEvent(new Event('input'));
            await fixture.whenStable();
            if (input.getAttribute('role') === 'combobox') {
                input.dispatchEvent(new KeyboardEvent('keydown', {key: 'Enter'}));
                await fixture.whenStable();
            }
        };
        return {
            fixture,
            root,
            current,
            generation,
            editingReady,
            execute,
            committed,
            button,
            field,
            change,
            previewApi,

        };
    }

    it('applies Follow once using the latest unrelated revision and preserves the pose', async () => {
        const panel = await setup();
        await panel.change('Mode', 'follow');
        await panel.change('Target', 'blocker');
        panel.committed.mockImplementationOnce(async action => {
            panel.current.update(value => ({...value, revision: 12}));
            await action(); return true;
        });
        panel.button('Apply Game Camera').click();
        await panel.fixture.whenStable();
        expect(panel.execute).toHaveBeenCalledExactlyOnceWith({
            type: 'set_camera', camera: {
                ...snapshot().document.camera, orbit: null,
                follow: {target: 'blocker', offset: [0, 2, 5], look_offset: [0, 0, 0], smoothing_seconds: 0.15}
            },
        }, 1, 12);
        await panel.change('Smoothing in seconds', '1e-');
        expect(panel.button('Apply Game Camera').disabled).toBe(true);
    });

    it('focuses an invalid orbit target for repair and keeps Apply disabled until the draft is valid', async () => {
        const panel = await setup();
        panel.current.update(value => ({
            ...value, document: {
                ...value.document, camera: {
                    ...value.document.camera,
                    orbit: {
                        target: 'missing-player', pivot_offset: [0, 1, 0], initial_yaw: 0, initial_pitch: 0,
                        initial_distance: 5, min_distance: 2, max_distance: 8, min_pitch: -1, max_pitch: 1,
                        follow_seconds: 0.15, recovery_seconds: 0.15, teleport_distance: 10,
                        probe_radius: 0.2, obstruction_mask: 1,
                    },
                }
            }
        }));
        const camera = panel.fixture.componentInstance;
        if (!(camera instanceof CameraSettings)) throw new Error('Expected camera settings');
        expect(camera.show(true, 'orbit')).toBe(true);
        await panel.fixture.whenStable();
        expect(panel.root.textContent).toContain('Missing target: missing-player');
        expect(panel.button('Apply Game Camera').disabled).toBe(true);
        expect(document.activeElement).toBe(panel.field('Target'));
        expect(panel.execute).not.toHaveBeenCalled();
    });

    it('cancels without changing the scene and rejects a concurrent camera projection edit', async () => {
        const panel = await setup();
        await panel.change('Mode', 'orbit');
        panel.button('Close').click();
        await panel.fixture.whenStable();
        expect(panel.execute).not.toHaveBeenCalled();
        panel.button('Game Camera…').click();
        await panel.fixture.whenStable();
        expect(panel.field('Mode').value).toBe('fixed');
        await panel.change('Mode', 'orbit');
        panel.committed.mockImplementationOnce(async action => {
            panel.current.update(s => ({
                ...s, revision: 8, document: {
                    ...s.document,
                    camera: {...s.document.camera, far: 300}
                }
            }));
            await action();
            return true;
        });
        panel.button('Apply Game Camera').click();
        await panel.fixture.whenStable();
        expect(panel.root.querySelector('[role="alert"]')?.textContent).toContain('Camera settings changed');
        expect(panel.execute).not.toHaveBeenCalled();
    });

    it('checks the target again after committing an ongoing edit', async () => {
        const panel = await setup();
        await panel.change('Mode', 'orbit');
        panel.committed.mockImplementationOnce(async action => {
            panel.current.update(s => ({
                ...s, revision: 8, document: {
                    ...s.document,
                    entities: s.document.entities.filter(e => e.key !== 'player')
                }
            }));
            await action();
            return true;
        });
        panel.button('Apply Game Camera').click();
        await panel.fixture.whenStable();
        expect(panel.root.querySelector('[role="alert"]')?.textContent).toContain('target');
        expect(panel.execute).not.toHaveBeenCalled();
    });

    it('moves preview controls out of the modal, samples the frozen camera and returns to the same draft', async () => {
        const panel = await setup();
        await panel.change('Mode', 'orbit');
        await panel.change('Initial distance', '7');
        panel.button('Preview Orbit').click();
        await panel.fixture.whenStable();
        expect(panel.root.querySelector('dialog[open]')).toBeNull();
        expect(panel.previewApi.open).toHaveBeenCalledWith(expect.objectContaining({
            expectedGeneration: 42, expectedRevision: 7, sceneId: 'main',
        }));
        expect(panel.root.textContent).toContain('Blocked by wall');
        panel.button('Turn right').click();
        await panel.fixture.whenStable();
        expect(panel.previewApi.sample).toHaveBeenCalledWith('test-token', 1, {
            yaw_delta: 15 * Math.PI / 180, pitch_delta: 0, log_distance_delta: 0
        });
        panel.button('Back to settings').click();
        await panel.fixture.whenStable();
        expect(panel.previewApi.close).toHaveBeenCalledWith('test-token');
        expect(panel.field('Initial distance').value).toBe('7');
        expect(panel.execute).not.toHaveBeenCalled();
        panel.button('Apply Game Camera').click();
        panel.button('Apply Game Camera').click();
        await panel.fixture.whenStable();
        expect(panel.execute).toHaveBeenCalledTimes(1);
        expect(panel.current().document.camera.orbit?.initial_distance).toBe(7);
        expect(panel.current().document.camera.follow).toBeNull();
    });

    it('preserves a reopened draft while an earlier Apply is awaiting the edit coordinator', async () => {
        const panel = await setup();
        await panel.change('Mode', 'orbit');
        let release!: () => void;
        const gate = new Promise<void>(resolve => {
            release = resolve;
        });
        panel.committed.mockImplementationOnce(async action => {
            await gate;
            await action();
            return true;
        });
        panel.button('Apply Game Camera').click();
        panel.button('Close').click();
        await panel.fixture.whenStable();
        panel.button('Game Camera…').click();
        await panel.fixture.whenStable();
        await panel.change('Mode', 'follow');
        release();
        await panel.fixture.whenStable();
        expect(panel.field('Mode').value).toBe('follow');
        expect(panel.execute).not.toHaveBeenCalled();
    });

    it.each(['fixed', 'follow', 'orbit'])('exposes accessible %s camera controls', async mode => {
        const panel = await setup();
        await panel.change('Mode', mode);
        if (mode === 'orbit') {
            await panel.change('Initial distance', '-');
            expect(panel.field('Initial distance').value).toBe('-');
            expect(panel.button('Preview Orbit').disabled).toBe(true);
        }
        const report = await axe.run(panel.root, {rules: {'color-contrast': {enabled: false}}});
        expect(report.violations.map(v => v.id)).toEqual([]);
    });

});
