import {signal} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import {invoke} from '@tauri-apps/api/core';
import {vi} from 'vitest';
import type {EditorCommand, EditorSnapshot, SceneEntity} from '../../../../core/editor/editor.models';
import {EditorService} from '../../../../core/editor/editor.service';
import {LAYOUT_PREFERENCES_STORAGE} from '../../../../shared/services/layout-preferences-storage';
import type {InspectorSubject} from '../inspector-panel/inspector-subject';
import {SceneEditingService} from '../scene-editing.service';
import {MaterialGraphDialog} from './material-graph-dialog';

vi.mock('@tauri-apps/api/core', () => ({invoke: vi.fn()}));

describe('Material Graph dialog', () => {
    const prototype = HTMLDialogElement.prototype;
    const originalShow = Object.getOwnPropertyDescriptor(prototype, 'showModal');
    const originalClose = Object.getOwnPropertyDescriptor(prototype, 'close');

    beforeAll(() => {
        Object.defineProperty(prototype, 'showModal', {
            configurable: true, value(this: HTMLDialogElement) { this.setAttribute('open', ''); },
        });
        Object.defineProperty(prototype, 'close', {
            configurable: true, value(this: HTMLDialogElement) { this.removeAttribute('open'); },
        });
    });

    beforeEach(() => vi.mocked(invoke).mockReset());

    afterAll(() => {
        if (originalShow) Object.defineProperty(prototype, 'showModal', originalShow);
        else Reflect.deleteProperty(prototype, 'showModal');
        if (originalClose) Object.defineProperty(prototype, 'close', originalClose);
        else Reflect.deleteProperty(prototype, 'close');
    });

    async function setup() {
        const snapshot = signal<Pick<EditorSnapshot, 'mode' | 'revision' | 'scene_revision' | 'active_scene_id' | 'selection'>>({
            mode: 'edit', revision: 8, scene_revision: 3, active_scene_id: 'level', selection: ['cube'],
        });
        const entity: SceneEntity = {
            key: 'cube', name: 'Cube', model_path: 'cube.glb', renderable: true,
            material: {base_color_factor: [0.25, 0.5, 0.75, 0.5]},
            transform: {position: [0, 0, 0], rotation: [0, 0, 0], scale: [1, 1, 1]},
        };
        const subject: InspectorSubject = {
            address: {kind: 'scene', scene: 'level', object: 'cube'},
            scope: {workspaceGeneration: 2, nativeGeneration: 1, revision: 8},
            contentRevision: 3, access: 'scene-edit',
        };
        const error = signal<string | null>(null);
        const execute = vi.fn(async (_command: EditorCommand) => undefined);
        TestBed.configureTestingModule({providers: [
            {provide: LAYOUT_PREFERENCES_STORAGE, useValue: {load: async () => ({}), save: vi.fn()}},
            {provide: EditorService, useValue: {snapshot, workspaceGeneration: () => 2, execute, nativeAvailable: true}},
            {provide: SceneEditingService, useValue: {error, withCommittedEdit: async (action: () => Promise<void>) => {
                try { await action(); return true; }
                catch (failure) { error.set(String(failure)); return false; }
            }}},
        ]});
        const fixture = TestBed.createComponent(MaterialGraphDialog);
        fixture.componentRef.setInput('entity', entity);
        fixture.componentRef.setInput('subject', subject);
        fixture.componentRef.setInput('editable', true);
        await fixture.whenStable();
        const root = fixture.nativeElement as HTMLElement;
        const click = async (name: string) => {
            const button = [...root.querySelectorAll('button')].find(item => item.textContent?.trim() === name);
            expect(button, name).toBeDefined();
            button!.click();
            await fixture.whenStable();
        };
        await click('Create Material Graph');
        return {click, root, execute, snapshot};
    }

    it('limits the node palette and applies native output without changing opacity', async () => {
        const {click, execute, root} = await setup();
        expect([...root.querySelectorAll<HTMLSelectElement>('select[aria-label="Node type"] option')]
            .map(option => option.textContent?.trim())).toEqual(['Color', 'Multiply', 'Material output', 'Value']);
        vi.mocked(invoke).mockImplementation(async (_command, args) => ({
            source_graph: args && 'graph' in args ? args['graph'] : undefined,
            base_color_factor: [0.25, 0.5, 0.75, 1],
        }));
        await click('Apply to Mesh');

        expect(invoke).toHaveBeenCalledExactlyOnceWith('editor_compile_material_graph', {
            graph: expect.objectContaining({domain: 'shader'}),
        });
        expect(execute).toHaveBeenCalledExactlyOnceWith({
            type: 'set_material', key: 'cube', material: {
                source_graph: expect.objectContaining({domain: 'shader'}),
                base_color_factor: [0.25, 0.5, 0.75, 0.5],
            },
        }, 2, 8);
        expect(root.querySelector('dialog[open]')).toBeNull();
    });

    it('leaves the graph open for native diagnostics and rejects a stale apply', async () => {
        const {click, execute, root, snapshot} = await setup();
        vi.mocked(invoke).mockRejectedValueOnce({code: 'missing_input', node: 'material', port: 'color', message: 'Connect a color.'});
        await click('Apply to Mesh');
        expect(root.querySelector('[role=alert]')?.textContent).toContain('material · color · Connect a color.');
        expect(root.querySelector('dialog[open]')).not.toBeNull();
        vi.mocked(invoke).mockResolvedValueOnce({base_color_factor: [0.5, 0.5, 0.5, 1]});
        snapshot.update(value => ({...value, revision: 9, scene_revision: 4}));
        await click('Apply to Mesh');
        expect(root.querySelector('[role=alert]')?.textContent).toContain('The scene changed.');
        expect(execute).not.toHaveBeenCalled();
    });
});
