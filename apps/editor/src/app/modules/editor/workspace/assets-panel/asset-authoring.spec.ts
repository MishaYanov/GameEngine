import {computed, signal} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import axe from 'axe-core';
import {vi} from 'vitest';
import type {EditorSnapshot, ModelAsset} from '../../../../core/editor/editor.models';
import {EditorService} from '../../../../core/editor/editor.service';
import {LAYOUT_PREFERENCES_STORAGE} from '../../../../shared/services/layout-preferences-storage';
import {SceneEditingService} from '../scene-editing.service';
import {AssetAuthoring} from './asset-authoring';

const model: ModelAsset = {name: 'Actor', path: 'models/actor.glb', kind: 'model',
    asset_id: 'actor-id', fingerprint: 'original', source_path: 'C:/Models/actor.glb'};

describe('AssetAuthoring', () => {
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
        const current = signal<Pick<EditorSnapshot, 'revision' | 'active_scene_id' | 'mode' | 'selection' | 'transaction'>>({
            revision: 7, active_scene_id: 'main', mode: 'edit', selection: ['actor'], transaction: null,
        });
        const generation = signal(1);
        const chooseExternalGlb = vi.fn(async (): Promise<string | null> => 'C:/Models/new.glb');
        const assets = vi.fn(async () => [model]);
        const operation = () => ({operation_id: 'operation', asset: model, snapshot: current()});
        const importExternalGlb = vi.fn(async (_source: string, _path: string, _generation?: number, _revision?: number) => operation());
        const reimportAsset = vi.fn(async (_id: string, _fingerprint: string, _source?: string | null, _generation?: number, _revision?: number) => operation());
        const moveAsset = vi.fn(async (_id: string, _path: string, _generation?: number, _revision?: number) => operation());
        const savePrefab = vi.fn(async (_path: string, _generation?: number, _revision?: number) => ({path: 'gameplay.json'}));
        const committed = vi.fn(async (action: () => Promise<void>) => { await action(); return true; });
        TestBed.configureTestingModule({providers: [
            {provide: LAYOUT_PREFERENCES_STORAGE, useValue: {load: async () => null, save: async () => undefined}},
            {provide: EditorService, useValue: {
                nativeAvailable: true, snapshot: current, workspaceGeneration: generation, busy: () => false,
                playing: computed(() => current().mode === 'play'),
                chooseExternalGlb, importExternalGlb, reimportAsset, moveAsset, savePrefab, assets,
            }},
            {provide: SceneEditingService, useValue: {ready: () => true, error: () => null, withCommittedEdit: committed}},
        ]});
        const fixture = TestBed.createComponent(AssetAuthoring);
        fixture.componentRef.setInput('selected', model);
        await fixture.whenStable();
        const root: HTMLElement = fixture.nativeElement;
        const button = (text: string) => [...root.querySelectorAll('button')].find(button => button.textContent.trim() === text)!;
        const path = async (value: string) => {
            const field = root.querySelector<HTMLInputElement>('input')!;
            field.value = value; field.dispatchEvent(new Event('input'));
            await fixture.whenStable();
        };
        return {fixture, root, button, path, current, generation, chooseExternalGlb, importExternalGlb, reimportAsset, moveAsset, savePrefab, assets, committed};
    }

    it('imports an external GLB after committing edits with a captured workspace and revision', async () => {
        const panel = await setup();
        panel.button('Import GLB…').click();
        await panel.fixture.whenStable();
        expect(panel.root.querySelector<HTMLInputElement>('input')?.value).toBe('new.glb');
        await panel.path('props/new.glb');
        panel.committed.mockImplementationOnce(async action => {
            panel.current.update(value => ({...value, revision: 12}));
            await action(); return true;
        });
        panel.button('Apply Asset Operation').click();
        await panel.fixture.whenStable();
        expect(panel.importExternalGlb).toHaveBeenCalledExactlyOnceWith('C:/Models/new.glb', 'props/new.glb', 1, 12);
        expect(panel.assets).toHaveBeenCalledExactlyOnceWith(true);
        expect(panel.root.textContent).toContain('External GLB imported.');
    });

    it('rejects unsafe destinations and preserves reimport errors for retry with the captured identity', async () => {
        const panel = await setup();
        panel.button('Import GLB…').click();
        await panel.fixture.whenStable();
        await panel.path('../outside.glb');
        expect(panel.button('Apply Asset Operation').disabled).toBe(true);
        panel.root.querySelector<HTMLDialogElement>('dialog')!.close();
        await panel.fixture.whenStable();
        panel.reimportAsset.mockRejectedValueOnce(new Error('The asset fingerprint changed.'));
        panel.button('Reimport GLB…').click();
        await panel.fixture.whenStable();
        expect(panel.reimportAsset).toHaveBeenCalledExactlyOnceWith('actor-id', 'original', 'C:/Models/new.glb', 1, 7);
        expect(panel.root.querySelector('[role="alert"]')?.textContent).toContain('fingerprint changed');
        expect(panel.assets).not.toHaveBeenCalled();
    });

    it('drops a source picker response after workspace replacement', async () => {
        const panel = await setup();
        let resolve!: (path: string) => void;
        panel.chooseExternalGlb.mockImplementationOnce(() => new Promise(done => resolve = done));
        panel.button('Import GLB…').click();
        panel.generation.set(2);
        resolve('C:/Models/late.glb');
        await panel.fixture.whenStable();
        expect(panel.root.querySelector('dialog[open]')).toBeNull();
        expect(panel.importExternalGlb).not.toHaveBeenCalled();
        expect(panel.root.querySelector('[role="alert"]')).toBeNull();
    });

    it('moves by asset ID and refuses to save a changed gameplay selection', async () => {
        const panel = await setup();
        panel.button('Move Asset…').click();
        await panel.fixture.whenStable();
        await panel.path('models/moved.glb');
        panel.button('Apply Asset Operation').click();
        await panel.fixture.whenStable();
        expect(panel.moveAsset).toHaveBeenCalledExactlyOnceWith('actor-id', 'models/moved.glb', 1, 7);
        panel.button('Save Gameplay Prefab…').click();
        await panel.fixture.whenStable();
        panel.committed.mockImplementationOnce(async action => {
            panel.current.update(value => ({...value, selection: ['other']}));
            await action(); return true;
        });
        panel.button('Apply Asset Operation').click();
        await panel.fixture.whenStable();
        expect(panel.savePrefab).not.toHaveBeenCalled();
        expect(panel.root.querySelector('[role="alert"]')?.textContent).toContain('selection changed');
    });

    it('matches native move and reimport support for legacy models and gameplay prefabs', async () => {
        const panel = await setup();
        panel.fixture.componentRef.setInput('selected', {...model, path: 'models/legacy.gltf'});
        await panel.fixture.whenStable();
        expect(panel.button('Reimport GLB…').disabled).toBe(true);
        expect(panel.button('Move Asset…').disabled).toBe(true);
        expect(panel.root.textContent).toContain('Import a GLB copy');

        panel.fixture.componentRef.setInput('selected', {...model, kind: 'prefab', path: 'prefabs/assembly.json'});
        await panel.fixture.whenStable();
        expect(panel.button('Reimport GLB…').disabled).toBe(true);
        expect(panel.button('Move Asset…').disabled).toBe(false);
        panel.button('Move Asset…').click();
        await panel.fixture.whenStable();
        await panel.path('prefabs/moved.json');
        panel.button('Apply Asset Operation').click();
        await panel.fixture.whenStable();
        expect(panel.moveAsset).toHaveBeenCalledExactlyOnceWith('actor-id', 'prefabs/moved.json', 1, 7);
    });

    it('saves a gameplay prefab through the existing guarded endpoint and has accessible controls', async () => {
        const panel = await setup();
        panel.button('Save Gameplay Prefab…').click();
        await panel.fixture.whenStable();
        const report = await axe.run(panel.root, {rules: {'color-contrast': {enabled: false}}});
        expect(report.violations.map(violation => violation.id)).toEqual([]);
        panel.button('Apply Asset Operation').click();
        await panel.fixture.whenStable();
        expect(panel.savePrefab).toHaveBeenCalledExactlyOnceWith('gameplay.json', 1, 7);
        panel.current.update(value => ({...value, mode: 'play'}));
        await panel.fixture.whenStable();
        expect(panel.button('Import GLB…').disabled).toBe(true);
    });
});
