import {TestBed} from '@angular/core/testing';
import {vi} from 'vitest';
import type {EditorCommand, SceneEntity} from '../../../../core/editor/editor.models';
import {EditorService} from '../../../../core/editor/editor.service';
import type {LinkedPrefabOverride, LinkedPrefabStatus} from '../../../../core/editor/linked-prefabs.models';
import {LAYOUT_PREFERENCES_STORAGE} from '../../../../shared/services/layout-preferences-storage';
import type {InspectorSubject} from '../inspector-panel/inspector-subject';
import {SceneEditingService} from '../scene-editing.service';
import {LinkedPrefabInspector} from './linked-prefab-inspector';

describe('Linked Prop leaf removal', () => {
    it('removes a direct inherited leaf and restores it from the instance override row', async () => {
        const member: SceneEntity = {
            key: 'instance-leaf', name: 'Leaf', model_path: 'leaf.glb', renderable: true, parent: 'instance',
            transform: {position: [0, 0, 0], rotation: [0, 0, 0], scale: [1, 1, 1]},
        };
        const anchor = {...member, key: 'instance', name: 'Prop', parent: null};
        const removal: LinkedPrefabOverride = {
            address: {kind: 'object', placement_path: [], source_entity: 'leaf'},
            target: {source_entity: 'leaf', property: {kind: 'presence'}},
            base: {kind: 'presence', value: true}, value: {kind: 'presence', value: false},
            can_apply_to_root_source: false,
        };
        const subject: InspectorSubject = {
            address: {kind: 'linked', scene: 'level', root: 'instance', address: removal.address},
            scope: {workspaceGeneration: 2, nativeGeneration: 1, revision: 8},
            access: 'linked-overrides', contentRevision: 3,
        };
        let status: LinkedPrefabStatus = {
            root_key: 'instance', source_id: 'prop', source_path: 'leaf.prefab.json',
            source_fingerprint: 'pinned', source_changed: false, source_error: null, bindings: {}, overrides: [],
        };
        const execute = vi.fn(async (command: EditorCommand) => {
            if (command.type === 'remove_linked_prefab_leaf') status = {...status, overrides: [removal]};
            else if (command.type === 'revert_linked_prefab') status = {...status, overrides: []};
        });
        TestBed.configureTestingModule({providers: [
            {provide: LAYOUT_PREFERENCES_STORAGE, useValue: {load: async () => ({}), save: vi.fn()}},
            {provide: EditorService, useValue: {
                workspaceGeneration: () => 2, modelAssets: () => [], execute,
                capabilities: () => ({commands: [{name: 'remove_linked_prefab_leaf'}]}),
                snapshot: () => ({revision: 8, active_scene_id: 'level', mode: 'edit', transaction: null,
                    document: {entities: [anchor, member]}}),
                linkedPrefabStatus: async () => status,
            }},
            {provide: SceneEditingService, useValue: {
                withCommittedEdit: async (action: () => Promise<void>) => { await action(); return true; },
            }},
        ]});
        const fixture = TestBed.createComponent(LinkedPrefabInspector);
        fixture.componentRef.setInput('entity', member);
        fixture.componentRef.setInput('subject', subject);
        fixture.componentRef.setInput('editable', true);
        await fixture.whenStable();
        const root = fixture.nativeElement as HTMLElement;
        const click = async (label: string) => {
            const element = [...root.querySelectorAll('button')].find(button => button.textContent?.trim() === label);
            expect(element, label).toBeDefined();
            element!.click();
            await fixture.whenStable();
        };
        await click('Remove Object from This Instance');
        expect(execute).toHaveBeenNthCalledWith(1, {
            type: 'remove_linked_prefab_leaf', root_key: 'instance', object_key: 'instance-leaf',
        }, 2, 8);
        fixture.componentRef.setInput('entity', anchor);
        fixture.componentRef.setInput('subject', null);
        await fixture.whenStable();
        expect(root.textContent).toContain('Removed from this instance');
        await click('Restore Object');
        expect(execute).toHaveBeenNthCalledWith(2, {
            type: 'revert_linked_prefab', root_key: 'instance', property: removal.target,
        }, 2, 8);
        expect(root.textContent).not.toContain('Removed from this instance');
    });
});
