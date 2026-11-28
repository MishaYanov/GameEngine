import {computed, signal} from '@angular/core';
import {type ComponentFixture, TestBed} from '@angular/core/testing';
import {vi} from 'vitest';
import axe from 'axe-core';
import type {EditorSnapshot, GameplayModuleDescriptor, SceneEntity} from '../../../../core/editor/editor.models';
import {EditorService} from '../../../../core/editor/editor.service';
import {type SceneIssue, type SceneVerificationReport, validSceneVerification} from '../../../../core/editor/scene-verification.models';
import {EditorSettingsService} from '../../../../core/editor/settings/editor-settings.service';
import {ViewportEditorService} from '../../../../core/editor/viewport-editor.service';
import type {EditorWarmingMode, SceneWarmingResult} from '../../../../core/editor/viewport-editor.models';
import {LAYOUT_PREFERENCES_STORAGE} from '../../../../shared/services/layout-preferences-storage';
import {SceneEditingService} from '../scene-editing.service';
import {SceneActions} from './scene-actions';
import {SceneSwitchService} from './scene-switch.service';
import {WorkspaceDraftReview} from '../workspace-draft-review';

const sampleModule: GameplayModuleDescriptor = {
    id: 'sample_module', label: 'Sample module', description: 'A test module with per-scene role metadata.',
    default_roles: [
        {name: 'player', label: 'Player', requirement_hint: 'Choose a movable player.'},
        {name: 'key', label: 'Key', requirement_hint: 'Choose the key marker.'},
        {name: 'exit', label: 'Exit', requirement_hint: 'Choose the exit marker.'},
    ],
    scene_roles: {
        finish: [
            {name: 'player', label: 'Player', requirement_hint: 'Choose a movable player.'},
            {name: 'finish', label: 'Finish', requirement_hint: 'Choose the finish marker.'},
        ],
    },
};

function entity(key: string): SceneEntity {
    return {
        key, name: key, model_path: 'models/robot.glb', renderable: true,
        transform: {position: [0, 0, 0], rotation: [0, 0, 0], scale: [1, 1, 1]},
    };
}

function choose(select: HTMLSelectElement, value: string): void {
    select.value = value;
    select.dispatchEvent(new Event('change'));
}

function scenePicker(root: ParentNode): HTMLInputElement {
    return root.querySelector<HTMLInputElement>('.scene-picker [role="combobox"]')!;
}

function bindingPicker(root: ParentNode): HTMLInputElement {
    return root.querySelector<HTMLInputElement>('[data-scene-binding] [role="combobox"]')!;
}

function pickerOptions(fixture: ComponentFixture<SceneActions>, control: HTMLInputElement): HTMLElement[] {
    control.click();
    fixture.detectChanges();
    return Array.from(control.closest('editor-search-picker')!.querySelectorAll<HTMLElement>('[role="option"]'));
}

function pick(fixture: ComponentFixture<SceneActions>, control: HTMLInputElement, key: string): void {
    if (key === '') {
        control.closest('editor-search-picker')!.querySelector<HTMLButtonElement>('button')!.click();
        return;
    }
    const options = pickerOptions(fixture, control);
    options.find(option => option.id.endsWith('-option-' + encodeURIComponent(key)))!.click();
}

function snapshot(): EditorSnapshot {
    return {
        active_scene_id: 'main', revision: 1, scene_revision: 1,
        project_dirty: false, dirty: false, can_undo: false, can_redo: false,
        mode: 'edit', paused: false, selection: [], transaction: null,
        scenes: [
            {id: 'main', asset_path: 'scenes/main.json', dirty: false, loaded: true},
            {id: 'second', asset_path: 'scenes/second.json', dirty: false, loaded: false},
        ],
        project: {
            schema_version: 3, name: 'Scene actions fixture', startup_scene_id: 'main',
            gameplay_module: sampleModule.id,
            scenes: [{id: 'main', asset_path: 'scenes/main.json'}, {id: 'second', asset_path: 'scenes/second.json'}],
            fixed_hz: 60, max_catchup_steps: 8, behaviors: [], behavior_definitions: [],
        },
        document: {
            schema_version: 3, bindings: {}, entities: [], lights: [],
            camera: {position: [0, 0, 3], yaw: 0, pitch: 0, vertical_fov: 1, near: 0.1, far: 100},
        },
        snap: {enabled: false, translation: 1, rotation_radians: 0.1, scale: 0.1},
    };
}

async function createActions(
    initial = snapshot(),
    modules: readonly GameplayModuleDescriptor[] | undefined = [sampleModule],
    openInformation = true,
) {
    const current = signal(initial);
    const busy = signal(false);
    const generation = signal(1);
    const nativeGeneration = signal<number | null>(17);
    const authoringGeneration = signal(17);
    const result = signal<SceneWarmingResult | null>(null);
    const mode = signal<EditorWarmingMode>('references');
    const capabilities = signal<{
        gameplay_modules: readonly GameplayModuleDescriptor[] | undefined;
        scene_warming_supported?: boolean;
        scene_verification_supported?: boolean;
        commands?: readonly { readonly name: string }[];
    }>({
        gameplay_modules: modules, scene_warming_supported: true,
    });
    const switchScene = vi.fn(async (sceneId: string) => {
        current.update(value => ({...value, active_scene_id: sceneId, scene_revision: value.scene_revision + 1}));
        return current();
    });
    const save = vi.fn(async () => {
        current.update(value => ({
            ...value,
            revision: value.revision + 1,
            scenes: value.scenes.map(scene => scene.id === value.active_scene_id ? {...scene, dirty: false} : scene),
        }));
        return current();
    });
    const warmScene = vi.fn<EditorService['warmScene']>(async () => undefined);
    const setSceneWarmingMode = vi.fn<EditorService['setSceneWarmingMode']>(async selected =>
        selected === 'models' ? 2 : selected === 'references' ? 1 : 0);
    const exportWarmingReport = vi.fn<EditorService['exportWarmingReport']>(async () => 3);
    const checkScenes = vi.fn<EditorService['checkScenes']>();
    const authoringScope = vi.fn<EditorService['authoringScope']>(async workspaceGeneration => ({
        workspaceGeneration, nativeGeneration: authoringGeneration(), revision: current().revision,
    }));
    const createScene = vi.fn<EditorService['createScene']>(async path => {
        current.update(value => ({
            ...value, revision: value.revision + 1, project_dirty: true,
            scenes: [...value.scenes, {id: 'new-scene', asset_path: path, dirty: false, loaded: false}],
        }));
        return current();
    });
    const registerScene = vi.fn(async (_path: string) => current());
    const saveSceneAs = vi.fn(async (_path: string) => current());
    const execute = vi.fn<EditorService['execute']>(async () => current());
    const withCommittedEdit = vi.fn(async (action: () => Promise<void>) => {
        await action();
        return true;
    });
    TestBed.configureTestingModule({providers: [
            SceneSwitchService, WorkspaceDraftReview,
            {provide: LAYOUT_PREFERENCES_STORAGE, useValue: {load: async () => null, save: async () => undefined}},
            {provide: EditorSettingsService, useValue: {sceneWarmingReady: signal(true), sceneWarmingMode: mode}},
            {
                provide: ViewportEditorService,
                useValue: {
                    state: computed(() => ({
                        workspace_generation: nativeGeneration(),
                        scene_warming_result: result()
                    }))
                }
            },
        {provide: EditorService, useValue: {
            nativeAvailable: true, snapshot: current, busy, capabilities,
            playing: computed(() => current().mode === 'play'),
                workspaceGeneration: generation,
                switchScene,
                warmScene,
                setSceneWarmingMode,
                exportWarmingReport,
                checkScenes,
                save,
                createScene,
                authoringScope,
                registerScene,
                saveSceneAs,
                execute,
        }},
        {provide: SceneEditingService, useValue: {
            ready: computed(() => !busy() && current().mode === 'edit' && current().transaction === null),
                error: signal(null),
            withCommittedEdit,
        }},
    ]});
    const fixture = TestBed.createComponent(SceneActions);
    await fixture.whenStable();
    const root: HTMLElement = fixture.nativeElement;
    const button = (text: string) => [...root.querySelectorAll<HTMLButtonElement>('button')]
        .find(value => value.textContent?.trim() === text)!;
    const openDialog = (title: string) => [...root.querySelectorAll<HTMLDialogElement>('dialog[open]')]
        .find(dialog => dialog.querySelector('h2')?.textContent?.trim() === title);
    if (openInformation) {
        expect(fixture.componentInstance.show()).toBe(true);
        await fixture.whenStable();
        await vi.waitFor(() => expect(button('Prepare scene').disabled).toBe(false));
    }
    const expand = async () => {
        button('Scene Files…').click();
        await fixture.whenStable();
        return root.querySelector<HTMLInputElement>('.scene-files input')!;
    };
    const nameNewScene = async (name: string) => {
        if (!openDialog('New Scene')) {
            expect(fixture.componentInstance.showNewScene()).toBe(true);
            await fixture.whenStable();
        }
        const input = openDialog('New Scene')!.querySelector<HTMLInputElement>('input')!;
        input.value = name;
        input.dispatchEvent(new Event('input', {bubbles: true}));
        await fixture.whenStable();
        return input;
    };
    const sceneSwitch = TestBed.inject(SceneSwitchService);
    return {
        fixture,
        root,
        current,
        busy,
        generation,
        nativeGeneration,
        authoringGeneration,
        result,
        mode,
        capabilities,
        sceneSwitch,
        drafts: TestBed.inject(WorkspaceDraftReview),
        switchScene,
        warmScene,
        setSceneWarmingMode,
        exportWarmingReport,
        checkScenes,
        save,
        createScene,
        authoringScope,
        registerScene,
        saveSceneAs,
        execute,
        withCommittedEdit,
        button,
        openDialog,
        nameNewScene,
        expand
    };
}

const prototype = HTMLDialogElement.prototype;
const originalShow = Object.getOwnPropertyDescriptor(prototype, 'showModal');
const originalClose = Object.getOwnPropertyDescriptor(prototype, 'close');

beforeAll(() => {
    Object.defineProperty(prototype, 'showModal', {
        configurable: true,
        value(this: HTMLDialogElement) {
            this.setAttribute('open', '');
        },
    });
    Object.defineProperty(prototype, 'close', {
        configurable: true,
        value(this: HTMLDialogElement) {
            this.removeAttribute('open');
            this.dispatchEvent(new Event('close'));
        },
    });
});

afterAll(() => {
    if (originalShow) Object.defineProperty(prototype, 'showModal', originalShow);
    else Reflect.deleteProperty(prototype, 'showModal');
    if (originalClose) Object.defineProperty(prototype, 'close', originalClose);
    else Reflect.deleteProperty(prototype, 'close');
});

describe('SceneActions', () => {
    it('names, creates, and opens a new scene using the existing folder and scene switch flow', async () => {
        const panel = await createActions(snapshot(), [sampleModule], false);
        expect(panel.fixture.componentInstance.showNewScene()).toBe(true);
        await panel.fixture.whenStable();
        const dialog = panel.openDialog('New Scene')!;
        const input = dialog.querySelector<HTMLInputElement>('input')!;
        const fill = async (name: string) => {
            input.value = name;
            input.dispatchEvent(new Event('input', {bubbles: true}));
            await panel.fixture.whenStable();
        };
        expect(panel.button('Create Scene').disabled).toBe(true);
        await fill('Second');
        expect(panel.button('Create Scene').disabled).toBe(true);
        expect(dialog.querySelector('[role="alert"]')?.textContent).toContain('already uses this file name');
        await fill('Boss Room');
        expect(dialog.querySelector('#new-scene-path')?.textContent).toContain('scenes/boss-room.json');
        panel.button('Create Scene').click();
        await vi.waitFor(() => expect(panel.switchScene).toHaveBeenCalledWith('new-scene', 1, 2));
        expect(panel.createScene).toHaveBeenCalledExactlyOnceWith('scenes/boss-room.json', {
            workspaceGeneration: 1, nativeGeneration: 17, revision: 1,
        });
        await panel.fixture.whenStable();
        expect(panel.openDialog('New Scene')).toBeUndefined();
        expect(panel.current().active_scene_id).toBe('new-scene');
    });

    it('opens the new scene while a background change receipt occupies the editor queue', async () => {
        const panel = await createActions(snapshot(), [sampleModule], false);
        await panel.nameNewScene('Boss Room');
        const register = panel.createScene.getMockImplementation()!;
        panel.createScene.mockImplementationOnce(async (...args) => {
            const registered = await register(...args);
            panel.busy.set(true);
            return registered;
        });
        const open = panel.switchScene.getMockImplementation()!;
        panel.switchScene.mockImplementationOnce(async sceneId => {
            expect(panel.busy()).toBe(true);
            expect(panel.sceneSwitch.pending()).toBe(true);
            panel.busy.set(false);
            return open(sceneId);
        });

        panel.button('Create Scene').click();
        await panel.fixture.whenStable();
        expect(panel.withCommittedEdit).toHaveBeenCalledTimes(1);
        expect(panel.createScene).toHaveBeenCalledExactlyOnceWith('scenes/boss-room.json', {
            workspaceGeneration: 1, nativeGeneration: 17, revision: 1,
        });
        expect(panel.switchScene).toHaveBeenCalledExactlyOnceWith('new-scene', 1, 2);
        expect(panel.current().active_scene_id).toBe('new-scene');
    });

    it('confirms dirty changes before creating a file and restores the name on cancellation', async () => {
        const initial = snapshot();
        const panel = await createActions({
            ...initial, dirty: true,
            scenes: initial.scenes.map(scene => ({...scene, dirty: scene.id === 'main'})),
        }, [sampleModule], false);
        await panel.nameNewScene('Boss Room');
        panel.button('Create Scene').click();
        await panel.fixture.whenStable();
        const confirm = panel.openDialog('Save changes before opening scene?');
        expect(confirm?.textContent).toContain('scenes/boss-room.json');
        expect(panel.openDialog('New Scene')).toBeUndefined();
        expect(panel.createScene).not.toHaveBeenCalled();
        expect(panel.switchScene).not.toHaveBeenCalled();

        panel.button('Cancel').click();
        await panel.fixture.whenStable();
        expect(panel.openDialog('Save changes before opening scene?')).toBeUndefined();
        expect(panel.openDialog('New Scene')?.querySelector('input')?.value).toBe('Boss Room');
        expect(panel.createScene).not.toHaveBeenCalled();
        expect(panel.save).not.toHaveBeenCalled();

        panel.button('Create Scene').click();
        await panel.fixture.whenStable();
        panel.button('Keep Edits and Open').click();
        await panel.fixture.whenStable();
        expect(panel.createScene).toHaveBeenCalledExactlyOnceWith('scenes/boss-room.json', {
            workspaceGeneration: 1, nativeGeneration: 17, revision: 1,
        });
        expect(panel.switchScene).toHaveBeenCalledExactlyOnceWith('new-scene', 1, 2);
        expect(panel.save).not.toHaveBeenCalled();
        expect(panel.current().scenes[0].dirty).toBe(true);
    });

    it('creates and opens a scene while the dialog keeps the viewport unattached', async () => {
        const panel = await createActions(snapshot(), [sampleModule], false);
        panel.nativeGeneration.set(null);
        await panel.nameNewScene('Boss Room');
        panel.button('Create Scene').click();
        await panel.fixture.whenStable();

        expect(panel.nativeGeneration()).toBeNull();
        expect(panel.authoringScope).toHaveBeenCalledWith(1);
        expect(panel.createScene).toHaveBeenCalledExactlyOnceWith('scenes/boss-room.json', {
            workspaceGeneration: 1, nativeGeneration: 17, revision: 1,
        });
        expect(panel.current().active_scene_id).toBe('new-scene');
        expect(panel.openDialog('New Scene')).toBeUndefined();
    });

    it('keeps the typed name and reports a native scope error without writing', async () => {
        const panel = await createActions(snapshot(), [sampleModule], false);
        await panel.nameNewScene('Boss Room');
        panel.authoringScope.mockRejectedValueOnce(new Error('This editor host could not identify the project.'));
        panel.button('Create Scene').click();
        await panel.fixture.whenStable();
        const form = panel.openDialog('New Scene');
        expect(form?.querySelector('input')?.value).toBe('Boss Room');
        expect(form?.querySelector('[role="alert"]')?.textContent).toContain('could not identify the project');
        expect(panel.createScene).not.toHaveBeenCalled();
    });

    it('rejects a native scene revision that differs from the frontend before writing', async () => {
        const panel = await createActions(snapshot(), [sampleModule], false);
        await panel.nameNewScene('Boss Room');
        panel.authoringScope.mockResolvedValueOnce({workspaceGeneration: 1, nativeGeneration: 17, revision: 2});
        panel.button('Create Scene').click();
        await panel.fixture.whenStable();
        expect(panel.openDialog('New Scene')?.querySelector('[role="alert"]')?.textContent)
            .toContain('The project changed while preparing the new scene');
        expect(panel.createScene).not.toHaveBeenCalled();
    });

    it('cancels a pending new-scene write if the native workspace changes during confirmation', async () => {
        const initial = snapshot();
        const panel = await createActions({
            ...initial, dirty: true,
            scenes: initial.scenes.map(scene => ({...scene, dirty: scene.id === 'main'})),
        }, [sampleModule], false);
        await panel.nameNewScene('Boss Room');
        panel.button('Create Scene').click();
        await panel.fixture.whenStable();
        panel.authoringGeneration.set(18);
        panel.button('Save and Open').click();
        await panel.fixture.whenStable();
        expect(panel.openDialog('New Scene')?.querySelector('[role="alert"]')?.textContent)
            .toContain('The native project changed. Choose Create Scene again.');
        expect(panel.save).not.toHaveBeenCalled();
        expect(panel.createScene).not.toHaveBeenCalled();
        expect(panel.switchScene).not.toHaveBeenCalled();
    });

    it('reports how to recover an already registered scene after opening fails', async () => {
        const panel = await createActions(snapshot(), [sampleModule], false);
        await panel.nameNewScene('Boss Room');
        panel.switchScene.mockRejectedValueOnce(new Error('Could not open the new scene.'));
        panel.button('Create Scene').click();
        await panel.fixture.whenStable();

        expect(panel.current().active_scene_id).toBe('main');
        expect(panel.current().scenes.some(scene => scene.asset_path === 'scenes/boss-room.json')).toBe(true);
        expect(panel.openDialog('New Scene')).toBeUndefined();
        expect(panel.root.querySelector('[role="alert"]:not(dialog *)')?.textContent)
            .toContain('Could not open the new scene. Created scenes/boss-room.json. Open it from Assets');
        expect(panel.createScene).toHaveBeenCalledTimes(1);
        expect(panel.switchScene).toHaveBeenCalledTimes(1);
    });

    it('opens Scene Information on demand and closes it when the workspace changes', async () => {
        const {fixture, root, generation, openDialog} = await createActions(snapshot(), [sampleModule], false);
        expect(root.querySelector('dialog[open]')).toBeNull();
        expect(fixture.componentInstance.show()).toBe(true);
        await fixture.whenStable();
        const information = openDialog('Scene Information');
        expect(scenePicker(information!).value).toBe('scenes/main.json');

        generation.update(value => value + 1);
        await fixture.whenStable();
        expect(openDialog('Scene Information')).toBeUndefined();
    });

    it('warms the selected scene before an explicit open and cancels a pending selection', async () => {
        const {fixture, root, button, warmScene, switchScene, current} = await createActions();
        button('Prepare scene').click();
        await fixture.whenStable();
        const scene = scenePicker(root);
        pick(fixture, scene, 'second');
        await fixture.whenStable();
        expect(warmScene).toHaveBeenLastCalledWith('second', 1, 17, 1);
        expect(switchScene).not.toHaveBeenCalled();

        button('Prepare scene').click();
        await fixture.whenStable();
        expect(warmScene).toHaveBeenLastCalledWith(null, 1, 17, 1);
        expect(current().active_scene_id).toBe('main');

        button('Prepare scene').click();
        await fixture.whenStable();
        pick(fixture, scene, 'second');
        await fixture.whenStable();
        button('Open selected scene').click();
        await fixture.whenStable();
        expect(switchScene).toHaveBeenCalledWith('second', 1, 1);
        expect(current().active_scene_id).toBe('second');
    });

    it('opens the selected scene while native warming is still pending', async () => {
        const {fixture, root, button, warmScene, switchScene, current} = await createActions();
        let finishWarm!: () => void;
        warmScene.mockImplementationOnce(() => new Promise<void>(resolve => finishWarm = resolve));
        button('Prepare scene').click();
        await fixture.whenStable();
        pick(fixture, scenePicker(root), 'second');
        await vi.waitFor(() => expect(warmScene).toHaveBeenCalledExactlyOnceWith('second', 1, 17, 1));

        try {
            button('Open selected scene').click();
            await vi.waitFor(() => expect(switchScene).toHaveBeenCalledExactlyOnceWith('second', 1, 1));
            expect(current().active_scene_id).toBe('second');
        } finally {
            finishWarm();
        }
        await fixture.whenStable();
    });

    it('keeps ordinary switching available while the native viewport has no workspace token', async () => {
        const {fixture, root, button, warmScene, switchScene, nativeGeneration} = await createActions();
        nativeGeneration.set(null);
        await fixture.whenStable();

        expect(button('Prepare scene').disabled).toBe(true);
        pick(fixture, scenePicker(root), 'second');
        await fixture.whenStable();
        expect(switchScene).toHaveBeenCalledExactlyOnceWith('second', 1, 1);
        expect(warmScene).not.toHaveBeenCalled();
    });

    it('retires a pending warm selection when the native workspace changes at the same revision', async () => {
        const {fixture, root, button, warmScene, current, nativeGeneration} = await createActions();
        let rejectWarm!: (error: Error) => void;
        warmScene.mockImplementationOnce(() => new Promise<void>((_resolve, reject) => rejectWarm = reject));
        button('Prepare scene').click();
        await fixture.whenStable();
        const scene = scenePicker(root);
        pick(fixture, scene, 'second');
        await vi.waitFor(() => expect(warmScene).toHaveBeenCalledExactlyOnceWith('second', 1, 17, 1));

        nativeGeneration.set(18);
        await fixture.whenStable();
        expect(scene.value).toBe('scenes/main.json');
        expect(button('Open selected scene')).toBeUndefined();
        rejectWarm(new Error('Retired workspace'));
        await fixture.whenStable();
        expect(root.querySelector('[role="alert"]')).toBeNull();

        await vi.waitFor(() => expect(button('Prepare scene').disabled).toBe(false));
        button('Prepare scene').click();
        await fixture.whenStable();
        pick(fixture, scene, 'second');
        await vi.waitFor(() => expect(warmScene).toHaveBeenLastCalledWith('second', current().revision, 18, 1));
        expect(warmScene).toHaveBeenCalledTimes(2);
    });

    it('applies a mode change while the old warm call is pending and binds the next scene to the new mode', async () => {
        const {fixture, root, button, warmScene, mode, setSceneWarmingMode} = await createActions();
        let release!: () => void;
        warmScene.mockImplementationOnce(() => new Promise<void>(resolve => release = resolve));
        button('Prepare scene').click();
        await fixture.whenStable();
        pick(fixture, scenePicker(root), 'second');
        await vi.waitFor(() => expect(warmScene).toHaveBeenCalledExactlyOnceWith('second', 1, 17, 1));
        try {
            mode.set('models');
            await vi.waitFor(() => expect(setSceneWarmingMode).toHaveBeenLastCalledWith('models', 17));
            expect(button('Open selected scene')).toBeUndefined();
        } finally {
            release();
        }
        await fixture.whenStable();
        await vi.waitFor(() => expect(button('Prepare scene').disabled).toBe(false));
        button('Prepare scene').click();
        await fixture.whenStable();
        pick(fixture, scenePicker(root), 'second');
        await fixture.whenStable();
        expect(warmScene).toHaveBeenLastCalledWith('second', 1, 17, 2);
    });

    it('skips an intermediate mode while a setting is waiting for native acknowledgement', async () => {
        const {fixture, button, mode, setSceneWarmingMode} = await createActions();
        let release!: (revision: number) => void;
        setSceneWarmingMode.mockClear();
        setSceneWarmingMode.mockImplementationOnce(() => new Promise(resolve => release = resolve));
        mode.set('models');
        await vi.waitFor(() => expect(setSceneWarmingMode).toHaveBeenCalledExactlyOnceWith('models', 17));

        try {
            mode.set('off');
            fixture.detectChanges();
            mode.set('references');
            fixture.detectChanges();
            expect(button('Prepare scene').disabled).toBe(true);
        } finally {
            release(2);
        }

        await vi.waitFor(() => expect(button('Prepare scene').disabled).toBe(false));
        expect(setSceneWarmingMode.mock.calls).toEqual([['models', 17], ['references', 17]]);
    });

    it('retires a native warming error when the editor selects another mode', async () => {
        const {fixture, root, button, mode, setSceneWarmingMode} = await createActions();
        setSceneWarmingMode.mockRejectedValueOnce(new Error('Could not enable Models.'));
        mode.set('models');
        await vi.waitFor(() => expect(root.querySelector('[role="alert"]')?.textContent)
            .toContain('Could not enable Models.'));
        expect(button('Prepare scene').disabled).toBe(true);

        mode.set('references');
        await fixture.whenStable();
        await vi.waitFor(() => expect(button('Prepare scene').disabled).toBe(false));
        expect(root.querySelector('[role="alert"]')).toBeNull();
    });

    it('reports zero adoptions once and retires a result with its native workspace', async () => {
        const {fixture, root, current, result, mode, nativeGeneration, generation} = await createActions();
        mode.set('models');
        await fixture.whenStable();
        const summary: SceneWarmingResult = {
            mode: 'models', mode_revision: 2, scene_id: current().active_scene_id,
            scene_revision: current().scene_revision, request_id: 'load-a', references_hit: true,
            prepared_models: 3, adopted_models: 0, retained_bytes: 2048,
        };
        result.set(summary);
        await fixture.whenStable();
        const status = root.querySelector('.warming-result')!;
        const announcement = status.firstElementChild;
        expect(status.getAttribute('role')).toBe('status');
        expect(status.textContent).toContain('3 models prepared, 0 adopted');
        result.set({...summary});
        await fixture.whenStable();
        expect(status.firstElementChild).toBe(announcement);

        nativeGeneration.set(18);
        generation.set(2);
        result.set(null);
        await fixture.whenStable();
        expect(status.textContent?.trim()).toBe('');
    });

    it('announces the exported row count in Off mode and clears it after cancelling the picker', async () => {
        const {fixture, root, button, mode, exportWarmingReport} = await createActions();
        mode.set('off');
        await fixture.whenStable();
        const exportButton = button('Export Warming Report');
        const status = root.querySelector('[data-warming-report-status]')!;
        exportButton.click();
        await fixture.whenStable();
        expect(exportWarmingReport).toHaveBeenCalledExactlyOnceWith(17);
        expect(status.textContent?.trim()).toBe('Warming report exported with 3 rows.');
        exportWarmingReport.mockResolvedValueOnce(null);
        exportButton.click();
        await fixture.whenStable();
        expect(status.textContent?.trim()).toBe('');
    });

    it('ignores an export result from a replaced workspace', async () => {
        const {fixture, root, button, exportWarmingReport, generation, nativeGeneration} = await createActions();
        let complete!: (rows: number) => void;
        exportWarmingReport.mockImplementationOnce(() => new Promise(resolve => complete = resolve));
        const exportButton = button('Export Warming Report');
        exportButton.click();
        await fixture.whenStable();
        expect(exportButton.disabled).toBe(true);
        generation.set(2);
        nativeGeneration.set(18);
        complete(3);
        await fixture.whenStable();
        expect(root.querySelector('[data-warming-report-status]')?.textContent?.trim()).toBe('');
        expect(exportButton.disabled).toBe(false);
    });

    it('matches the end of a long scene path and opens it with the keyboard', async () => {
        const initial = snapshot();
        const longPath = 'scenes/river-district/scenes-for-the-northern-expedition/final-approach-observation-tower.json';
        const {fixture, root, current, switchScene} = await createActions({
            ...initial,
            scenes: [initial.scenes[0], {...initial.scenes[1], asset_path: longPath}],
            project: {
                ...initial.project, scenes: [
                    initial.project.scenes[0], {...initial.project.scenes[1], asset_path: longPath},
                ]
            },
        });
        const control = scenePicker(root);
        control.click();
        control.value = 'observation tower';
        control.dispatchEvent(new Event('input'));
        await fixture.whenStable();
        const matches = root.querySelectorAll('.scene-picker [role="option"]');
        expect(Array.from(matches, option => option.textContent?.trim())).toEqual([longPath]);
        expect(control.getAttribute('aria-activedescendant')).toBe(matches[0].id);
        control.dispatchEvent(new KeyboardEvent('keydown', {key: 'Escape', bubbles: true, cancelable: true}));
        await fixture.whenStable();
        expect(control.value).toBe('scenes/main.json');
        expect(switchScene).not.toHaveBeenCalled();

        control.click();
        control.value = 'observation tower';
        control.dispatchEvent(new Event('input'));
        await fixture.whenStable();
        control.dispatchEvent(new KeyboardEvent('keydown', {key: 'Enter', bubbles: true, cancelable: true}));
        await fixture.whenStable();
        expect(switchScene).toHaveBeenCalledExactlyOnceWith('second', 1, 1);
        expect(current().active_scene_id).toBe('second');
        expect(control.value).toBe(longPath);
    });

    it('keeps the authoritative selected scene while a switch is pending or fails', async () => {
        const {fixture, root, switchScene} = await createActions();
        let fail!: (error: Error) => void;
        switchScene.mockImplementationOnce(() => new Promise<EditorSnapshot>((_resolve, reject) => fail = reject));
        const control = scenePicker(root);
        pick(fixture, control, 'second');
        expect(control.value).toBe('scenes/main.json');
        fail(new Error('The second scene could not be loaded.'));
        await fixture.whenStable();
        expect(control.value).toBe('scenes/main.json');
        expect(control.disabled).toBe(false);
        expect(root.querySelector('[role="alert"]')?.textContent).toContain('second scene could not be loaded');
    });

    it('updates the chooser only after a successful scene switch', async () => {
        const initial = snapshot();
        const first = {id: 'first', asset_path: 'scenes/first.json', dirty: false, loaded: false};
        const {fixture, root, current, sceneSwitch, switchScene, withCommittedEdit} = await createActions({
            ...initial,
            scenes: [first, ...initial.scenes],
            project: {
                ...initial.project,
                scenes: [{id: first.id, asset_path: first.asset_path}, ...initial.project.scenes],
            },
        });
        const control = scenePicker(root);
        expect(control.value).toBe('scenes/main.json');
        expect(pickerOptions(fixture, control).map(option => option.textContent?.trim()))
            .toEqual(['scenes/first.json', 'scenes/main.json', 'scenes/second.json']);
        pick(fixture, control, 'second');
        await fixture.whenStable();
        expect(withCommittedEdit).toHaveBeenCalledOnce();
        expect(switchScene).toHaveBeenCalledExactlyOnceWith('second', 1, 1);
        expect(current().active_scene_id).toBe('second');
        expect(control.value).toBe('scenes/second.json');
        expect(root.querySelector('.status[role="status"]')?.textContent).toContain('Opened scenes/second.json.');
        expect(sceneSwitch.lastSwitch()).toEqual({
            sceneId: 'second', path: 'scenes/second.json', sceneRevision: 2, generation: 1,
        });
        expect(sceneSwitch.loading()).toBeNull();
    });

    it('reviews local drafts ahead of the scene reservation and uses the revision after an accepted save', async () => {
        const {current, drafts, sceneSwitch, switchScene, withCommittedEdit} = await createActions();
        let approve!: (accepted: boolean) => void;
        drafts.setReviewers([() => new Promise<boolean>(resolve => approve = resolve)]);
        const request = sceneSwitch.request('second');
        expect(withCommittedEdit).not.toHaveBeenCalled();
        expect(await sceneSwitch.request('second')).toBe(false);
        current.update(value => ({...value, revision: 4}));
        approve(true);
        expect(await request).toBe(true);
        expect(switchScene).toHaveBeenCalledExactlyOnceWith('second', 1, 4);
    });

    it('leaves the active scene and draft alone when local draft review is canceled', async () => {
        const {drafts, sceneSwitch, switchScene, withCommittedEdit} = await createActions();
        drafts.setReviewers([() => false]);
        expect(await sceneSwitch.request('second')).toBe(false);
        expect(withCommittedEdit).not.toHaveBeenCalled();
        expect(switchScene).not.toHaveBeenCalled();
    });

    it('saves only the outgoing scene from a switch confirmation while Scene Information is closed', async () => {
        const {fixture, root, current, sceneSwitch, save, switchScene, button, openDialog} =
            await createActions(snapshot(), [sampleModule], false);
        current.update(value => ({
            ...value, dirty: true, project_dirty: true,
            scenes: value.scenes.map(scene => ({...scene, dirty: true})),
        }));
        let complete!: (snapshot: EditorSnapshot) => void;
        switchScene.mockImplementationOnce(() => new Promise(resolve => complete = resolve));
        const request = sceneSwitch.request('second');
        await fixture.whenStable();
        expect(openDialog('Scene Information')).toBeUndefined();
        expect(openDialog('Save changes before opening scene?')?.textContent).toContain('scenes/main.json');
        expect(save).not.toHaveBeenCalled();
        expect(switchScene).not.toHaveBeenCalled();

        button('Save and Open').click();
        await vi.waitFor(() => expect(switchScene).toHaveBeenCalledExactlyOnceWith('second', 1, 2));
        expect(save).toHaveBeenCalledExactlyOnceWith(1, 1);
        expect(save.mock.invocationCallOrder[0]).toBeLessThan(switchScene.mock.invocationCallOrder[0]);
        expect(sceneSwitch.loading()).toEqual({sceneId: 'second', path: 'scenes/second.json'});
        expect(current().scenes.map(scene => scene.dirty)).toEqual([false, true]);
        expect(current().project_dirty).toBe(true);
        complete({...current(), active_scene_id: 'second', scene_revision: 2});
        expect(await request).toBe(true);
        await fixture.whenStable();
        expect(openDialog('Scene Information')).toBeUndefined();
        expect([...root.querySelectorAll('[role="status"]')].some(
            status => status.textContent?.includes('Opened scenes/second.json.')
        )).toBe(true);
    });

    it('cancels the pending switch or keeps the outgoing draft without writing it', async () => {
        const {fixture, current, sceneSwitch, save, switchScene, button, openDialog} = await createActions();
        current.update(value => ({
            ...value, dirty: true,
            scenes: value.scenes.map(scene => ({...scene, dirty: scene.id === 'main'})),
        }));
        const canceled = sceneSwitch.request('second');
        await fixture.whenStable();
        button('Cancel').click();
        expect(await canceled).toBe(false);
        await fixture.whenStable();
        expect(openDialog('Save changes before opening scene?')).toBeUndefined();
        expect(openDialog('Scene Information')).toBeDefined();
        expect(current().active_scene_id).toBe('main');
        expect(switchScene).not.toHaveBeenCalled();

        const kept = sceneSwitch.request('second');
        await fixture.whenStable();
        button('Keep Edits and Open').click();
        expect(await kept).toBe(true);
        expect(save).not.toHaveBeenCalled();
        expect(current().scenes.find(scene => scene.id === 'main')?.dirty).toBe(true);
    });

    it('leaves the active scene open when saving fails or the workspace changes during confirmation', async () => {
        const {fixture, current, generation, sceneSwitch, save, switchScene, button} = await createActions();
        current.update(value => ({
            ...value, dirty: true,
            scenes: value.scenes.map(scene => ({...scene, dirty: scene.id === 'main'})),
        }));
        save.mockRejectedValueOnce(new Error('The scene could not be saved.'));
        const failed = sceneSwitch.request('second');
        await fixture.whenStable();
        button('Save and Open').click();
        expect(await failed).toBe(false);
        expect(sceneSwitch.error()).toContain('could not be saved');
        expect(current().active_scene_id).toBe('main');
        expect(switchScene).not.toHaveBeenCalled();

        const stale = sceneSwitch.request('second');
        await fixture.whenStable();
        generation.update(value => value + 1);
        button('Save and Open').click();
        expect(await stale).toBe(false);
        expect(sceneSwitch.error()).toBeNull();
        expect(save).toHaveBeenCalledOnce();
        expect(switchScene).not.toHaveBeenCalled();
    });

    it('registers an existing scene and saves a scene under a validated relative path', async () => {
        const {fixture, registerScene, saveSceneAs, button, expand} = await createActions();
        const path = await expand();
        path.value = ' scenes/level.json ';
        path.dispatchEvent(new Event('input'));
        await fixture.whenStable();
        button('Register Existing Scene').click();
        await fixture.whenStable();
        expect(registerScene).toHaveBeenCalledExactlyOnceWith('scenes/level.json');
        expect(path.value).toBe('');
        path.value = 'scenes/renamed.json';
        path.dispatchEvent(new Event('input'));
        await fixture.whenStable();
        button('Save Scene As').click();
        await fixture.whenStable();
        expect(saveSceneAs).toHaveBeenCalledExactlyOnceWith('scenes/renamed.json');
        expect(path.value).toBe('');
    });

    it.each(['', '/scenes/main.json', '../scene.json', 'scenes/../scene.json', 'scenes//main.json', 'scenes/./main.json', 'D:\\main.json', 'scenes\\main.json', 'scenes/main.txt'])(
        'does not submit an invalid scene path: %s', async value => {
            const {fixture, registerScene, saveSceneAs, button, expand} = await createActions();
            const path = await expand();
            path.value = value;
            path.dispatchEvent(new Event('input'));
            await fixture.whenStable();
            expect(button('Register Existing Scene').disabled).toBe(true);
            expect(button('Save Scene As').disabled).toBe(true);
            button('Register Existing Scene').click();
            button('Save Scene As').click();
            expect(registerScene).not.toHaveBeenCalled();
            expect(saveSceneAs).not.toHaveBeenCalled();
        },
    );

    it('retains the path for retry when saving fails', async () => {
        const {fixture, root, saveSceneAs, button, expand} = await createActions();
        saveSceneAs.mockRejectedValueOnce(new Error('This file already exists.'));
        const path = await expand();
        path.value = 'scenes/existing.json';
        path.dispatchEvent(new Event('input'));
        await fixture.whenStable();
        button('Save Scene As').click();
        await fixture.whenStable();
        expect(path.value).toBe('scenes/existing.json');
        expect(root.querySelector('[role="alert"]')?.textContent).toContain('file already exists');
        expect(button('Save Scene As').disabled).toBe(false);
    });

    it('sets the currently active scene as startup through the command pipeline', async () => {
        const {fixture, current, execute, button} = await createActions();
        current.update(value => ({...value, active_scene_id: 'second'}));
        await fixture.whenStable();
        button('Use as Startup').click();
        await fixture.whenStable();
        expect(execute).toHaveBeenCalledExactlyOnceWith({type: 'set_startup_scene', scene_id: 'second'});
    });

    it('offers eligible scene role targets, assigns a long camera name, and clears the sky explicitly', async () => {
        const initial = snapshot();
        const camera: SceneEntity = {
            ...entity('camera-key'),
            name: 'Observation Camera Facing the Northern Expedition Landing and Cliffside Entrance',
            camera: {
                id: 'camera-component', enabled: true, settings: {
                    lens: {vertical_fov: 1, near: 0.1, far: 100}, rig: {mode: 'fixed'},
                }
            },
        };
        const sky: SceneEntity = {
            ...entity('sky-key'), name: 'Cliffside Sky',
            sky: {
                id: 'sky-component', enabled: true, settings: {
                    zenith: [0.1, 0.3, 0.5], horizon: [0.2, 0.4, 0.6], yaw_degrees: 0,
                }
            },
        };
        const {capabilities, current, execute, fixture, root} = await createActions({
            ...initial,
            document: {
                ...initial.document, environment: {active_sky: sky.key}, entities: [
                    camera, {...camera, key: 'off-camera', camera: {...camera.camera!, enabled: false}},
                    sky, {...sky, key: 'child-sky', parent: camera.key},
                ]
            },
        });
        capabilities.update(value => ({
            ...value, commands: [{name: 'set_default_game_camera'}, {name: 'set_active_sky'}],
        }));
        execute.mockImplementation(async command => {
            current.update(value => ({
                ...value, revision: value.revision + 1,
                document: {
                    ...value.document,
                    default_game_camera: command.type === 'set_default_game_camera'
                        ? command.key : value.document.default_game_camera,
                    environment: command.type === 'set_active_sky'
                        ? {active_sky: command.key} : value.document.environment,
                },
            }));
            return current();
        });
        await fixture.whenStable();

        const field = (name: string) => [...root.querySelectorAll('label')]
            .find(label => label.textContent?.trim() === name)?.control as HTMLInputElement;
        const cameraPicker = field('Game Camera');
        const skyPicker = field('Scene Sky');
        const cameraOptions = pickerOptions(fixture, cameraPicker);
        expect(cameraOptions.map(option => option.querySelector('span')?.textContent?.trim())).toEqual([camera.name]);
        expect(cameraPicker.closest('editor-search-picker')?.querySelector('button')).toBeNull();
        cameraOptions[0].click();
        await fixture.whenStable();
        expect(execute).toHaveBeenNthCalledWith(1, {type: 'set_default_game_camera', key: camera.key}, 1, 1);
        expect(cameraPicker.value).toBe(camera.name);
        expect(pickerOptions(fixture, skyPicker).map(option => option.querySelector('span')?.textContent?.trim())).toEqual([sky.name]);
        pick(fixture, skyPicker, '');
        await fixture.whenStable();
        expect(execute).toHaveBeenNthCalledWith(2, {type: 'set_active_sky', key: null}, 1, 2);
        expect(skyPicker.value).toBe('');
    });

    it('shows the saved role on open and switches to the new scene binding', async () => {
        const initial = snapshot();
        const {fixture, root, current, switchScene} = await createActions({
            ...initial,
            document: {...initial.document, entities: [entity('robot')], bindings: {player: 'robot'}},
        });
        const player = bindingPicker(root);
        expect(player.value).toBe('robot');

        switchScene.mockImplementationOnce(async () => {
            current.update(value => ({
                ...value,
                active_scene_id: 'second',
                document: {...value.document, entities: [entity('scout')], bindings: {player: 'scout'}},
            }));
            return current();
        });
        pick(fixture, scenePicker(root), 'second');
        await fixture.whenStable();
        expect(switchScene).toHaveBeenCalledExactlyOnceWith('second', 1, 1);
        const options = pickerOptions(fixture, player);
        expect(options.map(option => option.querySelector('span')?.textContent?.trim())).toEqual(['scout']);
        expect(player.value).toBe('scout');
    });

    it('uses catalog labels, hints and scene overrides for an alternate module', async () => {
        const module: GameplayModuleDescriptor = {
            id: 'race', label: 'Race course', description: 'Cross the checkpoint.',
            default_roles: [{name: 'checkpoint', label: 'Checkpoint flag', requirement_hint: 'Choose the lap marker.'}],
            scene_roles: {finish: []},
        };
        const initial = snapshot();
        const {fixture, root, current} = await createActions({
            ...initial,
            project: {...initial.project, gameplay_module: module.id},
        }, [sampleModule, module]);
        const selector = root.querySelector<HTMLSelectElement>('.gameplay-settings select')!;
        expect(selector.value).toBe('race');
        expect(root.querySelector('.gameplay-settings')?.textContent).toContain('Cross the checkpoint.');
        const role = bindingPicker(root);
        expect(role.closest('editor-search-picker')?.querySelector('label')?.textContent).toContain('Checkpoint flag');
        expect(root.querySelector('#' + role.getAttribute('aria-describedby'))?.textContent).toContain('Choose the lap marker.');
        expect(root.querySelector('.gameplay-roles')?.textContent).not.toContain('Player');

        current.update(value => ({...value, active_scene_id: 'finish'}));
        await fixture.whenStable();
        expect(root.querySelector('.gameplay-roles')).toBeNull();
        current.update(value => ({...value, active_scene_id: 'constructor'}));
        await fixture.whenStable();
        expect(root.querySelector('.gameplay-roles')?.textContent).toContain('Checkpoint flag');
        current.update(value => ({...value, active_scene_id: '__proto__'}));
        await fixture.whenStable();
        expect(root.querySelector('.gameplay-roles')?.textContent).toContain('Checkpoint flag');
    });

    it('shows the course and finish roles from their metadata', async () => {
        const {fixture, root, current} = await createActions();
        const labels = () => [...root.querySelectorAll('.gameplay-roles label')]
            .map(label => label.firstChild?.textContent?.trim());
        expect(labels()).toEqual(['Player', 'Key', 'Exit']);
        current.update(value => ({...value, active_scene_id: 'finish'}));
        await fixture.whenStable();
        expect(labels()).toEqual(['Player', 'Finish']);
    });

    it('treats a prototype-named role as unassigned and restores it after rejection', async () => {
        const initial = snapshot();
        const {fixture, root, execute} = await createActions({
            ...initial,
            document: {...initial.document, entities: [entity('actor')]},
        }, [{
            ...sampleModule,
            default_roles: [{name: 'constructor', label: 'Builder', requirement_hint: 'Choose the builder.'}],
        }]);
        const role = bindingPicker(root);
        expect(role.value).toBe('');
        execute.mockRejectedValueOnce(new Error('Role update rejected.'));
        pick(fixture, role, 'actor');
        expect(role.value).toBe('');
        await fixture.whenStable();
        expect(role.value).toBe('');
        expect(execute).toHaveBeenCalledExactlyOnceWith({
            type: 'set_scene_binding', scene_id: 'main', name: 'constructor', target: {kind: 'assign', key: 'actor'},
        }, 1);
    });

    it('keeps generic projects free of module roles and disables selection when metadata is missing', async () => {
        const initial = snapshot();
        const {fixture, root, capabilities, execute} = await createActions({
            ...initial, project: {...initial.project, gameplay_module: null},
        });
        const selector = root.querySelector<HTMLSelectElement>('.gameplay-settings select')!;
        expect(selector.value).toBe('');
        expect(root.querySelector('.gameplay-roles')).toBeNull();
        capabilities.set({gameplay_modules: undefined});
        await fixture.whenStable();
        expect(selector.disabled).toBe(true);
        expect(root.querySelector('#gameplay-module-help')?.textContent).toContain('unavailable in this editor host');
        choose(selector, sampleModule.id);
        await fixture.whenStable();
        expect(execute).not.toHaveBeenCalled();
    });

    it('preserves an unavailable selection and repairs it through an authoritative project update', async () => {
        const initial = snapshot();
        const authored = {...initial, project: {...initial.project, gameplay_module: 'missing_module'}};
        const {fixture, root, current, execute} = await createActions(authored);
        const selector = root.querySelector<HTMLSelectElement>('.gameplay-settings select')!;
        expect(selector.value).toBe('missing_module');
        expect(selector.selectedOptions[0].textContent).toContain('missing_module (unavailable)');
        expect(root.querySelector('#gameplay-module-help')?.textContent).toContain('repair');
        expect(root.querySelector('.gameplay-roles')).toBeNull();
        let accept!: () => void;
        execute.mockImplementationOnce(() => new Promise(resolve => {
            accept = () => {
                current.set({...authored, revision: 2, project: {...authored.project, gameplay_module: null}});
                resolve(current());
            };
        }));
        choose(selector, '');
        await vi.waitFor(() => expect(accept).toBeDefined());
        expect(selector.value).toBe('missing_module');
        expect(execute).toHaveBeenCalledExactlyOnceWith({
            type: 'set_project', project: {...authored.project, gameplay_module: null},
        }, 1, 1);
        accept();
        await fixture.whenStable();
        expect(selector.value).toBe('');
        current.set({...authored, revision: 3});
        await fixture.whenStable();
        expect(selector.value).toBe('missing_module');
    });

    it('builds the module update after the Inspector commit and retains the latest project and bindings', async () => {
        const base = snapshot();
        const initial: EditorSnapshot = {
            ...base,
            project: {
                ...base.project,
                behaviors: [{scene_id: 'main', behavior: {kind: 'controlled_move', entity: 'actor', speed: 2, space: 'world'}}],
            },
        };
        const {fixture, root, current, execute, withCommittedEdit} = await createActions({
            ...initial,
            document: {...initial.document, bindings: {other: 'actor'}},
        });
        withCommittedEdit.mockImplementationOnce(async action => {
            current.update(value => ({
                ...value, revision: 4,
                project: {...value.project, name: 'Updated during commit', fixed_hz: 30},
            }));
            await action();
            return true;
        });
        execute.mockImplementationOnce(async command => {
            if (command.type !== 'set_project') throw new Error('Expected a project update.');
            current.update(value => ({...value, revision: 5, project: command.project}));
            return current();
        });
        choose(root.querySelector<HTMLSelectElement>('.gameplay-settings select')!, '');
        await fixture.whenStable();
        expect(execute).toHaveBeenCalledExactlyOnceWith({
            type: 'set_project',
            project: {...initial.project, name: 'Updated during commit', fixed_hz: 30, gameplay_module: null},
        }, 1, 4);
        expect(current().document.bindings).toEqual({other: 'actor'});
        expect(current().project.behaviors).toEqual(initial.project.behaviors);
    });

    it('keeps the selected module on rejection and ignores an old workspace failure', async () => {
        const {fixture, root, generation, execute} = await createActions();
        const selector = root.querySelector<HTMLSelectElement>('.gameplay-settings select')!;
        execute.mockRejectedValueOnce(new Error('Project changed elsewhere.'));
        choose(selector, '');
        await fixture.whenStable();
        expect(selector.value).toBe(sampleModule.id);
        expect(root.querySelector('[role="alert"]')?.textContent).toContain('Project changed elsewhere.');

        let reject!: (reason: Error) => void;
        execute.mockImplementationOnce(() => new Promise((_resolve, fail) => reject = fail));
        choose(selector, '');
        await vi.waitFor(() => expect(reject).toBeDefined());
        generation.set(2);
        await fixture.whenStable();
        reject(new Error('Old workspace failure.'));
        await fixture.whenStable();
        expect(root.querySelector('[role="alert"]')).toBeNull();
    });

    it('drops a module choice if the workspace changes during the Inspector commit', async () => {
        const {fixture, root, generation, execute, withCommittedEdit} = await createActions();
        withCommittedEdit.mockImplementationOnce(async action => {
            generation.set(2);
            await action();
            return true;
        });
        choose(root.querySelector<HTMLSelectElement>('.gameplay-settings select')!, '');
        await fixture.whenStable();
        expect(execute).not.toHaveBeenCalled();
    });

    it('assigns, reassigns and clears a role only after the command is accepted', async () => {
        const initial = snapshot();
        const {fixture, root, current, execute} = await createActions({
            ...initial,
            document: {...initial.document, entities: [entity('robot'), entity('scout')]},
        });
        const player = bindingPicker(root);
        for (const [before, after] of [['', 'robot'], ['robot', 'scout'], ['scout', '']] as const) {
            const bindings: EditorSnapshot['document']['bindings'] = after ? {player: after} : {};
            let accept!: () => void;
            execute.mockImplementationOnce(() => new Promise<EditorSnapshot>(resolve => {
                accept = () => {
                    current.update(value => ({
                        ...value,
                        document: {...value.document, bindings},
                    }));
                    resolve(current());
                };
            }));

            pick(fixture, player, after);
            await vi.waitFor(() => expect(accept).toBeDefined());
            expect(execute).toHaveBeenLastCalledWith({
                type: 'set_scene_binding', scene_id: 'main', name: 'player',
                target: after ? {kind: 'assign', key: after} : {kind: 'clear'},
            }, 1);
            expect(player.value).toBe(before);
            accept();
            await fixture.whenStable();
            expect(player.value).toBe(after);
        }
        expect(execute).toHaveBeenCalledTimes(3);
    });

    it('shows a cleared role after the entity is deleted and restores it after Undo', async () => {
        const initial = snapshot();
        const authored = {
            ...initial,
            document: {...initial.document, entities: [entity('robot')], bindings: {player: 'robot'}},
        };
        const {fixture, root, current, execute} = await createActions(authored);
        const player = bindingPicker(root);

        current.set({...initial, revision: 2, can_undo: true});
        await fixture.whenStable();
        expect(pickerOptions(fixture, player)).toEqual([]);
        expect(player.value).toBe('');

        current.set({...authored, revision: 3, can_redo: true});
        await fixture.whenStable();
        expect(player.value).toBe('robot');
        expect(execute).not.toHaveBeenCalled();
    });

    it('waits for the Inspector and keeps the confirmed role when the update fails', async () => {
        const initial = snapshot();
        const {fixture, root, execute, withCommittedEdit} = await createActions({
            ...initial,
            document: {...initial.document, entities: [entity('robot'), entity('scout')], bindings: {player: 'robot'}},
        });
        const player = bindingPicker(root);
        let finishEdit!: () => void;
        withCommittedEdit.mockImplementationOnce(async action => {
            await new Promise<void>(resolve => finishEdit = resolve);
            await action();
            return true;
        });
        execute.mockRejectedValueOnce(new Error('Binding target disappeared.'));

        pick(fixture, player, 'scout');
        expect(player.value).toBe('robot');
        expect(execute).not.toHaveBeenCalled();
        finishEdit();
        await fixture.whenStable();
        expect(execute).toHaveBeenCalledExactlyOnceWith({
            type: 'set_scene_binding', scene_id: 'main', name: 'player',
            target: {kind: 'assign', key: 'scout'},
        }, 1);
        expect(player.value).toBe('robot');
        expect(root.querySelector('[role="alert"]')?.textContent).toContain('Binding target disappeared.');
    });

    it('disables scene changes, startup, and scene files during Play', async () => {
        const {fixture, root, current, execute, registerScene, saveSceneAs, switchScene, button, expand} = await createActions();
        const path = await expand();
        path.value = 'scenes/new.json';
        path.dispatchEvent(new Event('input'));
        current.update(value => ({...value, active_scene_id: 'second', mode: 'play'}));
        await fixture.whenStable();
        expect(scenePicker(root).disabled).toBe(true);
        expect(root.querySelector<HTMLSelectElement>('.gameplay-settings select')!.disabled).toBe(true);
        expect(root.querySelector('fieldset')!.disabled).toBe(true);
        expect(path.disabled).toBe(true);
        for (const action of ['Use as Startup', 'Register Existing Scene', 'Save Scene As']) {
            expect(button(action).disabled).toBe(true);
            button(action).click();
        }
        expect(execute).not.toHaveBeenCalled();
        expect(registerScene).not.toHaveBeenCalled();
        expect(saveSceneAs).not.toHaveBeenCalled();
        expect(switchScene).not.toHaveBeenCalled();
    });

    it('has accessible scene and file controls', async () => {
        const {root, expand} = await createActions();
        await expand();
        const report = await axe.run(root, {rules: {'color-contrast': {enabled: false}}});
        expect(report.violations.map(violation => violation.id)).toEqual([]);
    });
});

const reviewIssue: SceneIssue = {
    scope: 'intro', scene_id: 'main', message: 'Choose the player role.', role: 'player',
    suggestion: {entity_key: 'actor', entity_name: 'Actor'},
};
const manualIssue: SceneIssue = {
    scope: 'intro', scene_id: 'main', message: 'Choose the key role.', role: 'key', suggestion: null,
};
const arenaIssue: SceneIssue = {
    scope: 'arena', scene_id: 'second', message: 'Check the arena camera.', role: null, suggestion: null,
};
const projectIssue: SceneIssue = {
    scope: 'project', scene_id: null, message: 'Check project settings.', role: null, suggestion: null,
};
const pinnedPropIssue: SceneIssue = {
    scope: 'scene', scene_id: 'main', message: 'The editing source is missing. The saved Prop is complete.',
    role: null, suggestion: null,
    dependency: {
        code: 'missing_prop_source', severity: 'warning', owner: {key: 'bell', name: 'Bell Prop'},
        target: {path: 'props/bell.json'}, open: {kind: 'prop', key: 'gate'},
    },
};

function verification(revision: number, sceneId: string, issues: readonly SceneIssue[] = []): SceneVerificationReport {
    return {
        generation: 17, revision, active_scene_id: sceneId,
        scenes: [
            {id: 'main', content_stamp: '0000000000000000-1111111111111111', checked: true},
            {id: 'second', content_stamp: '0000000000000000-2222222222222222', checked: true},
        ],
        issues,
    };
}

async function createVerification(initial = snapshot(), modules = [sampleModule]) {
    const actions = await createActions(initial, modules);
    actions.capabilities.update(value => ({...value, scene_verification_supported: true}));
    await actions.fixture.whenStable();
    const section = actions.root.querySelector<HTMLElement>('editor-scene-verification')!;
    const check = async () => {
        actions.button('Check Scenes').click();
        await actions.fixture.whenStable();
    };
    return {...actions, section, check};
}

describe('Scene Information verification', () => {
    it('shows pinned repair warnings and opens only a current owning scene', async () => {
        const {button, check, checkScenes, execute, fixture, openDialog, section} = await createVerification();
        const repair = vi.fn();
        fixture.componentInstance.repairRequested.subscribe(repair);
        checkScenes.mockResolvedValueOnce(verification(1, 'main', [
            pinnedPropIssue, {
                ...pinnedPropIssue, scene_id: 'second', message: 'The Prop in the second scene is incomplete.',
                dependency: {
                    code: 'missing_prop_member', severity: 'error', owner: {key: 'member', name: 'Nested Member'},
                    open: {kind: 'prop', key: 'other-root'},
                },
            },
        ]));
        await check();
        expect(section.textContent).toContain('main · Passed with warnings');
        expect(section.textContent).toContain('second · Issues');
        expect(section.textContent).toContain('Bell Prop');
        expect(section.textContent).toContain('props/bell.json');
        const prop = button('Open in Prop');
        const actions = Array.from(section.querySelectorAll('button')).filter(item => item.textContent?.trim() === 'Open in Prop');
        expect(actions).toEqual([prop]);
        prop.click();
        await fixture.whenStable();
        expect(repair).toHaveBeenCalledExactlyOnceWith({
            action: {kind: 'prop', key: 'gate'}, owner: {key: 'bell', name: 'Bell Prop'}, scene: 'main',
            generation: 1, nativeGeneration: 17, revision: 1,
        });
        expect(execute).not.toHaveBeenCalled();
        expect(openDialog('Scene Information')).toBeUndefined();
    });

    it('rejects malformed dependency locations and actions from stale reports', async () => {
        const wrongPath = {
            ...pinnedPropIssue,
            dependency: {...pinnedPropIssue.dependency!, target: {path: '../outside.json'}},
        };
        expect(validSceneVerification(verification(1, 'main', [wrongPath]))).toBe(false);
        expect(validSceneVerification(verification(1, 'main', [pinnedPropIssue]))).toBe(true);
        const {button, check, checkScenes, current, fixture} = await createVerification();
        const repair = vi.fn();
        fixture.componentInstance.repairRequested.subscribe(repair);
        checkScenes.mockResolvedValueOnce(verification(1, 'main', [pinnedPropIssue]));
        await check();
        const pending = button('Open in Prop');
        current.update(frame => ({...frame, revision: frame.revision + 1}));
        pending.click();
        await fixture.whenStable();
        expect(repair).not.toHaveBeenCalled();
    });

    it('filters issues without changing the report summary or issuing native commands', async () => {
        const {button, check, checkScenes, execute, fixture, section, switchScene} = await createVerification();
        checkScenes.mockResolvedValue(verification(1, 'main', [reviewIssue, manualIssue, arenaIssue, projectIssue]));
        await check();

        expect(section.textContent?.replace(/\s+/g, ' ')).toContain('2 scenes with issues, 1 project issue.');
        expect(section.textContent).toContain('4 of 4 issues shown.');
        expect(section.querySelectorAll('button[data-verification-action="visit"]')).toHaveLength(1);
        const filter = section.querySelector<HTMLSelectElement>('select')!;
        filter.focus();
        choose(filter, 'arena');
        await fixture.whenStable();
        expect(section.textContent).toContain('Check the arena camera.');
        expect(section.textContent).not.toContain('Check project settings.');
        expect(section.textContent).toContain('1 of 4 issues shown.');

        const actionable = section.querySelector<HTMLInputElement>('input[type="checkbox"]')!;
        actionable.click();
        await fixture.whenStable();
        expect(section.textContent).toContain('0 of 4 issues shown.');
        choose(filter, 'intro');
        await fixture.whenStable();
        expect(section.textContent).toContain('2 of 4 issues shown.');
        expect(section.textContent?.replace(/\s+/g, ' ')).toContain('2 scenes with issues, 1 project issue.');
        expect(document.activeElement).toBe(filter);
        expect(checkScenes).toHaveBeenCalledExactlyOnceWith('main', 1, 17);
        expect(execute).not.toHaveBeenCalled();
        expect(switchScene).not.toHaveBeenCalled();
        expect(button('Review player assignment in main')).toBeDefined();
        const access = await axe.run(section, {rules: {'color-contrast': {enabled: false}}});
        expect(access.violations).toEqual([]);
    });

    it('reviews a role assignment, restores focus on cancel, and rechecks after a confirmed repair', async () => {
        const {button, check, checkScenes, current, execute, fixture, section} = await createVerification();
        checkScenes
            .mockResolvedValueOnce(verification(1, 'main', [reviewIssue]))
            .mockResolvedValueOnce(verification(2, 'main'));
        execute.mockImplementation(async () => {
            current.update(value => ({...value, revision: 2}));
            return current();
        });
        await check();
        const review = button('Review player assignment in main');
        review.click();
        await fixture.whenStable();
        const cancel = button('Cancel assignment');
        expect(document.activeElement).toBe(cancel);
        expect(section.textContent).toContain('Assign player to Actor (actor)?');
        expect(execute).not.toHaveBeenCalled();
        cancel.click();
        await fixture.whenStable();
        expect(document.activeElement).toBe(review);

        review.click();
        await fixture.whenStable();
        button('Apply assignment').click();
        await fixture.whenStable();
        expect(execute).toHaveBeenCalledExactlyOnceWith(
            {type: 'set_scene_binding', scene_id: 'main', name: 'player', target: {kind: 'assign', key: 'actor'}},
            1, 1,
        );
        expect(checkScenes).toHaveBeenLastCalledWith('main', 2, 17);
        expect(section.textContent).toContain('main · Passed');
        expect(section.textContent).toContain('0 of 0 issues shown.');
        expect(document.activeElement).toBe(section.querySelector('select'));

        current.update(value => ({...value, revision: 3}));
        await fixture.whenStable();
        expect(section.textContent).toContain('Needs check.');
        expect(section.textContent).not.toContain('main · Passed');
    });

    it('keeps remapped Cinder Oath role controls available when the snapshot is replaced', async () => {
        const initial = snapshot();
        const configured = {
            ...initial,
            project: {
                ...initial.project,
                gameplay_module: 'cinder_oath',
                gameplay_settings: {intro_scene: 'main', arena_scene: 'second'},
            },
        };
        const descriptor = {
            ...sampleModule, id: 'cinder_oath', default_roles: [],
            scene_roles: {intro: sampleModule.default_roles, arena: sampleModule.scene_roles['finish']},
        };
        const {button, check, checkScenes, current, fixture, root} = await createVerification(configured, [descriptor]);
        checkScenes.mockResolvedValueOnce(verification(1, 'main', [reviewIssue]));
        await check();
        button('Inspect player in main').click();
        await fixture.whenStable();
        const role = bindingPicker(root);
        expect(role).toBeDefined();
        expect(document.activeElement).toBe(role);

        current.update(value => ({...value, project: structuredClone(value.project), revision: 2}));
        await fixture.whenStable();
        expect(bindingPicker(root)).toBe(role);
    });

    it('checks a scene only after navigation accepts the dirty-scene confirmation', async () => {
        const initial = snapshot();
        const dirty = {...initial, scenes: [{...initial.scenes[0], dirty: true}, initial.scenes[1]]};
        const {button, check, checkScenes, fixture, openDialog, section, switchScene} = await createVerification(dirty);
        checkScenes
            .mockResolvedValueOnce(verification(1, 'main'))
            .mockResolvedValueOnce(verification(1, 'second'));
        await check();
        expect(section.textContent).toContain('main · Passed');
        expect(section.textContent).toContain('second · Needs visit');

        button('Visit and check second').click();
        await fixture.whenStable();
        expect(openDialog('Save changes before opening scene?')).toBeDefined();
        button('Cancel').click();
        await fixture.whenStable();
        expect(checkScenes).toHaveBeenCalledTimes(1);
        expect(switchScene).not.toHaveBeenCalled();
        expect(document.activeElement).toBe(button('Visit and check second'));

        button('Visit and check second').click();
        await fixture.whenStable();
        button('Keep Edits and Open').click();
        await fixture.whenStable();
        expect(checkScenes).toHaveBeenLastCalledWith('second', 1, 17);
        expect(section.textContent).toContain('2 of 2 scenes passed during a visit');
        expect(section.textContent).toContain('second · Passed');
    });

    it('ignores delayed results after a revision or workspace changes and drops actions for unregistered scenes', async () => {
        const {button, check, checkScenes, current, fixture, generation, section} = await createVerification();
        let resolve!: (value: SceneVerificationReport) => void;
        checkScenes.mockImplementationOnce(() => new Promise(accept => {
            resolve = accept;
        }));
        button('Check Scenes').click();
        current.update(value => ({...value, revision: 2}));
        resolve(verification(1, 'main', [reviewIssue]));
        await fixture.whenStable();
        expect(section.textContent).not.toContain('Review player assignment');

        checkScenes.mockResolvedValueOnce(verification(2, 'main', [reviewIssue]));
        await check();
        expect(section.textContent).toContain('Review player assignment');
        current.update(value => ({...value, scenes: [value.scenes[0]]}));
        await fixture.whenStable();
        expect(section.textContent).toContain('Needs check.');
        expect(section.textContent).not.toContain('Review player assignment');

        current.update(value => ({...snapshot(), revision: value.revision}));
        checkScenes.mockImplementationOnce(() => new Promise(accept => {
            resolve = accept;
        }));
        button('Check Scenes').click();
        generation.set(2);
        resolve(verification(2, 'main', [reviewIssue]));
        await fixture.whenStable();
        expect(fixture.componentInstance.show()).toBe(true);
        await fixture.whenStable();
        expect(section.textContent).not.toContain('Review player assignment');
        expect(section.textContent).not.toContain('Needs check.');
    });
});
