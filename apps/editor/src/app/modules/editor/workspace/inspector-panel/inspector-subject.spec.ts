import {signal} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import {vi} from 'vitest';
import type {EditorSnapshot} from '../../../../core/editor/editor.models';
import {type LinkedPropAddress, type PrefabSourceDraft, readLinkedPropAddress} from '../../../../core/editor/linked-prefabs.models';
import {
    inspectorAddressKey, linkedInspectorSubject, type LinkedInspectorTarget, sceneInspectorSubject,
    sourceInspectorSubject, watchLinkedInspectorAddress,
} from './inspector-subject';

const fixtureScope = {workspaceGeneration: 4, nativeGeneration: 11, revision: 20};
const transform = {position: [0, 0, 0], rotation: [0, 0, 0], scale: [1, 1, 1]} as const;
const light = {key: 'light', name: 'Light', transform, components: []};
const source: PrefabSourceDraft = {
    draft_id: 'draft-one', workspace_generation: 11, revision: 14, draft_revision: 3,
    path: 'props/light.json', asset_id: 'light-prop', fingerprint: 'a'.repeat(64),
    prefab: {schema_version: 3, entities: [light], gameplay: null, reference_slots: {}},
};
function snapshot(scene: string, revision = fixtureScope.revision): EditorSnapshot {
    return {
        active_scene_id: scene, revision, scene_revision: 5, project_dirty: false, scenes: [],
        project: {
            schema_version: 3, name: 'Inspector fixture', scenes: [], startup_scene_id: scene,
            fixed_hz: 60, max_catchup_steps: 8, behaviors: [], behavior_definitions: [],
        },
        document: {
            schema_version: 3, bindings: {}, lights: [],
            camera: {position: [0, 0, 3], yaw: 0, pitch: 0, vertical_fov: 1, near: 0.1, far: 100},
            entities: [{...light, model_path: 'light.glb', renderable: true}],
        },
        selection: ['light'], transaction: null, mode: 'edit', paused: false, can_undo: false, can_redo: false,
        snap: {enabled: false, translation: 1, rotation_radians: 0.1, scale: 0.1}, dirty: false,
    };
}
function key(subject: NonNullable<ReturnType<typeof sourceInspectorSubject>>): string {
    const {address, scope} = subject;
    return inspectorAddressKey(address, scope.workspaceGeneration, scope.nativeGeneration);
}

describe('Inspector subject addressing', () => {
    it('keeps the object identity distinct across scene and native workspace changes', () => {
        const a = sceneInspectorSubject(snapshot('workshop-a'), light.key, fixtureScope, true);
        const b = sceneInspectorSubject(snapshot('workshop-b'), light.key, fixtureScope, true);
        expect(a).toEqual({
            address: {kind: 'scene', scene: 'workshop-a', object: 'light'},
            scope: fixtureScope, contentRevision: 5, access: 'scene-edit',
        });
        expect(key(a)).not.toBe(key(b));
        expect(key(a)).not.toBe(key({...a, scope: {...fixtureScope, nativeGeneration: 12}}));
        expect(key(a)).toBe(key({...a, scope: {...fixtureScope, revision: 21}, contentRevision: 6}));
    });

    it('separates two Prop drafts and makes a legacy draft read-only', () => {
        const a = sourceInspectorSubject(source, light.key, fixtureScope, true)!;
        const b = sourceInspectorSubject({...source, draft_id: 'draft-two'}, light.key, fixtureScope, true)!;
        expect(a).toMatchObject({address: {kind: 'prop', draftId: 'draft-one', sourceObject: 'light'},
            contentRevision: 3, access: 'prop-edit'});
        expect(key(a)).not.toBe(key(b));
        expect(sourceInspectorSubject({...source, draft_revision: undefined}, light.key, fixtureScope, true))
            .toMatchObject({contentRevision: 14, access: 'read-only'});
        expect(sourceInspectorSubject(source, light.key, {...fixtureScope, nativeGeneration: 12}, true)).toBeNull();
        expect(sourceInspectorSubject(source, 'missing', fixtureScope, true)).toBeNull();
    });

    it('keeps a late reply from a previous linked instance out of the active subject', async () => {
        const origin = {scene: 'workshop-a', root: 'left', object: 'member-a', scope: fixtureScope};
        const next = {...origin, root: 'right', object: 'member-b'};
        const target = signal<LinkedInspectorTarget | null>(origin);
        const replies: ((response: LinkedPropAddress) => void)[] = [];
        const inspectLinkedPropAddress = vi.fn(() => new Promise<LinkedPropAddress>(resolve => replies.push(resolve)));
        const address = TestBed.runInInjectionContext(() => watchLinkedInspectorAddress({inspectLinkedPropAddress}, target));
        TestBed.tick();
        target.set(next);
        TestBed.tick();
        const exact = (selected: LinkedInspectorTarget, placement: string): LinkedPropAddress => ({
            root_key: selected.root, entity_key: selected.object, scene_id: selected.scene,
            address: {kind: 'object', placement_path: [placement, 'lamp'], source_entity: light.key},
        });
        replies[1](exact(next, 'stand-right'));
        await Promise.resolve();
        replies[0](exact(origin, 'stand-left'));
        await Promise.resolve();
        const result = address();
        expect(linkedInspectorSubject(snapshot(next.scene), next, result, true)).toMatchObject({
            address: {kind: 'linked', scene: next.scene, root: next.root, address: exact(next, 'stand-right').address},
            scope: fixtureScope, access: 'linked-overrides',
        });
        expect(linkedInspectorSubject(snapshot(next.scene, 21), next, result, true)).toBeNull();
        expect(linkedInspectorSubject(snapshot(origin.scene), origin, result, true)).toBeNull();
        expect(() => readLinkedPropAddress({...result, address: {kind: 'placement', placement_path: []}}))
            .toThrow('The linked Prop address is invalid.');
    });
});
