import {signal} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import {vi} from 'vitest';
import type {ComponentRecord} from '../../../../core/editor/editor-components';
import {EditorService} from '../../../../core/editor/editor.service';
import type {ScriptDescriptor} from '../../../../core/editor/object-scripts.models';
import {ViewportEditorService} from '../../../../core/editor/viewport-editor.service';
import type {InspectorSubject} from '../inspector-panel/inspector-subject';
import {PropPreviewService} from '../prop-workspace/prop-preview.service';
import {PropWorkspaceService} from '../prop-workspace/prop-workspace.service';
import {SceneEditingService} from '../scene-editing.service';
import {WorkspaceViewService} from '../scene-viewport/workspace-view.service';
import {ScopedPaneRequests} from '../scoped-pane-request';
import {WorkspaceDraftReview} from '../workspace-draft-review';
import {BehaviorWorkspace} from './behavior-workspace';
import {ComponentWorkspaceSession} from './component-workspace-session';

const descriptor: ScriptDescriptor = {id: 'game.branch', label: 'Branch Script', config_version: 1,
    fields: [{kind: 'number', key: 'speed', label: 'Speed', default: 1, min: 0, max: 10}],
    references: [{key: 'target', label: 'Target object', requires: 'entity'}], receive_local_actions: true};
const script: ComponentRecord = {id: 'script-branch', enabled: true, schema_version: 1, data: {
    kind: 'script', settings: {script_id: descriptor.id, config_version: 1,
        properties: {speed: 2}, references: {target: {entity: 'friend'}}},
}};
const native: ComponentRecord = {id: 'collect', enabled: true, schema_version: 1, data: {
    kind: 'native_behavior', behavior: {kind: 'collectible', actor: 'friend', trigger: 'guard', visual: null, despawn_owner: false},
}};
const subject: InspectorSubject = {address: {kind: 'scene', scene: 'arena', object: 'guard'}, access: 'scene-edit',
    scope: {workspaceGeneration: 1, nativeGeneration: 8, revision: 3}, contentRevision: 2};
const prop: InspectorSubject = {...subject, address: {kind: 'prop', draftId: 'prop', sourceObject: 'guard'}, access: 'prop-edit'};

function setup() {
    const owner = {key: 'guard', name: 'Guard', component_views: [script, native]};
    const friend = {key: 'friend', name: 'Friend'};
    const activeViewKind = signal('scene-viewport');
    const snapshot = signal({
        active_scene_id: 'arena', mode: 'edit', revision: 3, scene_revision: 2,
        document: {entities: [owner, friend]}, project: {input_profile: {actions: [{id: 'jump'}]}},
    });
    const execute = vi.fn(async () => snapshot());
    const editComponent = vi.fn(async () => true);
    TestBed.configureTestingModule({providers: [
        ComponentWorkspaceSession, WorkspaceDraftReview, ScopedPaneRequests,
        {provide: EditorService, useValue: {workspaceGeneration: signal(1), playing: signal(false),
            snapshot, busy: signal(false), execute, modelAssets: signal([]), capabilities: () => ({object_scripts: [descriptor]}),
        }},
        {provide: ViewportEditorService, useValue: {state: signal({workspace_generation: 8})}},
        {provide: SceneEditingService, useValue: {selectedEntity: signal(owner), editable: signal(true),
            withCommittedEdit: async (action: () => Promise<void>) => { await action(); return true; }}},
        {provide: WorkspaceViewService, useValue: {activeViewKind, selectView: (view: string) => activeViewKind.set(view)}},
        {provide: PropWorkspaceService, useValue: {inspectorSubject: signal(prop), canEditDraft: signal(true), editComponent,
            document: () => ({objects: [owner, friend]}), selectedObject: () => ({...owner, components: [script]}),
            draft: () => ({prefab: {reference_slots: {'outer-target': 'prop-ref'}}}),
        }},
        {provide: PropPreviewService, useValue: {}},
    ]});
    const session = TestBed.inject(ComponentWorkspaceSession);
    return {session, activeViewKind, execute, editComponent};
}

function control<T extends HTMLElement>(root: HTMLElement, name: string, element: string): T {
    return [...root.querySelectorAll('label')].find(label => label.textContent?.trim().startsWith(name))!
        .querySelector<T>(element)!;
}

describe('Behavior workspace', () => {
    it('edits registered Script properties and keeps input actions separate from object references', async () => {
        const {session, activeViewKind, execute} = setup();
        expect(await session.open('behavior-workspace', subject, script.id)).toBe(true);
        expect(activeViewKind()).toBe('behavior-workspace');
        const fixture = TestBed.createComponent(BehaviorWorkspace);
        await fixture.whenStable();
        const root = fixture.nativeElement as HTMLElement;
        expect(root.textContent).toContain('Receives local input from the project action profile.');
        const target = control<HTMLSelectElement>(root, 'Target object', 'select');
        expect([...target.options].map(option => option.value)).toEqual(['', 'entity:guard', 'entity:friend']);
        const speed = control<HTMLInputElement>(root, 'Speed', 'input');
        speed.value = '5';
        speed.dispatchEvent(new Event('input', {bubbles: true}));
        await fixture.whenStable();
        const review = session.reviewNavigation();
        expect(session.confirming()).toBe(true);
        await session.settleNavigation('cancel');
        expect(await review).toBe(false);
        expect(speed.value).toBe('5');
        const apply = [...root.querySelectorAll('button')].find(button => button.textContent?.trim() === 'Apply Component')!;
        apply.click();
        await fixture.whenStable();
        expect(execute).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({
            type: 'replace_component', key: 'guard', component: expect.objectContaining({
                id: script.id, data: {kind: 'script', settings: {script_id: descriptor.id, config_version: 1,
                    properties: {speed: 5}, references: {target: {entity: 'friend'}}}},
            }),
        }), 1, 3);
    });

    it('uses the exact Native Behavior instance and the existing descriptor editor', async () => {
        const {session} = setup();
        expect(await session.open('behavior-workspace', subject, native.id)).toBe(true);
        const fixture = TestBed.createComponent(BehaviorWorkspace);
        await fixture.whenStable();
        expect(control<HTMLSelectElement>(fixture.nativeElement, 'Native behavior', 'select').value).toBe('collectible');
        expect(session.target()?.componentId).toBe(native.id);
    });

    it('binds a source Prop script to its declared reference slots', async () => {
        const {session, editComponent, execute} = setup();
        expect(await session.open('behavior-workspace', prop, script.id)).toBe(true);
        const fixture = TestBed.createComponent(BehaviorWorkspace);
        await fixture.whenStable();
        const target = control<HTMLSelectElement>(fixture.nativeElement, 'Target object', 'select');
        expect([...target.options].map(option => option.value)).toContain('slot:outer-target');
        target.value = 'slot:outer-target';
        target.dispatchEvent(new Event('change', {bubbles: true}));
        await fixture.whenStable();
        const root = fixture.nativeElement as HTMLElement;
        const apply = [...root.querySelectorAll('button')].find(button => button.textContent?.trim() === 'Apply Component')!;
        apply.click();
        await fixture.whenStable();
        expect(editComponent).toHaveBeenCalledExactlyOnceWith('guard', expect.objectContaining({
            kind: 'replace', component: expect.objectContaining({id: script.id, data: {kind: 'script',
                settings: {script_id: descriptor.id, config_version: 1, properties: {speed: 2}, references: {target: {slot: 'outer-target'}}},
            }}),
        }));
        expect(execute).not.toHaveBeenCalled();
    });
});
