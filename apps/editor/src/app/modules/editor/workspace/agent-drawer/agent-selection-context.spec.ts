import {computed, signal} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import {vi} from 'vitest';
import type {EditorSnapshot, SceneEntity} from '../../../../core/editor/editor.models';
import {EditorService} from '../../../../core/editor/editor.service';
import type {LinkedPropAddress, PrefabSourceDraft} from '../../../../core/editor/linked-prefabs.models';
import {ViewportEditorService} from '../../../../core/editor/viewport-editor.service';
import {ComponentWorkspaceSession} from '../components/component-workspace-session';
import {sceneInspectorSubject, sourceInspectorSubject, type InspectorSubject} from '../inspector-panel/inspector-subject';
import {PropWorkspaceService, type PropWorkspaceDocument, type PropWorkspaceObject} from '../prop-workspace/prop-workspace.service';
import {WorkspaceViewService} from '../scene-viewport/workspace-view.service';
import {AgentSelectionContextService} from './agent-selection-context';

const scope = {workspaceGeneration: 4, nativeGeneration: 11, revision: 20};
const transform = {position: [0, 0, 0], rotation: [0, 0, 0], scale: [1, 1, 1]} as const;
const first: SceneEntity = {key: 'first', name: 'Lamp', transform, model_path: 'lamp.glb', renderable: true,
    component_ids: {collider: 'first-collider'}};
const second: SceneEntity = {...first, key: 'second', component_ids: {collider: 'second-collider'}};
const sourceObject: PropWorkspaceObject = {key: 'lamp', name: 'Lamp', transform, parent: null, level: 0, components: []};
const draft: PrefabSourceDraft = {
    draft_id: 'draft-one', workspace_generation: scope.nativeGeneration, revision: 14, draft_revision: 3,
    path: 'props/lamp.json', asset_id: 'lamp-prop', fingerprint: 'a'.repeat(64),
    prefab: {schema_version: 3, entities: [sourceObject], gameplay: null, reference_slots: {}},
};

function snapshot(entities: readonly SceneEntity[] = [first, second], selection = first.key): EditorSnapshot {
    return {
        active_scene_id: 'room', revision: scope.revision, scene_revision: 5, project_dirty: false, scenes: [],
        project: {
            schema_version: 3, name: 'Agent fixture', scenes: [], startup_scene_id: 'room',
            fixed_hz: 60, max_catchup_steps: 8, behaviors: [], behavior_definitions: [],
        },
        document: {
            schema_version: 3, bindings: {}, lights: [], entities,
            camera: {position: [0, 0, 3], yaw: 0, pitch: 0, vertical_fov: 1, near: 0.1, far: 100},
        },
        selection: [selection], transaction: null, mode: 'edit', paused: false, can_undo: false, can_redo: false,
        snap: {enabled: false, translation: 1, rotation_radians: 0.1, scale: 0.1}, dirty: false,
    };
}

function setup() {
    const current = signal<EditorSnapshot | null>(snapshot());
    const generation = signal(scope.workspaceGeneration);
    const native = signal(scope.nativeGeneration);
    const propView = signal(false);
    const audioView = signal(false);
    const behaviorView = signal(false);
    const detail = signal<{subject: InspectorSubject; ownerKey: string; componentId: string} | null>(null);
    const propDocument = signal<PropWorkspaceDocument | null>(null);
    const selected = signal<PropWorkspaceObject | null>(null);
    const linkedAddress = signal<LinkedPropAddress | null>(null);
    const inspectLinkedPropAddress = vi.fn(async () => linkedAddress());
    const suppliedSubject = signal<InspectorSubject | null>(null);
    const propSubject = computed(() => {
        const document = propDocument(), object = selected();
        return document?.kind === 'source' && object ?
            sourceInspectorSubject(document.draft, object.key, scope, true) : null;
    });
    TestBed.configureTestingModule({providers: [
        AgentSelectionContextService,
        {provide: EditorService, useValue: {snapshot: current, workspaceGeneration: generation, inspectLinkedPropAddress}},
        {provide: ViewportEditorService, useValue: {state: () => ({workspace_generation: native()})}},
        {provide: PropWorkspaceService, useValue: {document: propDocument, selectedObject: selected,
            inspectorSubject: propSubject, canEditDraft: () => true}},
        {provide: WorkspaceViewService, useValue: {propView, animationView: () => false, audioView, behaviorView}},
        {provide: ComponentWorkspaceSession, useValue: {target: detail}},
    ]});
    const service = TestBed.inject(AgentSelectionContextService);
    service.setSceneSubject(() => {
        const state = current(), key = state?.selection[0];
        return suppliedSubject() ?? (state && key ? sceneInspectorSubject(state, key, {
            workspaceGeneration: generation(), nativeGeneration: native(), revision: state.revision,
        }, true) : null);
    });
    return {service, current, generation, native, propView, propDocument, selected, linkedAddress,
        suppliedSubject, inspectLinkedPropAddress, detail, audioView, behaviorView};
}

describe('Agent selection context', () => {
    it('captures exact component IDs for two same-named objects and preserves the submitted label', () => {
        const {service, current} = setup();
        const capture = service.capture('first-collider')!;
        expect(capture).toMatchObject({label: 'Lamp', componentId: 'first-collider',
            subject: {address: {kind: 'scene', scene: 'room', object: 'first'}, scope, contentRevision: 5}});
        current.set(snapshot([first, {...second, name: 'Second Lamp'}], second.key));
        expect(service.capture('first-collider')).toBeNull();
        expect(service.capture('second-collider')).toMatchObject({label: 'Second Lamp',
            subject: {address: {kind: 'scene', object: 'second'}}});
        expect(capture.label).toBe('Lamp');
        expect(service.canDeliver(capture)).toBe(true);
    });

    it('captures the owned component when a detailed Audio or Behavior workspace is active', () => {
        const {service, current, detail, audioView, behaviorView} = setup();
        const subject = sceneInspectorSubject(current()!, second.key, scope, true);
        detail.set({subject, ownerKey: second.key, componentId: 'second-collider'});
        audioView.set(true);
        expect(current()?.selection).toEqual([first.key]);
        expect(service.capture()).toMatchObject({subject, label: 'Lamp', componentId: 'second-collider'});
        audioView.set(false);
        behaviorView.set(true);
        expect(service.componentIds()).toEqual(['second-collider']);
        expect(service.capture('first-collider')).toBeNull();
        detail.set(null);
        expect(service.capture()).toBeNull();
    });

    it('keeps a removed pin unavailable and drops it on workspace change', () => {
        const {service, current, generation} = setup();
        service.togglePin();
        const capture = service.capture()!;
        current.set(snapshot([first, second], second.key));
        expect(service.capture()?.subject.address).toMatchObject({kind: 'scene', object: first.key});
        current.set(snapshot([second], second.key));
        expect(service.pinnedLabel()).toBe(first.name);
        expect(service.capture()).toBeNull();
        expect(service.canDeliver(capture)).toBe(true);
        generation.update(value => value + 1);
        expect(service.pinned()).toBe(false);
        expect(service.pinnedLabel()).toBeNull();
        expect(service.canDeliver(capture)).toBe(false);
    });

    it('preserves the complete nested path when focus moves to another instance', async () => {
        const {service, current, suppliedSubject, linkedAddress, inspectLinkedPropAddress} = setup();
        const linked_prefab = {source: {source_id: 'lamp', path: 'props/lamp.json'}, bindings: {}};
        const left = {...first, key: 'left-root', linked_prefab};
        const right = {...second, key: 'right-root', linked_prefab};
        const entities = [left, right, {...first, parent: left.key}, {...second, parent: right.key}];
        const path = {kind: 'object', placement_path: ['shelf', 'lamp'], source_entity: 'bulb'} as const;
        const leftTarget = {kind: 'linked', scene: 'room', root: left.key, address: path} as const;
        current.set(snapshot(entities, first.key));
        suppliedSubject.set({scope, address: leftTarget, contentRevision: 5, access: 'linked-overrides'});
        linkedAddress.set({root_key: left.key, entity_key: first.key, scene_id: 'room', address: path});
        const capture = service.capture()!;
        service.togglePin();
        current.set(snapshot(entities, second.key));
        suppliedSubject.set({...capture.subject, address: {...leftTarget, root: right.key}});
        TestBed.tick();
        await vi.waitFor(() => expect(service.capture()?.subject.address).toEqual(leftTarget));
        expect(inspectLinkedPropAddress).toHaveBeenCalledWith(first.key, scope);
        service.togglePin();
        expect(service.capture()?.subject.address).toEqual({...leftTarget, root: right.key});
        expect(capture.subject.address).toEqual(leftTarget);
    });

    it('binds a Prop source capture to its draft revision and invalidates a replaced native workspace', () => {
        const {service, propView, propDocument, selected, native} = setup();
        propView.set(true);
        selected.set(sourceObject);
        const document: PropWorkspaceDocument = {
            kind: 'source', scope: {generation: scope.workspaceGeneration, sceneId: 'room'}, draft, name: 'Lamp Prop',
            path: draft.path, objects: [sourceObject], placements: [], references: [],
        };
        propDocument.set(document);
        service.togglePin();
        const capture = service.capture()!;
        expect(capture.subject).toMatchObject({address: {kind: 'prop', draftId: draft.draft_id, sourceObject: sourceObject.key},
            scope, contentRevision: 3});
        propDocument.set({...document, draft: {...draft, draft_revision: 4}});
        expect(service.capture()?.subject.contentRevision).toBe(4);
        expect(capture.subject.contentRevision).toBe(3);
        native.update(value => value + 1);
        expect(service.canDeliver(capture)).toBe(false);
        expect(service.capture()).toBeNull();
    });
});
