import {computed, signal} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import {vi} from 'vitest';
import type {EditorSnapshot, ModelAsset} from '../../../../core/editor/editor.models';
import {EditorService} from '../../../../core/editor/editor.service';
import {SceneEditingService} from '../scene-editing.service';
import {AssetsPanel} from './assets-panel';

const robot: ModelAsset = {name: 'Robot', path: 'props/robot.glb'};

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
    const busy = signal(false);
    const assets = vi.fn(async () => {
        modelAssets.set([robot]);
        return modelAssets()!;
    });
    const switchScene = vi.fn(async (sceneId: string) => {
        current.update(value => ({...value, active_scene_id: sceneId}));
        return current();
    });
    const importModel = vi.fn(async (_path: string, _position: readonly [number, number, number]) => {
        current.update(value => ({...value, dirty: true}));
        return current();
    });
    TestBed.configureTestingModule({providers: [
        {provide: EditorService, useValue: {
            nativeAvailable: options.nativeAvailable ?? true,
            snapshot: current, busy, playing: computed(() => current().mode === 'play'),
            modelAssets, assets, switchScene, importModel,
        }},
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
    await fixture.whenStable();
    return {fixture, root, button, file, current, modelAssets, busy, assets, switchScene, importModel};
}

describe('AssetsPanel', () => {
    it('loads models explicitly and keeps the current folder and selected file when refreshed', async () => {
        const panel = await createPanel();
        expect(panel.assets).not.toHaveBeenCalled();
        panel.button('scenes').click();
        await panel.fixture.whenStable();
        panel.button('Load Models').click();
        await panel.fixture.whenStable();
        expect(panel.assets).toHaveBeenCalledExactlyOnceWith(true);
        expect(panel.root.textContent).toContain('Assets / scenes');

        panel.button('Up').click();
        await panel.fixture.whenStable();
        panel.button('props').click();
        await panel.fixture.whenStable();
        panel.file(robot.path).click();
        await panel.fixture.whenStable();
        panel.button('Refresh Models').click();
        await panel.fixture.whenStable();
        expect(panel.root.textContent).toContain('Assets / props');
        expect(panel.file(robot.path).getAttribute('aria-pressed')).toBe('true');
        expect(panel.root.querySelector('.asset-details')?.textContent).toContain(robot.path);
    });

    it('opens the selected scene through the backend without saving or discarding dirty work', async () => {
        const panel = await createPanel();
        panel.current.update(value => ({...value, dirty: true}));
        panel.button('scenes').click();
        await panel.fixture.whenStable();
        expect(panel.file('scenes/main.json').textContent).toContain('Active');
        panel.file('scenes/courtyard.json').click();
        await panel.fixture.whenStable();
        panel.button('Open Scene').click();
        await panel.fixture.whenStable();

        expect(panel.switchScene).toHaveBeenCalledExactlyOnceWith('courtyard');
        expect(panel.current().dirty).toBe(true);
        expect(panel.importModel).not.toHaveBeenCalled();
        expect(panel.file('scenes/courtyard.json').textContent).toContain('Active');
        expect(panel.button('Open Scene').disabled).toBe(true);
        expect(panel.root.querySelector('[role="status"]')?.textContent).toContain('Opened courtyard.json');
    });

    it('adds a catalogued model to the active scene at the origin', async () => {
        const panel = await createPanel({models: [robot]});
        panel.button('props').click();
        await panel.fixture.whenStable();
        panel.file(robot.path).click();
        await panel.fixture.whenStable();
        panel.button('Add to Scene').click();
        await panel.fixture.whenStable();

        expect(panel.importModel).toHaveBeenCalledExactlyOnceWith('props/robot.glb', [0, 0, 0]);
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
        expect(panel.button('Refresh Models').disabled).toBe(true);
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

        expect(panel.importModel).toHaveBeenCalledExactlyOnceWith('props/robot.glb', [0, 0, 0]);
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
