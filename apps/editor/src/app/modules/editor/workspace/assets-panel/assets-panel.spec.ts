import {computed, signal} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import {By} from '@angular/platform-browser';
import {invoke} from '@tauri-apps/api/core';
import {vi} from 'vitest';
import type {EditorSnapshot, ModelAsset} from '../../../../core/editor/editor.models';
import type {AudioClipDetails} from '../../../../core/editor/audio-clip.models';
import {EditorService} from '../../../../core/editor/editor.service';
import {LAYOUT_PREFERENCES_STORAGE} from '../../../../shared/services/layout-preferences-storage';
import {SceneSwitchService} from '../scene-actions/scene-switch.service';
import {SceneEditingService} from '../scene-editing.service';
import {ViewportAssetDragService} from '../viewport-asset-drag.service';
import {AssetsPanel} from './assets-panel';
import {PropAssetSelection} from '../prefabs/prop-asset-selection';
import {EditorContextMenuTrigger} from '../../../../shared/components/context-menu/context-menu-trigger';
import type {ContextMenuItem} from '../../../../shared/builder/context-menu-builder';

vi.mock('@tauri-apps/api/core', () => ({invoke: vi.fn(), isTauri: vi.fn(() => true)}));
vi.mock('@tauri-apps/api/event', () => ({listen: vi.fn()}));

const robot: ModelAsset = {name: 'Robot', path: 'props/robot.glb'};
const bell: ModelAsset = {name: 'Bell', path: 'bell.wav', kind: 'audio_clip', asset_id: 'bell-id'};
const bellDetails: AudioClipDetails = {
    sample_rate_hz: 16000, frames: 24000, channels: 1,
    references: [{asset_id: 'hardware', path: 'props/hardware.json'}],
};

function snapshot(): EditorSnapshot {
    return {
        active_scene_id: 'main', revision: 1, scene_revision: 1,
        project_dirty: false, dirty: false, can_undo: false, can_redo: false,
        mode: 'edit', paused: false, selection: [], transaction: null,
        scenes: [
            {id: 'main', asset_path: 'scenes/main.json', dirty: false, loaded: true},
            {id: 'courtyard', asset_path: 'scenes/courtyard.json', dirty: false, loaded: false},
        ],
        project: {
            schema_version: 3, name: 'Asset fixture', startup_scene_id: 'main',
            scenes: [{id: 'main', asset_path: 'scenes/main.json'}, {id: 'courtyard', asset_path: 'scenes/courtyard.json'}],
            fixed_hz: 60, max_catchup_steps: 8, behaviors: [], behavior_definitions: [],
        },
        document: {
            schema_version: 3, bindings: {}, entities: [], lights: [],
            camera: {position: [0, 0, 3], yaw: 0, pitch: 0, vertical_fov: 1, near: 0.1, far: 100},
        },
        snap: {enabled: false, translation: 1, rotation_radians: 0.1, scale: 0.1},
    };
}

type EditingStub = Pick<SceneEditingService, 'ready' | 'error' | 'withCommittedEdit'>;

async function createPanel(options: {models?: readonly ModelAsset[]; editing?: EditingStub; nativeAvailable?: boolean} = {}) {
    const current = signal(snapshot());
    const modelAssets = signal<readonly ModelAsset[] | null>(options.models ?? null);
    const workspaceGeneration = signal(1);
    const busy = signal(false);
    const audioClipDetails = vi.fn(async (_id: string, _generation: number, _revision: number, _nativeGeneration: number | null) => bellDetails);
    const assets = vi.fn(async () => {
        modelAssets.set([robot]);
        return modelAssets()!;
    });
    const request = vi.fn(async (sceneId: string) => {
        current.update(value => ({...value, active_scene_id: sceneId}));
        return true;
    });
    const drag = {canBegin: () => true, candidate: vi.fn(), candidateScene: vi.fn()};
    const importModel = vi.fn(async (_path: string, _position: readonly [number, number, number], _generation?: number, _revision?: number) => {
        current.update(value => ({...value, dirty: true}));
        return current();
    });
    const loadPrefab = vi.fn(async (_path: string, _position: readonly [number, number, number], _generation?: number, _revision?: number) => current());
    TestBed.configureTestingModule({providers: [
        PropAssetSelection,
        {provide: LAYOUT_PREFERENCES_STORAGE, useValue: {load: async () => null, save: async () => undefined}},
        {provide: EditorService, useValue: {
            nativeAvailable: options.nativeAvailable ?? true,
            capabilities: () => ({}),
            snapshot: current, busy, playing: computed(() => current().mode === 'play'),
                modelAssets, assets, importModel, loadPrefab, workspaceGeneration, audioClipDetails,
        }},
            {provide: SceneSwitchService, useValue: {request, canSwitch: () => true}},
            {provide: ViewportAssetDragService, useValue: drag},
        ...(options.editing ? [{provide: SceneEditingService, useValue: options.editing}] : []),
    ]});
    const fixture = TestBed.createComponent(AssetsPanel);
    const root = fixture.nativeElement as HTMLElement;
    const button = (label: string): HTMLButtonElement => {
        const candidate = [...root.querySelectorAll('button')].find(element => element.textContent.trim() === label);
        if (!candidate) throw new Error(`Missing asset button: ${label}`);
        return candidate;
    };
    const file = (path: string): HTMLButtonElement => {
        const candidate = root.querySelector<HTMLButtonElement>(`.asset-file[title="${path}"] button`);
        if (!candidate) throw new Error(`Missing asset file: ${path}`);
        return candidate;
    };
    const contextAction = (path: string, id: string): Extract<ContextMenuItem, { kind: 'action' }> => {
        const trigger = fixture.debugElement.queryAll(By.directive(EditorContextMenuTrigger))
            .find(candidate => candidate.nativeElement === file(path))!.injector.get(EditorContextMenuTrigger);
        const source = trigger.editorContextMenu();
        const items = typeof source === 'function' ? source() : source;
        const action = items.find(item => item.kind === 'action' && item.id === id);
        if (!action || action.kind !== 'action') throw new Error(`Missing asset context action: ${id}`);
        return action;
    };
    await fixture.whenStable();
    return {
        fixture,
        root,
        button,
        file,
        contextAction,
        current,
        modelAssets,
        workspaceGeneration,
        audioClipDetails,
        busy,
        assets,
        request,
        importModel,
        loadPrefab,
        drag
    };
}

describe('AssetsPanel', () => {
    it('filters Audio Clips and shows native duration, format and reverse references with focus on the row', async () => {
        const panel = await createPanel({models: [robot, bell]});
        const filter = panel.root.querySelector<HTMLSelectElement>('.asset-filter select')!;
        filter.value = 'audio_clip';
        filter.dispatchEvent(new Event('change'));
        await panel.fixture.whenStable();
        expect(panel.root.textContent).not.toContain('props');

        const row = panel.file(bell.path);
        row.focus();
        row.click();
        await panel.fixture.whenStable();
        expect(panel.audioClipDetails).toHaveBeenCalledExactlyOnceWith('bell-id', 1, 1, null);
        expect(row.classList.contains('drag-source')).toBe(false);
        expect(document.activeElement).toBe(row);
        const details = panel.root.querySelector('[aria-label="Audio Clip details"]');
        expect(details?.textContent).toContain('1.5 s');
        expect(details?.textContent).toContain('16-bit mono PCM WAV');
        expect(details?.textContent).toContain('props/hardware.json');
        expect(panel.root.textContent).not.toContain('Add to Scene');
    });

    it('offers a retry when native Audio Clip metadata fails', async () => {
        const panel = await createPanel({models: [bell]});
        panel.audioClipDetails.mockRejectedValueOnce(new Error('The WAV no longer matches its asset fingerprint.'));
        panel.file(bell.path).click();
        await panel.fixture.whenStable();
        expect(panel.root.querySelector('editor-audio-clip-details [role="alert"]')?.textContent).toContain('asset fingerprint');
        panel.button('Retry Audio Clip details').click();
        await panel.fixture.whenStable();
        expect(panel.audioClipDetails).toHaveBeenCalledTimes(2);
        expect(panel.root.querySelector('[aria-label="Audio Clip details"]')).not.toBeNull();
    });

    it('reveals a linked Prop by stable asset ID and focuses its current catalogue row', async () => {
        const source: ModelAsset = {
            kind: 'prefab', asset_id: 'gate-source', name: 'Gate', path: 'props/moved/gate.json',
        };
        const duplicate = {...source, asset_id: 'different-source', path: 'prefabs/gate.json'};
        const panel = await createPanel({models: [duplicate, source]});
        const reveal = TestBed.inject(PropAssetSelection);

        reveal.reveal({sourceId: source.asset_id!, workspaceGeneration: 1, sceneId: 'main'});
        await panel.fixture.whenStable();
        expect(panel.root.textContent).toContain('Assets / props/moved');
        const selected = panel.file(source.path);
        expect(selected.getAttribute('aria-pressed')).toBe('true');
        expect(panel.root.ownerDocument.activeElement).toBe(selected);
        expect(panel.assets).not.toHaveBeenCalled();
        expect(panel.current().dirty).toBe(false);

        reveal.reveal({sourceId: duplicate.asset_id, workspaceGeneration: 0, sceneId: 'main'});
        await panel.fixture.whenStable();
        expect(selected.getAttribute('aria-pressed')).toBe('true');
        expect(reveal.request()).toBeNull();
    });

    it('reports a missing linked Prop without selecting an asset at its former path', async () => {
        const panel = await createPanel({models: [{...robot, path: 'prefabs/gate.json', kind: 'prefab'}]});
        TestBed.inject(PropAssetSelection).reveal({sourceId: 'removed', workspaceGeneration: 1, sceneId: 'main'});
        await panel.fixture.whenStable();
        expect(panel.assets).toHaveBeenCalledOnce();
        expect(panel.root.querySelector('[role=alert]')?.textContent).toContain('linked Prop source is unavailable');
        expect(panel.root.querySelector('.asset-details')).toBeNull();
        expect(panel.loadPrefab).not.toHaveBeenCalled();
    });

    it('adds the clicked asset through the field commit flow independently of the selected file', async () => {
        const crate: ModelAsset = {name: 'Crate', path: 'props/crate.glb'};
        const withCommittedEdit = vi.fn(async (action: () => Promise<void>) => {
            await action();
            return true;
        });
        const panel = await createPanel({
            models: [robot, crate], editing: {
                ready: signal(true), error: signal(null), withCommittedEdit,
            }
        });
        panel.button('props').click();
        await panel.fixture.whenStable();
        panel.file(robot.path).click();
        await panel.fixture.whenStable();
        await panel.contextAction(crate.path, 'asset.add-model').execute();
        await panel.fixture.whenStable();
        expect(panel.importModel).toHaveBeenCalledExactlyOnceWith(crate.path, [0, 0, 0], 1, 1);
        expect(withCommittedEdit).toHaveBeenCalledOnce();
        expect(panel.file(robot.path).getAttribute('aria-pressed')).toBe('true');
    });

    it('offers scene and prefab actions for their clicked files without selecting them first', async () => {
        const prefab: ModelAsset = {name: 'Actor', path: 'prefabs/actor.json', kind: 'prefab'};
        const panel = await createPanel({models: [robot, prefab]});
        panel.button('prefabs').click();
        await panel.fixture.whenStable();
        await panel.contextAction(prefab.path, 'asset.instantiate-prefab').execute();
        expect(panel.loadPrefab).toHaveBeenCalledExactlyOnceWith(prefab.path, [0, 0, 0], 1, 1);
        panel.button('Up').click();
        await panel.fixture.whenStable();
        panel.button('scenes').click();
        await panel.fixture.whenStable();
        expect(panel.contextAction('scenes/main.json', 'asset.open-scene').disabled).toBe(true);
        await panel.contextAction('scenes/courtyard.json', 'asset.open-scene').execute();
        expect(panel.request).toHaveBeenCalledExactlyOnceWith('courtyard');
    });

    it('blocks an open asset action after authoring becomes unavailable, scene changes, or workspace changes', async () => {
        const panel = await createPanel({models: [robot]});
        panel.button('props').click();
        await panel.fixture.whenStable();
        const add = panel.contextAction(robot.path, 'asset.add-model');
        panel.current.update(value => ({...value, mode: 'play'}));
        await panel.fixture.whenStable();
        await add.execute();
        panel.current.update(value => ({...value, mode: 'edit', active_scene_id: 'courtyard'}));
        await panel.fixture.whenStable();
        await add.execute();
        panel.current.update(value => ({...value, active_scene_id: 'main'}));
        panel.workspaceGeneration.set(2);
        await panel.fixture.whenStable();
        await add.execute();
        expect(panel.importModel).not.toHaveBeenCalled();
    });

    it('resolves moved asset identities and rejects deleted assets before context actions execute', async () => {
        const original: ModelAsset = {...robot, asset_id: 'robot-id'};
        const panel = await createPanel({models: [original]});
        panel.button('props').click();
        await panel.fixture.whenStable();
        const add = panel.contextAction(original.path, 'asset.add-model');
        panel.modelAssets.set([{...original, path: 'props/moved.glb'}]);
        await panel.fixture.whenStable();
        await add.execute();
        expect(panel.importModel).toHaveBeenCalledExactlyOnceWith('props/moved.glb', [0, 0, 0], 1, 1);
        panel.modelAssets.set([]);
        await panel.fixture.whenStable();
        await add.execute();
        expect(panel.importModel).toHaveBeenCalledOnce();
    });

    it('retains selection through real service publication and the following catalog refresh', async () => {
        const original: ModelAsset = {...robot, asset_id: 'robot-id', kind: 'model'};
        const moved: ModelAsset = {...original, path: 'props/moved.glb'};
        let resolve!: (assets: readonly ModelAsset[]) => void;
        const refreshed = new Promise<readonly ModelAsset[]>(done => resolve = done);
        vi.mocked(invoke).mockReset()
            .mockResolvedValueOnce(snapshot())
            .mockResolvedValueOnce([original])
            .mockResolvedValueOnce({operation_id: 'move', asset: moved, snapshot: {...snapshot(), revision: 2}})
            .mockReturnValueOnce(refreshed);
        TestBed.configureTestingModule({providers: [EditorService,
            {provide: LAYOUT_PREFERENCES_STORAGE, useValue: {load: async () => null, save: async () => undefined}},
            {provide: SceneSwitchService, useValue: {request: vi.fn(), canSwitch: () => true}},
        ]});
        const editor = TestBed.inject(EditorService);
        await editor.openProject('project.json');
        await editor.assets();
        const fixture = TestBed.createComponent(AssetsPanel);
        await fixture.whenStable();
        const root: HTMLElement = fixture.nativeElement;
        [...root.querySelectorAll('button')].find(button => button.textContent.trim() === 'props')!.click();
        await fixture.whenStable();
        const file = (path: string) => root.querySelector<HTMLButtonElement>(`.asset-file[title="${path}"] button`);
        file(original.path)!.click();
        await fixture.whenStable();
        await editor.moveAsset('robot-id', moved.path, editor.workspaceGeneration(), 1);
        await fixture.whenStable();
        expect(file(original.path)?.getAttribute('aria-pressed')).toBe('true');
        const refresh = editor.assets();
        resolve([moved]);
        await refresh;
        await fixture.whenStable();
        expect(file(moved.path)?.getAttribute('aria-pressed')).toBe('true');
        expect(file(original.path)).toBeNull();
    });

    it('keeps catalogue selection by identity after an asset move and instantiates catalogued prefabs', async () => {
        const prefab: ModelAsset = {name: 'Gameplay actor', path: 'prefabs/actor.json', kind: 'prefab', asset_id: 'prefab-id'};
        const panel = await createPanel({models: [{...robot, asset_id: 'robot-id'}, prefab]});
        panel.button('props').click();
        await panel.fixture.whenStable();
        panel.file(robot.path).click();
        panel.modelAssets.set([{...robot, path: 'props/moved.glb', asset_id: 'robot-id'}, prefab]);
        await panel.fixture.whenStable();
        expect(panel.file('props/moved.glb').getAttribute('aria-pressed')).toBe('true');
        panel.button('Up').click();
        await panel.fixture.whenStable();
        panel.button('prefabs').click();
        await panel.fixture.whenStable();
        panel.file(prefab.path).click();
        await panel.fixture.whenStable();
        panel.button('Place as Copy').click();
        await panel.fixture.whenStable();
        expect(panel.loadPrefab).toHaveBeenCalledExactlyOnceWith('prefabs/actor.json', [0, 0, 0], 1, 1);
        expect(panel.importModel).not.toHaveBeenCalled();
    });
    it('discovers props automatically and keeps the current folder and selected file when refreshed', async () => {
        const panel = await createPanel();
        expect(panel.assets).toHaveBeenCalledExactlyOnceWith(false);
        panel.button('scenes').click();
        await panel.fixture.whenStable();
        expect(panel.root.textContent).toContain('Assets / scenes');

        panel.button('Up').click();
        await panel.fixture.whenStable();
        panel.button('props').click();
        await panel.fixture.whenStable();
        panel.file(robot.path).click();
        await panel.fixture.whenStable();
        panel.button('Refresh Assets').click();
        await panel.fixture.whenStable();
        expect(panel.assets).toHaveBeenCalledTimes(2);
        expect(panel.assets).toHaveBeenLastCalledWith(true);
        expect(panel.root.textContent).toContain('Assets / props');
        expect(panel.file(robot.path).getAttribute('aria-pressed')).toBe('true');
        expect(panel.root.querySelector('.asset-details')?.textContent).toContain(robot.path);
    });

    it('opens the selected scene through the shared review flow', async () => {
        const panel = await createPanel();
        panel.current.update(value => ({...value, dirty: true}));
        panel.button('scenes').click();
        await panel.fixture.whenStable();
        expect(panel.file('scenes/main.json').textContent).toContain('Active');
        panel.file('scenes/courtyard.json').click();
        await panel.fixture.whenStable();
        panel.button('Open Scene').click();
        await panel.fixture.whenStable();

        expect(panel.request).toHaveBeenCalledExactlyOnceWith('courtyard');
        expect(panel.current().dirty).toBe(true);
        expect(panel.importModel).not.toHaveBeenCalled();
        expect(panel.file('scenes/courtyard.json').textContent).toContain('Active');
        expect(panel.button('Open Scene').disabled).toBe(true);
        expect(panel.root.querySelector('[role="status"]')).toBeNull();
    });

    it('hands models and scene files to their drag entry points while excluding the active scene', async () => {
        const panel = await createPanel();
        panel.button('props').click();
        await panel.fixture.whenStable();
        const propPointer = new MouseEvent('pointerdown', {button: 0});
        panel.file(robot.path).dispatchEvent(propPointer);
        expect(panel.drag.candidate).toHaveBeenCalledExactlyOnceWith(robot.path, propPointer);

        panel.button('Up').click();
        await panel.fixture.whenStable();
        panel.button('scenes').click();
        await panel.fixture.whenStable();
        const scenePointer = new MouseEvent('pointerdown', {button: 0});
        panel.file('scenes/main.json').dispatchEvent(scenePointer);
        expect(panel.drag.candidateScene).not.toHaveBeenCalled();
        panel.file('scenes/courtyard.json').dispatchEvent(scenePointer);
        expect(panel.drag.candidateScene).toHaveBeenCalledExactlyOnceWith('courtyard', scenePointer, expect.any(Function));
        expect(panel.request).not.toHaveBeenCalled();

        const open = panel.drag.candidateScene.mock.calls[0][2] as () => Promise<boolean>;
        expect(await open()).toBe(true);
        expect(panel.request).toHaveBeenCalledExactlyOnceWith('courtyard');
    });

    it('adds a catalogued model to the active scene at the origin', async () => {
        const panel = await createPanel({models: [robot]});
        panel.button('props').click();
        await panel.fixture.whenStable();
        panel.file(robot.path).click();
        await panel.fixture.whenStable();
        panel.button('Add to Scene').click();
        await panel.fixture.whenStable();

        expect(panel.importModel).toHaveBeenCalledExactlyOnceWith('props/robot.glb', [0, 0, 0], 1, 1);
        expect(panel.assets).not.toHaveBeenCalled();
        expect(panel.current().dirty).toBe(true);
        expect(panel.root.querySelector('[role="status"]')?.textContent).toContain('Added Robot');
    });

    it('shows an import error and permits retrying the same selected model', async () => {
        const panel = await createPanel({models: [robot]});
        panel.importModel.mockRejectedValueOnce(new Error('The model could not be loaded.'));
        panel.button('props').click();
        await panel.fixture.whenStable();
        panel.file(robot.path).click();
        await panel.fixture.whenStable();
        panel.button('Add to Scene').click();
        await panel.fixture.whenStable();
        expect(panel.root.querySelector('[role="alert"]')?.textContent).toContain('The model could not be loaded.');
        expect(panel.button('Add to Scene').disabled).toBe(false);

        panel.button('Add to Scene').click();
        await panel.fixture.whenStable();
        expect(panel.importModel).toHaveBeenCalledTimes(2);
        expect(panel.root.querySelector('[role="alert"]')).toBeNull();
        expect(panel.root.querySelector('[role="status"]')?.textContent).toContain('Added Robot');
    });

    it('blocks mutations during Play, opening, and unrelated backend work', async () => {
        const panel = await createPanel({models: [robot]});
        panel.button('props').click();
        await panel.fixture.whenStable();
        panel.file(robot.path).click();
        panel.current.update(value => ({...value, mode: 'play'}));
        await panel.fixture.whenStable();
        expect(panel.button('Add to Scene').disabled).toBe(true);
        expect(panel.button('Refresh Assets').disabled).toBe(true);
        panel.button('Add to Scene').click();
        expect(panel.importModel).not.toHaveBeenCalled();

        panel.current.update(value => ({...value, mode: 'edit'}));
        panel.fixture.componentRef.setInput('opening', true);
        await panel.fixture.whenStable();
        expect(panel.button('Add to Scene').disabled).toBe(true);
        panel.fixture.componentRef.setInput('opening', false);
        panel.busy.set(true);
        await panel.fixture.whenStable();
        expect(panel.button('Add to Scene').disabled).toBe(true);
        panel.busy.set(false);
        await panel.fixture.whenStable();
        expect(panel.button('Add to Scene').disabled).toBe(false);
    });

    it('settles an owned field edit before adding a model and prevents duplicate actions', async () => {
        let release!: () => void;
        const settled = new Promise<void>(resolve => release = resolve);
        const withCommittedEdit = vi.fn(async (action: () => Promise<void>) => {
            await settled;
            await action();
            return true;
        });
        const panel = await createPanel({models: [robot], editing: {
            ready: signal(true), error: signal(null), withCommittedEdit,
        }});
        panel.busy.set(true);
        panel.button('props').click();
        await panel.fixture.whenStable();
        panel.file(robot.path).click();
        await panel.fixture.whenStable();
        expect(panel.button('Add to Scene').disabled).toBe(false);
        panel.button('Add to Scene').click();
        await panel.fixture.whenStable();
        expect(withCommittedEdit).toHaveBeenCalledOnce();
        expect(panel.importModel).not.toHaveBeenCalled();
        expect(panel.button('Add to Scene').disabled).toBe(true);
        panel.button('Add to Scene').click();
        release();
        await withCommittedEdit.mock.results[0].value;
        await panel.fixture.whenStable();

        expect(panel.importModel).toHaveBeenCalledExactlyOnceWith('props/robot.glb', [0, 0, 0], 1, 1);
        expect(withCommittedEdit).toHaveBeenCalledOnce();
    });

    it('surfaces a failed field commit instead of starting the asset action', async () => {
        const panel = await createPanel({models: [robot], editing: {
            ready: signal(true), error: signal('Could not commit the transform.'),
            withCommittedEdit: vi.fn(async () => false),
        }});
        panel.button('props').click();
        await panel.fixture.whenStable();
        panel.file(robot.path).click();
        await panel.fixture.whenStable();
        panel.button('Add to Scene').click();
        await panel.fixture.whenStable();

        expect(panel.importModel).not.toHaveBeenCalled();
        expect(panel.root.querySelector('[role="alert"]')?.textContent).toContain('Could not commit the transform.');
        expect(panel.button('Add to Scene').disabled).toBe(false);
    });
});
