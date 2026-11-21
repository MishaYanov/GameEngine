import {computed, signal} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import axe from 'axe-core';
import {vi} from 'vitest';
import type {CampaignDefinition, EditorCommand, EditorSnapshot} from '../../../../core/editor/editor.models';
import {EditorClientError, EditorService} from '../../../../core/editor/editor.service';
import {
    CameraPreviewController,
    type PreviewDefinition,
    type PreviewIntent
} from '../../../../core/editor/camera-preview';
import {ViewportEditorService} from '../../../../core/editor/viewport-editor.service';
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

    async function setup(kind: 'camera' | 'interactions' | 'campaign', openDialog = true) {
        const original = snapshot();
        const current = signal(original);
        if (kind === 'campaign') {
            const scene = {id: 'garden', asset_path: 'scenes/garden.json'};
            current.set({
                ...original,
                scenes: [...original.scenes, {...scene, loaded: true, dirty: false}],
                project: {
                    ...original.project, scenes: [...original.project.scenes, scene],
                    gameplay_settings: {
                        schema_version: 2, scenes: {
                            main: {
                                kind: 'course',
                                key_trigger: 'key-sensor',
                                exit_trigger: 'exit-sensor',
                                destination_scene: 'garden'
                            },
                            garden: {
                                kind: 'course',
                                key_trigger: 'garden-key',
                                exit_trigger: 'garden-exit',
                                destination_scene: 'ending'
                            },
                            ending: {kind: 'finish', finish_trigger: 'goal'},
                        }, campaign: {
                            chapters: [
                                {id: 'main', label: 'Main', course_scene: 'main'},
                                {id: 'garden', label: 'Garden', course_scene: 'garden'},
                            ], finish_scene: 'ending'
                        }
                    },
                },
            });
        }
        const generation = signal(1);
        const campaignGeneration = vi.fn(async (_generation: number, _revision: number) => 42);
        const applyCampaign = vi.fn(async (
            definition: CampaignDefinition, _generation: number, _nativeGeneration: number, _revision: number,
        ) => {
            current.update(value => ({
                ...value, revision: value.revision + 1, project_dirty: true,
                project: {...value.project, startup_scene_id: definition.chapters[0].course_scene},
            }));
            return current();
        });
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
                    campaignGeneration,
                    applyCampaign,
                    cameraPreview: new CameraPreviewController(previewApi),
            }},
                {provide: ViewportEditorService, useValue: {state: signal({workspace_generation: 42})}},
            {provide: SceneEditingService, useValue: {ready: () => true, error: () => null, withCommittedEdit: committed}},
        ]});
        const fixture = kind === 'camera' ? TestBed.createComponent(CameraSettings) : TestBed.createComponent(GameplaySettings);
        await fixture.whenStable();
        const root: HTMLElement = fixture.nativeElement;
        const button = (text: string) => [...root.querySelectorAll('button')].find(button => button.textContent.trim() === text)!;
        if (openDialog) {
            if (kind === 'campaign') button('Campaign Settings…').click();
            else root.querySelector<HTMLButtonElement>('button')!.click();
        }
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
            execute,
            committed,
            button,
            field,
            change,
            previewApi,
            campaignGeneration,
            applyCampaign
        };
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

    it('binds a Route Gate from a disabled authored lever and preserves prior settings on native failure', async () => {
        const panel = await setup('interactions');
        const before = panel.current().project.gameplay_settings;
        panel.current.update(value => ({
            ...value,
            project: {
                ...value.project, input_profile: {
                    schema_version: 1, movement: {negative_x: 'a', positive_x: 'd', negative_y: 's', positive_y: 'w'},
                    actions: [{id: 'interact', key: 'e'}],
                }
            },
            document: {
                ...value.document, entities: [
                    ...value.document.entities,
                    {
                        ...value.document.entities[0], key: 'lever', name: 'Lever', component_views: [{
                            id: 'use', schema_version: 1, enabled: false,
                            data: {kind: 'interactable', settings: {action: 'interact', reach: 1.25}},
                        }]
                    },
                ]
            },
        }));
        await panel.change('Interaction mode', 'course');
        await panel.change('Key trigger', 'key-sensor');
        await panel.change('Exit trigger', 'exit-sensor');
        await panel.change('Destination scene', 'ending');
        panel.field('Use a route lever').click();
        await panel.fixture.whenStable();
        expect(panel.button('Apply Interaction Settings').disabled).toBe(true);
        await panel.change('Route lever', 'lever');
        await panel.change('Route gate collider', 'blocker');
        expect(panel.root.textContent).toContain('Inactive Interactable');
        expect(panel.button('Apply Interaction Settings').disabled).toBe(false);
        panel.execute.mockRejectedValueOnce(new Error('Scene was changed'));
        panel.button('Apply Interaction Settings').click();
        await panel.fixture.whenStable();
        expect(panel.root.textContent).toContain('Scene was changed');
        expect(panel.current().project.gameplay_settings).toEqual(before);
        panel.button('Apply Interaction Settings').click();
        await panel.fixture.whenStable();
        expect(panel.current().project.gameplay_settings).toMatchObject({
            route_gate: {scene_id: 'main', lever: 'lever', gate: 'blocker', action: 'interact', radius: 1.25},
        });
    });

    it('reorders chapters with focus, previews the startup change, and sends one reviewed draft', async () => {
        const panel = await setup('campaign');
        const ids = () => [...panel.root.querySelectorAll<HTMLInputElement>('ol li input')].filter((_, index) => index % 2 === 0);
        expect(panel.campaignGeneration).toHaveBeenCalledExactlyOnceWith(1, 7);
        expect(panel.button('Apply Campaign').disabled).toBe(true);
        expect(panel.root.textContent).toContain('Exit leads to scenes/ending.json (ending).');
        panel.root.querySelector<HTMLButtonElement>('[aria-label="Move chapter 2 up"]')!.click();
        await panel.fixture.whenStable();
        expect(ids().map(input => input.value)).toEqual(['garden', 'main']);
        expect(document.activeElement).toBe(ids()[0]);
        expect(panel.root.textContent).toContain("Applying will change the project's startup scene.");
        panel.button('Close').click();
        await panel.fixture.whenStable();
        expect(panel.applyCampaign).not.toHaveBeenCalled();

        panel.button('Campaign Settings…').click();
        await panel.fixture.whenStable();
        expect(ids().map(input => input.value)).toEqual(['main', 'garden']);
        panel.root.querySelector<HTMLButtonElement>('[aria-label="Move chapter 2 up"]')!.click();
        await panel.fixture.whenStable();
        panel.button('Apply Campaign').click();
        await panel.fixture.whenStable();
        expect(panel.applyCampaign).toHaveBeenCalledExactlyOnceWith({
            chapters: [
                {id: 'garden', label: 'Garden', course_scene: 'garden'},
                {id: 'main', label: 'Main', course_scene: 'main'},
            ], finish_scene: 'ending'
        }, 1, 42, 7);
        expect(panel.root.textContent).toContain('Campaign settings applied. Save All');
    });

    it('ignores a campaign response that arrives during workspace navigation', async () => {
        const panel = await setup('campaign', false);
        let respond!: (generation: number) => void;
        panel.campaignGeneration.mockReturnValueOnce(new Promise(resolve => {
            respond = resolve;
        }));
        panel.button('Campaign Settings…').click();
        await vi.waitFor(() => expect(panel.campaignGeneration).toHaveBeenCalledOnce());
        panel.fixture.componentRef.setInput('opening', true);
        await panel.fixture.whenStable();
        respond(42);
        await panel.fixture.whenStable();
        expect(panel.root.querySelector('dialog[open]')).toBeNull();

        panel.fixture.componentRef.setInput('opening', false);
        await panel.fixture.whenStable();
        panel.button('Campaign Settings…').click();
        await panel.fixture.whenStable();
        expect(panel.root.querySelector('dialog[open]')).not.toBeNull();
    });

    it('keeps a rejected campaign draft and focuses the diagnosed chapter', async () => {
        const panel = await setup('campaign');
        await panel.change('Chapter label', 'New title');
        const before = panel.current().project.gameplay_settings;
        panel.applyCampaign.mockRejectedValueOnce(new EditorClientError(
            'backend', 'editor_apply_campaign', 'Scene "garden": Exit trigger needs an enabled sensor collider',
            {code: 'campaign_validation', scene_id: 'garden'},
        ));
        panel.button('Apply Campaign').click();
        await panel.fixture.whenStable();
        expect(panel.root.querySelector('[role="alert"]')?.textContent).toContain('Exit trigger');
        expect(panel.field('Chapter label').value).toBe('New title');
        expect((document.activeElement as HTMLInputElement).value).toBe('garden');
        expect(panel.current().project.gameplay_settings).toEqual(before);
        const dialog = [...panel.root.querySelectorAll('dialog')].find(element => element.open)!;
        expect((await axe.run(dialog, {rules: {'color-contrast': {enabled: false}}})).violations).toEqual([]);
    });

    it('applies Follow once using the latest unrelated revision and preserves the pose', async () => {
        const panel = await setup('camera');
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
        expect(panel.root.textContent).toContain('Missing target: missing-player');
        expect(panel.button('Apply Game Camera').disabled).toBe(true);
        expect(document.activeElement).toBe(panel.field('Target'));
        expect(panel.execute).not.toHaveBeenCalled();
    });

    it('cancels without changing the scene and rejects a concurrent camera projection edit', async () => {
        const panel = await setup('camera');
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
        const panel = await setup('camera');
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
        const panel = await setup('camera');
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
        const panel = await setup('camera');
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
        const panel = await setup('camera');
        await panel.change('Mode', mode);
        if (mode === 'orbit') {
            await panel.change('Initial distance', '-');
            expect(panel.field('Initial distance').value).toBe('-');
            expect(panel.button('Preview Orbit').disabled).toBe(true);
        }
        const report = await axe.run(panel.root, {rules: {'color-contrast': {enabled: false}}});
        expect(report.violations.map(v => v.id)).toEqual([]);
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
