import {computed, signal} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import {vi} from 'vitest';
import axe from 'axe-core';
import type {EditorSnapshot} from '../../../../core/editor/editor.models';
import {EditorService} from '../../../../core/editor/editor.service';
import {SceneEditingService} from '../scene-editing.service';
import {SceneActions} from './scene-actions';

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
    const switchScene = vi.fn(async (sceneId: string) => {
        current.update(value => ({...value, active_scene_id: sceneId}));
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
        {provide: EditorService, useValue: {
            nativeAvailable: true, snapshot: current, busy,
            playing: computed(() => current().mode === 'play'),
            switchScene, registerScene, saveSceneAs, execute,
        }},
        {provide: SceneEditingService, useValue: {
            ready: computed(() => !busy() && current().mode === 'edit' && current().transaction === null),
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
    return {fixture, root, current, busy, switchScene, registerScene, saveSceneAs, execute, withCommittedEdit, button, expand};
}

describe('SceneActions', () => {
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
        const {fixture, root, current, switchScene, withCommittedEdit} = await createActions();
        const select = root.querySelector('select')!;
        select.value = 'second';
        select.dispatchEvent(new Event('change'));
        await fixture.whenStable();
        expect(withCommittedEdit).toHaveBeenCalledOnce();
        expect(switchScene).toHaveBeenCalledExactlyOnceWith('second');
        expect(current().active_scene_id).toBe('second');
        expect(select.value).toBe('second');
        expect(root.querySelector('[role="status"]')?.textContent).toContain('Unsaved edits in other scenes are retained');
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
