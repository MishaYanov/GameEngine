import {signal} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import {vi} from 'vitest';
import type {AnimationSettings} from '../../../../core/editor/animation.models';
import type {AnimationWorkspaceTarget, ScopedAnimationPreview} from '../../../../core/editor/animation-workspace.models';
import type {AuthoredClip, AuthoredClipPreviewRequest, ClipAssetDocument, VisualClipPreview} from '../../../../core/editor/authored-clip.models';
import type {ComponentRecord} from '../../../../core/editor/editor-components';
import type {SceneEntity, Transform} from '../../../../core/editor/editor.models';
import {EditorService} from '../../../../core/editor/editor.service';
import type {NestedPropPlacement, PrefabSourceDocument} from '../../../../core/editor/linked-prefabs.models';
import {PropWorkspaceService, type PropWorkspaceDocument} from '../prop-workspace/prop-workspace.service';
import {SceneEditingService} from '../scene-editing.service';
import {WorkspaceViewService} from '../scene-viewport/workspace-view.service';
import {AnimationWorkspace} from './animation-workspace';
import {AnimationWorkspaceService} from './animation-workspace.service';

vi.mock('@tauri-apps/api/event', () => ({listen: vi.fn(async () => () => undefined)}));

const transform: Transform = {position: [0, 0, 0], rotation: [0, 0, 0], scale: [1, 1, 1]};
const settings: AnimationSettings = {
    source: {
        asset_id: '10000000-0000-0000-0000-000000000001', expected_sha256: 'a'.repeat(64),
        scene_index: 0, mesh_node: 1, skin_index: 0,
    },
    playback: {kind: 'single_clip', clip_index: 0, looping: true},
    speed_numerator: 1,
    speed_denominator: 1,
};
const actor: SceneEntity = {
    key: 'actor', name: 'Knight', renderable: true, model_path: 'knight.glb', transform,
    animation: {id: 'animation-component', enabled: true, settings},
};
const clips = [
    {index: 0, name: 'Idle', duration_seconds: 1},
    {index: 1, name: 'Walk', duration_seconds: 1},
    {index: 2, name: 'Turn', duration_seconds: 1},
];

function setup() {
    const selected = signal<SceneEntity | null>(actor);
    const entities = signal<readonly SceneEntity[]>([actor]);
    const generation = signal(1);
    const revision = signal(4);
    const active = signal(true);
    const origin = signal<'scene' | 'prop'>('scene');
    const playing = signal(false);
    const propSource = signal<PropWorkspaceDocument | null>(null);
    const placement = signal<NestedPropPlacement | null>(null);
    const opened = new Map<string, ScopedAnimationPreview>();
    const open = vi.fn(async (
        target: AnimationWorkspaceTarget, clip: number, _generation: number, revision: number,
        nativeGeneration: number, request: number,
    ): Promise<ScopedAnimationPreview> => {
        const token = crypto.randomUUID();
        const preview: ScopedAnimationPreview = {
            token, target, request_id: request, revision, entity_key: target.entity_key, clip_index: clip,
            workspace_generation: nativeGeneration, duration_seconds: 1, time_seconds: 0,
        };
        opened.set(token, preview);
        return preview;
    });
    const inspect = vi.fn(async () => ({
        workspace_generation: 11, revision: revision(),
        metadata: {source: settings.source, mesh_index: 0, nodes: 2, joints: 2, clips},
    }));
    const sample = vi.fn(async (token: string, seconds: number, _generation: number) => ({
        ...opened.get(token)!, time_seconds: seconds,
    }));
    const close = vi.fn(async (_token: string) => undefined);
    const frame = vi.fn(async (_token: string) => undefined);
    const library = new Map<string, ClipAssetDocument>();
    const listAuthoredClips = vi.fn(async () => [...library.values()].map(entry => entry.asset));
    const openAuthoredClip = vi.fn(async (id: string) => library.get(id)!);
    const saveAuthoredClip = vi.fn(async (path: string, clip: AuthoredClip) => {
        const document: ClipAssetDocument = {
            asset: {asset_id: 'authored-clip-id', path, fingerprint: 'e'.repeat(64), revision: 1},
            clip, workspace_generation: 11,
        };
        library.set(document.asset.asset_id, document);
        return document;
    });
    let authoredPreview: VisualClipPreview;
    const openAuthoredClipPreview = vi.fn(async (request: AuthoredClipPreviewRequest) => {
        authoredPreview = {
            token: crypto.randomUUID(), workspace_generation: 11, revision: request.expectedRevision,
            request_id: request.requestId, duration_ticks: 1000, tick: 0,
            poses: [{entity_key: 'lever', position: [0, 0, 0], rotation_xyzw: [0, 0, 0, 1], scale: [1, 1, 1]}],
        };
        return authoredPreview;
    });
    const sampleAuthoredClipPreview = vi.fn(async (_token: string, tick: number, _generation: number) => ({
        ...authoredPreview, tick,
        poses: [{...authoredPreview.poses[0], position: [tick / 250, 0, 0] as const}],
    }));
    const closeAuthoredClipPreview = vi.fn(async (_token: string) => undefined);
    TestBed.configureTestingModule({
        providers: [
            AnimationWorkspaceService,
            {provide: EditorService, useValue: {
                nativeAvailable: true, workspaceGeneration: generation, playing,
                snapshot: () => ({
                    revision: revision(), transaction: null, mode: playing() ? 'play' : 'edit',
                    active_scene_id: 'arena', document: {entities: entities()},
                }),
                inspectAnimation: inspect, openAnimationWorkspacePreview: open,
                sampleAnimationWorkspacePreview: sample, closeAnimationPreview: close, frameAnimationWorkspacePreview: frame,
                listAuthoredClips, openAuthoredClip, saveAuthoredClip, openAuthoredClipPreview, sampleAuthoredClipPreview, closeAuthoredClipPreview,
            }},
            {provide: SceneEditingService, useValue: {entity: selected}},
            {provide: WorkspaceViewService, useValue: {animationView: active, animationOrigin: origin}},
            {provide: PropWorkspaceService, useValue: {document: propSource, selectedObject: () => null, selectedPlacement: placement}},
        ],
    });
    const fixture = TestBed.createComponent(AnimationWorkspace);
    return {
        fixture, selected, generation, revision, active, origin, playing, entities, propSource, placement,
        open, inspect, sample, close, frame, opened, saveAuthoredClip, openAuthoredClip, sampleAuthoredClipPreview, closeAuthoredClipPreview, workspace: TestBed.inject(AnimationWorkspaceService),
    };
}

describe('Animation workspace', () => {
    it('offers a clear empty view and opens the selected imported clip independently', async () => {
        const {fixture, selected, inspect, open, workspace, frame} = setup();
        selected.set(null);
        await fixture.whenStable();
        expect(fixture.nativeElement.textContent).toContain('Select a visible object with an Animation component');
        expect(inspect).not.toHaveBeenCalled();
        selected.set(actor);
        await fixture.whenStable();
        expect(workspace.ready()).toBe(true);
        expect(inspect).toHaveBeenCalledWith(settings.source.asset_id, 1, 4);
        expect(open).toHaveBeenCalledWith({
            document: {kind: 'scene', scene_id: 'arena'}, entity_key: 'actor', component_id: 'animation-component',
        }, 0, 1, 4, 11, expect.any(Number));
        const clip = fixture.nativeElement.querySelector('select') as HTMLSelectElement;
        clip.value = '1';
        clip.dispatchEvent(new Event('change'));
        await fixture.whenStable();
        expect(workspace.preview()?.clip_index).toBe(1);
        expect(settings.playback).toEqual({kind: 'single_clip', clip_index: 0, looping: true});
        const buttons = fixture.nativeElement.querySelectorAll('button') as NodeListOf<HTMLButtonElement>;
        Array.from(buttons).find(button => button.textContent?.includes('Frame Animated Object'))?.click();
        expect(frame).toHaveBeenCalledWith(workspace.preview()?.token);
    });

    it('coalesces scrub inputs and ignores samples after a clip change or tab exit', async () => {
        const {fixture, workspace, sample, close, active, opened} = setup();
        await fixture.whenStable();
        const old = workspace.preview()!;
        let release!: (result: ScopedAnimationPreview) => void;
        sample.mockImplementationOnce(() => new Promise(resolve => { release = resolve; }));
        const slider = fixture.nativeElement.querySelector('input[type=range]') as HTMLInputElement;
        for (const seconds of [0.1, 0.45, 0.8]) {
            slider.value = String(seconds);
            slider.dispatchEvent(new Event('input'));
        }
        expect(sample).toHaveBeenCalledTimes(1);
        release({...old, time_seconds: 0.1});
        await fixture.whenStable();
        expect(sample.mock.calls.map(call => call[1])).toEqual([0.1, 0.8]);
        expect(workspace.seconds()).toBe(0.8);

        sample.mockImplementationOnce(() => new Promise(resolve => { release = resolve; }));
        workspace.seek(0.2);
        workspace.chooseClip(1);
        await fixture.whenStable();
        const latest = workspace.preview()!;
        expect(latest.clip_index).toBe(1);
        expect(opened.has(latest.token)).toBe(true);
        release({...old, time_seconds: 0.2});
        await fixture.whenStable();
        expect(workspace.seconds()).toBe(0);
        expect(workspace.preview()?.token).toBe(latest.token);
        expect(close).toHaveBeenCalledWith(old.token);
        active.set(false);
        await fixture.whenStable();
        expect(workspace.ready()).toBe(false);
        expect(close).toHaveBeenCalledWith(latest.token);
    });

    it('binds equal source-local children to distinct scene instance owners and releases on Play', async () => {
        const {fixture, selected, entities, revision, open, workspace, playing, close} = setup();
        const link = {source: {source_id: 'asset', path: 'knight.json'}, bindings: {}};
        const root = (key: string): SceneEntity => ({...actor, key, animation: null, renderable: false, linked_prefab: link});
        const first = {...actor, key: 'member-1', parent: 'anchor-1'};
        const second = {...actor, key: 'member-2', parent: 'anchor-2'};
        entities.set([root('anchor-1'), root('anchor-2'), first, second]);
        selected.set(first);
        await fixture.whenStable();
        const previous = workspace.preview()!;
        selected.set(second);
        revision.set(5);
        await fixture.whenStable();
        expect(open.mock.calls.map(([target]) => [target.document, target.entity_key])).toEqual([
            [{kind: 'instance', scene_id: 'arena', root_key: 'anchor-1'}, 'member-1'],
            [{kind: 'instance', scene_id: 'arena', root_key: 'anchor-2'}, 'member-2'],
        ]);
        expect(close).toHaveBeenCalledWith(previous.token);
        const active = workspace.preview()!;
        playing.set(true);
        await fixture.whenStable();
        expect(workspace.preview()).toBeNull();
        expect(close).toHaveBeenCalledWith(active.token);
    });

    it('targets a nested object in its retained Prop source draft and invalidates a changed draft', async () => {
        const {fixture, propSource, placement, origin, open, close, workspace} = setup();
        const component: ComponentRecord = {
            id: 'animation-component', enabled: true, schema_version: 1, data: {kind: 'animation', settings},
        };
        const pinVersion = {asset_id: '10000000-0000-0000-0000-000000000002', fingerprint: 'b'.repeat(64)};
        const item: NestedPropPlacement = {id: 'performer', name: 'Performer', target: pinVersion, parent: 'root', local: transform};
        const prefab: PrefabSourceDocument = {
            schema_version: 4, gameplay: null, reference_slots: {},
            entities: [{key: 'root', name: 'Stage', transform, components: []}],
            composition: {
                placements: [item], pins: [{version: pinVersion, path: 'prefabs/knight.json',
                    entities: [{key: 'actor', name: 'Knight', transform, components: [component]}]}],
            },
        };
        const source: PropWorkspaceDocument = {
            kind: 'source', scope: {generation: 1, sceneId: 'arena'}, path: 'prefabs/stage.json', name: 'Stage',
            objects: [], placements: [item], references: [],
            draft: {
                asset_id: '10000000-0000-0000-0000-000000000003',
                draft_id: 'stage-draft', workspace_generation: 11, revision: 4, draft_revision: 3,
                path: 'prefabs/stage.json', fingerprint: 'c'.repeat(64), prefab,
            },
        };
        propSource.set(source);
        placement.set(item);
        origin.set('prop');
        await fixture.whenStable();
        expect(fixture.nativeElement.textContent).toContain('Performer › Knight');
        expect(open.mock.calls.at(-1)?.[0]).toEqual({
            document: {kind: 'source', asset_id: source.draft.asset_id, draft_id: 'stage-draft', draft_revision: 3},
            address: {kind: 'object', placement_path: ['performer'], source_entity: 'actor'},
            entity_key: 'actor', component_id: 'animation-component',
        });
        const previous = workspace.preview()!;
        propSource.set({...source, draft: {...source.draft, draft_revision: 4}});
        await fixture.whenStable();
        expect(workspace.preview()?.target.document).toMatchObject({draft_revision: 4});
        expect(close).toHaveBeenCalledWith(previous.token);
    });
    it('creates a visual clip, reopens its authored keys and samples halfway without a Scene edit', async () => {
        const {fixture, selected, entities, workspace, saveAuthoredClip, openAuthoredClip, sampleAuthoredClipPreview, closeAuthoredClipPreview} = setup();
        const lever: SceneEntity = {...actor, name: 'Lever', key: 'lever', animation: null, parent: 'pivot'};
        selected.set(lever);
        entities.set([{...actor, key: 'pivot', renderable: false}, lever]);
        await fixture.whenStable();
        const root = fixture.nativeElement as HTMLElement;
        const button = (name: string) => Array.from(root.querySelectorAll('button'))
            .find(item => item.textContent?.trim() === name)!;
        button('Authored Clips').click();
        await fixture.whenStable();
        button('New Visual Clip').click();
        await fixture.whenStable();
        const name = Array.from(root.querySelectorAll('label'))
            .find(item => item.textContent?.trim() === 'Clip name')?.querySelector('input') as HTMLInputElement;
        name.value = 'Lever Turn';
        name.dispatchEvent(new Event('input'));
        const end = root.querySelector<HTMLInputElement>('input[aria-label="End position X"]')!;
        end.value = '4';
        end.dispatchEvent(new Event('input'));
        await fixture.whenStable();
        button('Save Clip').click();
        await fixture.whenStable();
        expect(saveAuthoredClip).toHaveBeenCalledWith('lever-turn.clip.json', expect.objectContaining({
            name: 'Lever Turn', duration_ticks: 1000,
            owner: {kind: 'scene', scene_id: 'arena'},
        }), null, 1, 4);
        expect(saveAuthoredClip.mock.calls[0][1].tracks[0]).toEqual({
            target: {kind: 'scene', entity_key: 'lever'},
            track: {kind: 'local_position', interpolation: 'linear', keys: [
                {at: 0, value: [0, 0, 0]}, {at: 1000, value: [4, 0, 0]},
            ]},
        });
        const library = root.querySelector('select')!;
        library.value = 'authored-clip-id';
        library.dispatchEvent(new Event('change'));
        await fixture.whenStable();
        expect(openAuthoredClip).toHaveBeenCalledWith('authored-clip-id', 1);
        expect(root.querySelector<HTMLInputElement>('input[aria-label="End position X"]')?.value).toBe('4');
        button('Preview Saved Clip').click();
        await fixture.whenStable();
        const slider = root.querySelector<HTMLInputElement>('input[type=range]')!;
        slider.value = '500';
        slider.dispatchEvent(new Event('change'));
        await fixture.whenStable();
        expect(sampleAuthoredClipPreview).toHaveBeenCalledWith(expect.any(String), 500, 1);
        expect(root.textContent).toContain('Position 2, 0, 0');
        expect(workspace.ready()).toBe(true);
        button('Imported Clips').click();
        await fixture.whenStable();
        expect(closeAuthoredClipPreview).toHaveBeenCalled();
    });
});
