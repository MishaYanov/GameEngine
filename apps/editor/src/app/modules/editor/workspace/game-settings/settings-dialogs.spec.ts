import {computed, signal} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import axe from 'axe-core';
import {vi} from 'vitest';
import type {EditorCommand, EditorSnapshot} from '../../../../core/editor/editor.models';
import {EditorService} from '../../../../core/editor/editor.service';
import {LAYOUT_PREFERENCES_STORAGE} from '../../../../shared/services/layout-preferences-storage';
import {SceneEditingService} from '../scene-editing.service';
import {CameraSettings} from './camera-settings';
import {GameplaySettings} from './gameplay-settings';

function snapshot(): EditorSnapshot {
    return {
        revision: 7, scene_revision: 3, active_scene_id: 'main', mode: 'edit', paused: false,
        dirty: false, project_dirty: false, transaction: null, selection: ['player'], can_undo: false, can_redo: false,
        snap: {enabled: false, translation: 1, rotation_radians: 0.1, scale: 0.1},
        scenes: ['main', 'ending'].map(id => ({id, asset_path: `scenes/${id}.json`, loaded: true, dirty: false})),
        project: {
            schema_version: 3, name: 'Settings fixture', gameplay_module: 'courtyard_run',
            scenes: ['main', 'ending'].map(id => ({id, asset_path: `scenes/${id}.json`})),
            startup_scene_id: 'main', fixed_hz: 60, max_catchup_steps: 8, behaviors: [], behavior_definitions: [],
            gameplay_settings: {schema_version: 1, scenes: {ending: {kind: 'finish', finish_trigger: 'goal'}}},
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

describe('game settings dialogs', () => {
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

    async function setup(kind: 'camera' | 'interactions') {
        const current = signal(snapshot());
        const generation = signal(1);
        const execute = vi.fn(async (command: EditorCommand, _generation?: number, _revision?: number) => {
            current.update(value => command.type === 'set_project' ? {...value, project: command.project}
                : command.type === 'set_camera' ? {...value, document: {...value.document, camera: command.camera}} : value);
            return current();
        });
        const committed = vi.fn(async (action: () => Promise<void>) => { await action(); return true; });
        TestBed.configureTestingModule({providers: [
            {provide: LAYOUT_PREFERENCES_STORAGE, useValue: {load: async () => null, save: async () => undefined}},
            {provide: EditorService, useValue: {
                nativeAvailable: true, snapshot: current, workspaceGeneration: generation, busy: signal(false),
                playing: computed(() => current().mode === 'play'), execute,
            }},
            {provide: SceneEditingService, useValue: {ready: () => true, error: () => null, withCommittedEdit: committed}},
        ]});
        const fixture = kind === 'camera' ? TestBed.createComponent(CameraSettings) : TestBed.createComponent(GameplaySettings);
        await fixture.whenStable();
        const root: HTMLElement = fixture.nativeElement;
        root.querySelector<HTMLButtonElement>('button')!.click();
        await fixture.whenStable();
        const button = (text: string) => [...root.querySelectorAll('button')].find(button => button.textContent.trim() === text)!;
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
        return {fixture, root, current, generation, execute, committed, button, field, change};
    }

    it('authors a scene sensor override while preserving other scenes and latest project edits', async () => {
        const panel = await setup('interactions');
        await panel.change('Interaction mode', 'course');
        await panel.change('Key trigger', 'key-sensor');
        await panel.change('Exit trigger', 'exit-sensor');
        await panel.change('Destination scene', 'ending');
        panel.committed.mockImplementationOnce(async action => {
            panel.current.update(value => ({...value, revision: 11, project: {...value.project, name: 'Latest'}}));
            await action(); return true;
        });
        panel.button('Apply Interaction Settings').click();
        await panel.fixture.whenStable();
        expect(panel.execute).toHaveBeenCalledOnce();
        const [command, generation, revision] = panel.execute.mock.calls[0];
        expect(generation).toBe(1); expect(revision).toBe(11);
        expect(command.type).toBe('set_project');
        if (command.type !== 'set_project') throw new Error('Expected SetProject.');
        expect(command.project.name).toBe('Latest');
        expect(command.project.gameplay_settings).toEqual({schema_version: 1, scenes: {
            ending: {kind: 'finish', finish_trigger: 'goal'},
            main: {kind: 'course', key_trigger: 'key-sensor', exit_trigger: 'exit-sensor', destination_scene: 'ending', exit_blocker: null},
        }});
        await panel.change('Interaction mode', 'legacy');
        panel.button('Apply Interaction Settings').click();
        await panel.fixture.whenStable();
        expect(panel.current().project.gameplay_settings).toEqual({schema_version: 1, scenes: {ending: {kind: 'finish', finish_trigger: 'goal'}}});
    });

    it('applies Follow to the latest fixed camera without replacing its pose', async () => {
        const panel = await setup('camera');
        await panel.change('Mode', 'follow');
        await panel.change('Target', 'blocker');
        panel.committed.mockImplementationOnce(async action => {
            panel.current.update(value => ({...value, revision: 12, document: {...value.document,
                camera: {...value.document.camera, position: [2, 3, 4]}}}));
            await action(); return true;
        });
        panel.button('Apply Game Camera').click();
        await panel.fixture.whenStable();
        expect(panel.execute).toHaveBeenCalledExactlyOnceWith({
            type: 'set_camera', camera: {...snapshot().document.camera, position: [2, 3, 4],
                follow: {target: 'blocker', offset: [0, 2, 5], look_offset: [0, 0, 0], smoothing_seconds: 0.15}
            },
        }, 1, 12);
        await panel.change('Smoothing in seconds', '1e-');
        expect(panel.button('Apply Game Camera').disabled).toBe(true);
    });

    it('reveals an existing orbit target without offering a conflicting follow edit', async () => {
        const panel = await setup('camera');
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
        const target = panel.root.querySelector<HTMLElement>('[aria-label="Orbit camera"] p');
        expect(target?.textContent).toContain('Missing target: missing-player');
        expect(panel.button('Apply Game Camera')).toBeUndefined();
        expect(document.activeElement).toBe(target);
        expect(panel.execute).not.toHaveBeenCalled();
    });

    it('requires a different Finish destination and enabled mutually compatible sensors', async () => {
        const panel = await setup('interactions');
        await panel.change('Interaction mode', 'course');
        await panel.change('Key trigger', 'key-sensor');
        await panel.change('Exit trigger', 'exit-sensor');
        await panel.change('Destination scene', 'main');
        expect(panel.button('Apply Interaction Settings').disabled).toBe(true);
        expect(panel.root.textContent).toContain('different registered destination');
        await panel.change('Destination scene', 'ending');
        expect(panel.button('Apply Interaction Settings').disabled).toBe(false);
        panel.current.update(value => ({...value, document: {...value.document,
            entities: value.document.entities.map(entity => entity.key === 'key-sensor' ? {...entity, collision_enabled: false} : entity)}}));
        await panel.fixture.whenStable();
        expect(panel.button('Apply Interaction Settings').disabled).toBe(true);
        panel.current.update(value => ({...value, project: {...value.project, gameplay_settings: {schema_version: 1, scenes: {}}}}));
        await panel.fixture.whenStable();
        expect(panel.button('Apply Interaction Settings').disabled).toBe(true);
    });

    it('preserves unsupported settings payloads and prevents authoring over unknown fields', async () => {
        const panel = await setup('interactions');
        const payload = {schema_version: 1, scenes: {}, future: {retained: true}};
        panel.current.update(value => ({...value, project: {...value.project, gameplay_settings: payload}}));
        await panel.fixture.whenStable();
        expect(panel.button('Apply Interaction Settings').disabled).toBe(true);
        expect(panel.root.querySelector('[role="alert"]')?.textContent).toContain('unsupported');
        expect(panel.current().project.gameplay_settings).toBe(payload);
        expect(panel.execute).not.toHaveBeenCalled();
    });

    it.each(['camera', 'interactions'] as const)('rejects a stale %s dialog before sending and has accessible controls', async kind => {
        const panel = await setup(kind);
        panel.committed.mockImplementationOnce(async action => { panel.generation.set(2); await action(); return true; });
        panel.button(kind === 'camera' ? 'Apply Game Camera' : 'Apply Interaction Settings').click();
        await panel.fixture.whenStable();
        expect(panel.execute).not.toHaveBeenCalled();
        expect(panel.root.querySelector('[role="alert"]')).toBeNull();
        panel.root.querySelector<HTMLButtonElement>('button')!.click();
        await panel.fixture.whenStable();
        const report = await axe.run(panel.root, {rules: {'color-contrast': {enabled: false}}});
        expect(report.violations.map(violation => violation.id)).toEqual([]);
    });
});
