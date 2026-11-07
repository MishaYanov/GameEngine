import {computed, signal} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import {vi} from 'vitest';
import axe from 'axe-core';
import type {EditorSnapshot} from '../../../../core/editor/editor.models';
import {EditorService} from '../../../../core/editor/editor.service';
import {LAYOUT_PREFERENCES_STORAGE} from '../../../../shared/services/layout-preferences-storage';
import {SceneEditingService} from '../scene-editing.service';
import {SceneActions} from './scene-actions';
import {SceneSwitchService} from './scene-switch.service';

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

async function createActions() {
    const current = signal(snapshot());
    const busy = signal(false);
    const generation = signal(1);
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
    const registerScene = vi.fn(async (_path: string) => current());
    const saveSceneAs = vi.fn(async (_path: string) => current());
    const execute = vi.fn(async () => current());
    const withCommittedEdit = vi.fn(async (action: () => Promise<void>) => {
        await action();
        return true;
    });
    TestBed.configureTestingModule({providers: [
            SceneSwitchService,
            {provide: LAYOUT_PREFERENCES_STORAGE, useValue: {load: async () => null, save: async () => undefined}},
        {provide: EditorService, useValue: {
            nativeAvailable: true, snapshot: current, busy,
            playing: computed(() => current().mode === 'play'),
                workspaceGeneration: generation,
                switchScene, save, registerScene, saveSceneAs, execute,
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
    const expand = async () => {
        button('Scene Files…').click();
        await fixture.whenStable();
        return root.querySelector<HTMLInputElement>('.scene-files input')!;
    };
    const sceneSwitch = TestBed.inject(SceneSwitchService);
    return {
        fixture,
        root,
        current,
        busy,
        generation,
        sceneSwitch,
        switchScene,
        save,
        registerScene,
        saveSceneAs,
        execute,
        withCommittedEdit,
        button,
        expand
    };
}

describe('SceneActions', () => {
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

    it('keeps the authoritative selected scene while a switch is pending or fails', async () => {
        const {fixture, root, switchScene} = await createActions();
        let fail!: (error: Error) => void;
        switchScene.mockImplementationOnce(() => new Promise<EditorSnapshot>((_resolve, reject) => fail = reject));
        const select = root.querySelector('select')!;
        select.value = 'second';
        select.dispatchEvent(new Event('change'));
        expect(select.value).toBe('main');
        fail(new Error('The second scene could not be loaded.'));
        await fixture.whenStable();
        expect(select.value).toBe('main');
        expect(select.disabled).toBe(false);
        expect(root.querySelector('[role="alert"]')?.textContent).toContain('second scene could not be loaded');
    });

    it('updates the chooser only after a successful scene switch', async () => {
        const {fixture, root, current, sceneSwitch, switchScene, withCommittedEdit} = await createActions();
        const select = root.querySelector('select')!;
        select.value = 'second';
        select.dispatchEvent(new Event('change'));
        await fixture.whenStable();
        expect(withCommittedEdit).toHaveBeenCalledOnce();
        expect(switchScene).toHaveBeenCalledExactlyOnceWith('second');
        expect(current().active_scene_id).toBe('second');
        expect(select.value).toBe('second');
        expect(root.querySelector('[role="status"]')?.textContent).toContain('Opened scenes/second.json.');
        expect(sceneSwitch.lastSwitch()).toEqual({
            sceneId: 'second', path: 'scenes/second.json', sceneRevision: 2, generation: 1,
        });
        expect(sceneSwitch.loading()).toBeNull();
    });

    it('saves only the outgoing scene before switching when the user chooses Save and Open', async () => {
        const {fixture, root, current, sceneSwitch, save, switchScene, button} = await createActions();
        current.update(value => ({
            ...value, dirty: true, project_dirty: true,
            scenes: value.scenes.map(scene => ({...scene, dirty: true})),
        }));
        let complete!: (snapshot: EditorSnapshot) => void;
        switchScene.mockImplementationOnce(() => new Promise(resolve => complete = resolve));
        const request = sceneSwitch.request('second');
        await fixture.whenStable();
        expect(root.querySelector('dialog[open]')?.textContent).toContain('scenes/main.json');
        expect(save).not.toHaveBeenCalled();
        expect(switchScene).not.toHaveBeenCalled();

        button('Save and Open').click();
        await vi.waitFor(() => expect(switchScene).toHaveBeenCalledExactlyOnceWith('second'));
        expect(save).toHaveBeenCalledOnce();
        expect(save.mock.invocationCallOrder[0]).toBeLessThan(switchScene.mock.invocationCallOrder[0]);
        expect(sceneSwitch.loading()).toEqual({sceneId: 'second', path: 'scenes/second.json'});
        expect(current().scenes.map(scene => scene.dirty)).toEqual([false, true]);
        expect(current().project_dirty).toBe(true);
        complete({...current(), active_scene_id: 'second', scene_revision: 2});
        expect(await request).toBe(true);
    });

    it('cancels the pending switch or keeps the outgoing draft without writing it', async () => {
        const {fixture, root, current, sceneSwitch, save, switchScene, button} = await createActions();
        current.update(value => ({
            ...value, dirty: true,
            scenes: value.scenes.map(scene => ({...scene, dirty: scene.id === 'main'})),
        }));
        const canceled = sceneSwitch.request('second');
        await fixture.whenStable();
        button('Cancel').click();
        expect(await canceled).toBe(false);
        await fixture.whenStable();
        expect(root.querySelector('dialog[open]')).toBeNull();
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
        expect(sceneSwitch.error()).toContain('editor state changed');
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

    it('disables scene changes, startup, and scene files during Play', async () => {
        const {fixture, root, current, execute, registerScene, saveSceneAs, switchScene, button, expand} = await createActions();
        const path = await expand();
        path.value = 'scenes/new.json';
        path.dispatchEvent(new Event('input'));
        current.update(value => ({...value, active_scene_id: 'second', mode: 'play'}));
        await fixture.whenStable();
        expect(root.querySelector('select')!.disabled).toBe(true);
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
