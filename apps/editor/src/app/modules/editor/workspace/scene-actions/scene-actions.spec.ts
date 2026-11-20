import {computed, signal} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import {vi} from 'vitest';
import axe from 'axe-core';
import type {EditorSnapshot, GameplayModuleDescriptor, SceneEntity} from '../../../../core/editor/editor.models';
import {EditorService} from '../../../../core/editor/editor.service';
import {LAYOUT_PREFERENCES_STORAGE} from '../../../../shared/services/layout-preferences-storage';
import {SceneEditingService} from '../scene-editing.service';
import {SceneActions} from './scene-actions';
import {SceneSwitchService} from './scene-switch.service';

const courtyard: GameplayModuleDescriptor = {
    id: 'courtyard_run', label: 'Courtyard Run', description: 'Collect the key and reach the finish.',
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
            gameplay_module: courtyard.id,
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

async function createActions(initial = snapshot(), modules: readonly GameplayModuleDescriptor[] | undefined = [courtyard]) {
    const current = signal(initial);
    const busy = signal(false);
    const generation = signal(1);
    const capabilities = signal<{gameplay_modules: readonly GameplayModuleDescriptor[] | undefined}>({gameplay_modules: modules});
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
    const execute = vi.fn<EditorService['execute']>(async () => current());
    const withCommittedEdit = vi.fn(async (action: () => Promise<void>) => {
        await action();
        return true;
    });
    TestBed.configureTestingModule({providers: [
            SceneSwitchService,
            {provide: LAYOUT_PREFERENCES_STORAGE, useValue: {load: async () => null, save: async () => undefined}},
        {provide: EditorService, useValue: {
            nativeAvailable: true, snapshot: current, busy, capabilities,
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
        capabilities,
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
        const select = root.querySelector<HTMLSelectElement>('.scene-actions select')!;
        expect([...select.options].map(option => option.value)).toEqual(['first', 'main', 'second']);
        expect(select.value).toBe('main');
        choose(select, 'second');
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

    it('shows the saved role on open and switches to the new scene binding', async () => {
        const initial = snapshot();
        const {fixture, root, current, switchScene} = await createActions({
            ...initial,
            document: {...initial.document, entities: [entity('robot')], bindings: {player: 'robot'}},
        });
        const player = root.querySelector<HTMLSelectElement>('fieldset select')!;
        expect(player.value).toBe('robot');

        switchScene.mockImplementationOnce(async () => {
            current.update(value => ({
                ...value,
                active_scene_id: 'second',
                document: {...value.document, entities: [entity('scout')], bindings: {player: 'scout'}},
            }));
            return current();
        });
        choose(root.querySelector<HTMLSelectElement>('.scene-actions select')!, 'second');
        await fixture.whenStable();
        expect(switchScene).toHaveBeenCalledExactlyOnceWith('second');
        expect([...player.options].map(option => option.value)).toEqual(['', 'scout']);
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
        }, [courtyard, module]);
        const selector = root.querySelector<HTMLSelectElement>('.gameplay-settings select')!;
        expect(selector.value).toBe('race');
        expect(root.querySelector('.gameplay-settings')?.textContent).toContain('Cross the checkpoint.');
        const role = root.querySelector<HTMLSelectElement>('.gameplay-roles select')!;
        expect(role.closest('label')?.textContent).toContain('Checkpoint flag');
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

    it('shows Finish authoring for a configured destination with a custom scene ID', async () => {
        const initial = snapshot();
        const {fixture, root, current} = await createActions({
            ...initial,
            active_scene_id: 'ending',
            project: {...initial.project, gameplay_settings: {
                schema_version: 1, scenes: {ending: {kind: 'finish', finish_trigger: 'goal-sensor'}},
            }},
        });
        const labels = () => [...root.querySelectorAll('.gameplay-roles label')]
            .map(label => label.firstChild?.textContent?.trim());
        expect(labels()).toEqual(['Player', 'Finish']);
        current.update(value => ({...value, active_scene_id: 'main'}));
        await fixture.whenStable();
        expect(labels()).toEqual(['Player', 'Key', 'Exit']);
    });

    it('treats a prototype-named role as unassigned and restores it after rejection', async () => {
        const initial = snapshot();
        const {fixture, root, execute} = await createActions({
            ...initial,
            document: {...initial.document, entities: [entity('actor')]},
        }, [{
            ...courtyard,
            default_roles: [{name: 'constructor', label: 'Builder', requirement_hint: 'Choose the builder.'}],
        }]);
        const role = root.querySelector<HTMLSelectElement>('.gameplay-roles select')!;
        expect(role.value).toBe('');
        execute.mockRejectedValueOnce(new Error('Role update rejected.'));
        choose(role, 'actor');
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
        choose(selector, courtyard.id);
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
        expect(selector.value).toBe(courtyard.id);
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
        const player = root.querySelector<HTMLSelectElement>('fieldset select')!;
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

            choose(player, after);
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
        const player = root.querySelector<HTMLSelectElement>('fieldset select')!;

        current.set({...initial, revision: 2, can_undo: true});
        await fixture.whenStable();
        expect([...player.options].map(option => option.value)).toEqual(['']);
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
        const player = root.querySelector<HTMLSelectElement>('fieldset select')!;
        let finishEdit!: () => void;
        withCommittedEdit.mockImplementationOnce(async action => {
            await new Promise<void>(resolve => finishEdit = resolve);
            await action();
            return true;
        });
        execute.mockRejectedValueOnce(new Error('Binding target disappeared.'));

        choose(player, 'scout');
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
        expect(root.querySelector('select')!.disabled).toBe(true);
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
