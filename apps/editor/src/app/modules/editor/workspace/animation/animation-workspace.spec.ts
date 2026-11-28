import {signal} from '@angular/core';
import {type ComponentFixture, TestBed} from '@angular/core/testing';
import {vi} from 'vitest';
import type {AnimationSettings} from '../../../../core/editor/animation.models';
import type {AnimationWorkspaceTarget, ScopedAnimationPreview} from '../../../../core/editor/animation-workspace.models';
import type {
    AuthoredClip,
    AuthoredClipPreviewRequest,
    ClipAssetDocument,
    ClipAssetVersion,
    VisualClipPreview
} from '../../../../core/editor/authored-clip.models';
import type {ComponentRecord} from '../../../../core/editor/editor-components';
import type {SceneEntity, Transform} from '../../../../core/editor/editor.models';
import {EditorService} from '../../../../core/editor/editor.service';
import type {NestedPropPlacement, PrefabSourceDocument} from '../../../../core/editor/linked-prefabs.models';
import {ViewportEditorService} from '../../../../core/editor/viewport-editor.service';
import {PropWorkspaceService, type PropWorkspaceDocument} from '../prop-workspace/prop-workspace.service';
import {SceneEditingService} from '../scene-editing.service';
import {WorkspaceViewService} from '../scene-viewport/workspace-view.service';
import {AnimationWorkspace} from './animation-workspace';
import {AnimationWorkspaceService} from './animation-workspace.service';
import type {AnimationPaneRequest} from './animation-pane-request';
import {ScopedPaneRequests} from '../scoped-pane-request';
import {WorkspaceDraftReview} from '../workspace-draft-review';

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

function setup(useRealViews = false) {
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
    const importedAnimationKeys = vi.fn(async (token: string, track: number, offset: number) => ({
        token, track, offset, clip_index: opened.get(token)?.clip_index ?? 0, total: 2,
        tracks: [{index: 0, source_node: 2, property: 'Rotation', interpolation: 'Linear', key_count: 2}],
        keys: [{at_seconds: 0, value: [0, 0, 0, 1]}, {at_seconds: 0.75, value: [0, 1, 0, 0]}],
    }));
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
    const saveAuthoredClip = vi.fn(async (path: string, clip: AuthoredClip, replacement?: ClipAssetVersion | null) => {
        const previous = replacement ? library.get(replacement.asset_id) : undefined;
        const document: ClipAssetDocument = {
            asset: {
                asset_id: previous?.asset.asset_id ?? 'authored-clip-id', path,
                fingerprint: (previous ? 'f' : 'e').repeat(64), revision: (previous?.asset.revision ?? 0) + 1
            },
            clip, workspace_generation: 11,
        };
        library.set(document.asset.asset_id, document);
        return document;
    });
    const openClipTimelineDraft = vi.fn(async (id: string) => ({
        ...library.get(id)!, draft_id: 'timeline-draft', draft_revision: 0,
        dirty: false, can_undo: false, can_redo: false,
    }));
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
            AnimationWorkspaceService, ScopedPaneRequests, WorkspaceDraftReview,
            {provide: EditorService, useValue: {
                nativeAvailable: true, workspaceGeneration: generation, playing,
                snapshot: () => ({
                    revision: revision(), transaction: null, mode: playing() ? 'play' : 'edit',
                    active_scene_id: 'arena', document: {entities: entities()},
                }),
                inspectAnimation: inspect, openAnimationWorkspacePreview: open, importedAnimationKeys,
                sampleAnimationWorkspacePreview: sample, closeAnimationPreview: close, frameAnimationWorkspacePreview: frame,
                listAuthoredClips, openAuthoredClip, saveAuthoredClip, openAuthoredClipPreview, sampleAuthoredClipPreview, closeAuthoredClipPreview,
                    openClipTimelineDraft,
                openClipTimelinePreview: openAuthoredClipPreview,
                sampleClipTimelinePreview: sampleAuthoredClipPreview,
                closeClipTimelineDraft: async () => undefined,
            }},
            {provide: SceneEditingService, useValue: {entity: selected}},
            useRealViews ? WorkspaceViewService
                : {
                    provide: WorkspaceViewService,
                    useValue: {
                        animationView: active,
                        animationOrigin: origin,
                        setAnimationScope: (value: 'scene' | 'prop') => origin.set(value)
                    }
                },
            {provide: ViewportEditorService, useValue: {state: () => ({workspace_generation: 11, settings: {view: playing() ? 'game' : 'scene'}}), gameStatus: signal(null)}},
            {provide: PropWorkspaceService, useValue: {document: propSource, selectedObject: () => null, selectedPlacement: placement}},
        ],
    });
    const fixture = TestBed.createComponent(AnimationWorkspace);
    const workspace = TestBed.inject(AnimationWorkspaceService);
    TestBed.inject(WorkspaceDraftReview).setReviewers([() => workspace.reviewNavigation()]);
    return {
        fixture, selected, generation, revision, active, origin, playing, entities, propSource, placement,
        views: TestBed.inject(WorkspaceViewService),
        open, inspect, sample, close, frame, opened, saveAuthoredClip, openAuthoredClip, openClipTimelineDraft,
        openAuthoredClipPreview, sampleAuthoredClipPreview, closeAuthoredClipPreview, workspace, library,
    };
}

async function pickAnimationOption(fixture: ComponentFixture<AnimationWorkspace>, picker: string, label: string): Promise<void> {
    const root: HTMLElement = fixture.nativeElement;
    const element = Array.from(root.querySelectorAll('editor-search-picker'))
        .find(item => item.querySelector('label')?.textContent?.trim() === picker)!;
    element.querySelector<HTMLInputElement>('[role="combobox"]')!.click();
    await fixture.whenStable();
    const option = Array.from(element.querySelectorAll<HTMLElement>('[role="option"]'))
        .find(item => item.querySelector('span')?.textContent?.trim() === label)!;
    option.click();
    await fixture.whenStable();
}

const importedRequest: AnimationPaneRequest = {
    library: 'imported',
    clipIndex: 2,
    subject: {
        address: {kind: 'scene', scene: 'arena', object: actor.key},
        scope: {workspaceGeneration: 1, nativeGeneration: 11, revision: 4},
        contentRevision: 5, access: 'scene-edit',
    },
    componentId: actor.animation!.id,
    asset: {id: settings.source.asset_id, fingerprint: settings.source.expected_sha256},
};

describe('Animation workspace', () => {
    it('reviews drafts and loads the requested imported clip before opening the pane', async () => {
        const {fixture, workspace, views, inspect, open} = setup(true);
        await fixture.whenStable();
        const review = vi.fn(async () => false);
        TestBed.inject(WorkspaceDraftReview).setReviewers([review]);
        const resolve = () => importedRequest;
        expect(await workspace.openScoped(importedRequest, resolve)).toBe(false);
        expect(views.activeViewKind()).toBe('scene-viewport');
        expect(inspect).not.toHaveBeenCalled();

        review.mockResolvedValue(true);
        const missing = {...importedRequest, clipIndex: 99};
        expect(await workspace.openScoped(missing, resolve)).toBe(false);
        expect(views.activeViewKind()).toBe('scene-viewport');
        expect(open).not.toHaveBeenCalled();

        inspect.mockClear();
        expect(await workspace.openScoped(importedRequest, resolve)).toBe(true);
        await fixture.whenStable();
        expect(views.activeViewKind()).toBe('animation-workspace');
        expect(inspect).toHaveBeenCalledExactlyOnceWith(settings.source.asset_id, 1, 4);
        expect(open).toHaveBeenCalledWith(expect.objectContaining({entity_key: actor.key}), 2, 1, 4, 11, expect.any(Number));
        expect(workspace.clipIndex()).toBe(2);
    });

    it('leaves the original pane intact if import preparation finishes after its binding changes', async () => {
        const {fixture, workspace, views, inspect, open} = setup(true);
        await fixture.whenStable();
        const response = await inspect();
        inspect.mockClear();
        let finish!: (review: typeof response) => void;
        inspect.mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }));
        const target = signal<AnimationPaneRequest | null>(importedRequest);
        const result = workspace.openScoped(importedRequest, target);
        await vi.waitFor(() => expect(inspect).toHaveBeenCalledOnce());
        expect(views.activeViewKind()).toBe('scene-viewport');
        target.set({...importedRequest, asset: {...importedRequest.asset, fingerprint: 'b'.repeat(64)}});
        finish(response);
        expect(await result).toBe(false);
        expect(views.activeViewKind()).toBe('scene-viewport');
        expect(open).not.toHaveBeenCalled();
    });

    it('opens the exact requested clip for the selected nested linked Prop object', async () => {
        const {fixture, workspace, selected, entities, open} = setup(true);
        const outer: SceneEntity = {
            ...actor, key: 'assembly', animation: null, renderable: false,
            linked_prefab: {source: {source_id: 'assembly-source', path: 'props/assembly.json'}, bindings: {}},
        };
        const member = {...actor, key: 'left-knight', parent: outer.key};
        entities.set([outer, member, {...member, key: 'right-knight'}]);
        selected.set(member);
        const address = {kind: 'object', placement_path: ['left', 'knight'], source_entity: 'knight'} as const;
        const request: AnimationPaneRequest = {
            ...importedRequest,
            subject: {...importedRequest.subject, access: 'linked-overrides',
                address: {kind: 'linked', scene: 'arena', root: outer.key, address}},
        };
        expect(await workspace.openScoped(request, () => request)).toBe(true);
        await fixture.whenStable();
        expect(open).toHaveBeenCalledWith({
            document: {kind: 'instance', scene_id: 'arena', root_key: outer.key},
            entity_key: member.key, component_id: actor.animation!.id, address,
        }, 2, 1, 4, 11, expect.any(Number));
        expect(workspace.preview()?.target.entity_key).toBe(member.key);
    });

    it('preloads an authored marker and opens its precise timeline instead of a library default', async () => {
        const {fixture, workspace, views, openAuthoredClip, saveAuthoredClip} = setup(true);
        const document = await saveAuthoredClip('props/knight.clip.json', {
            schema_version: 2, name: 'Knight Turn', duration_ticks: 1000,
            owner: {kind: 'scene', scene_id: 'arena'},
            markers: [{id: 'turn-finish', label: 'Turn finish', at: 376}],
            tracks: [{target: {kind: 'scene', entity_key: actor.key}, track: {
                kind: 'local_position', interpolation: 'linear',
                keys: [{id: 'start', at: 0, value: [0, 0, 0]}, {id: 'end', at: 1000, value: [1, 0, 0]}],
            }}],
        });
        const request: AnimationPaneRequest = {
            library: 'authored', markerId: 'turn-finish', subject: importedRequest.subject,
            asset: {id: document.asset.asset_id, revision: document.asset.revision, fingerprint: document.asset.fingerprint},
        };
        expect(await workspace.openScoped({...request, markerId: 'removed-marker'}, () => request)).toBe(false);
        expect(views.activeViewKind()).toBe('scene-viewport');
        openAuthoredClip.mockClear();
        expect(await workspace.openScoped(request, () => request)).toBe(true);
        await fixture.whenStable();
        const timeline = fixture.nativeElement.querySelector('editor-clip-timeline') as HTMLElement;
        expect(openAuthoredClip).toHaveBeenCalledExactlyOnceWith(document.asset.asset_id, 1);
        expect(timeline.querySelector('[aria-label="Timeline bookmarks"] [aria-pressed="true"]')?.textContent)
            .toContain('Turn finish · 376 ms');
    });
    it('discovers animated objects and changes the preview without a Scene selection', async () => {
        const {fixture, selected, entities, inspect, open, workspace, frame} = setup();
        selected.set(null);
        entities.set([]);
        await fixture.whenStable();
        expect(fixture.nativeElement.textContent).toContain('Add an Animation component to a visible GLB');
        expect(inspect).not.toHaveBeenCalled();
        entities.set([actor, {...actor, key: 'archer', name: 'Archer'}]);
        await fixture.whenStable();
        expect(workspace.ready()).toBe(true);
        expect(inspect).toHaveBeenCalledWith(settings.source.asset_id, 1, 4);
        expect(open).toHaveBeenCalledWith({
            document: {kind: 'scene', scene_id: 'arena'}, entity_key: 'actor', component_id: 'animation-component',
        }, 0, 1, 4, 11, expect.any(Number));
        await pickAnimationOption(fixture, 'Animated object', 'Archer');
        expect(workspace.preview()?.target.entity_key).toBe('archer');
        expect(selected()).toBeNull();
        await pickAnimationOption(fixture, 'Imported GLB clip', 'Walk');
        expect(workspace.preview()?.clip_index).toBe(1);
        expect(settings.playback).toEqual({kind: 'single_clip', clip_index: 0, looping: true});
        const buttons = fixture.nativeElement.querySelectorAll('button') as NodeListOf<HTMLButtonElement>;
        Array.from(buttons).find(button => button.textContent?.includes('Frame Animated Object'))?.click();
        expect(frame).toHaveBeenCalledWith(workspace.preview()?.token);
    });

    it('explains when the imported source contains no previewable clips', async () => {
        const {fixture, inspect, open, workspace} = setup();
        inspect.mockResolvedValueOnce({
            workspace_generation: 11, revision: 4,
            metadata: {source: settings.source, mesh_index: 0, nodes: 2, joints: 2, clips: []},
        });
        await fixture.whenStable();
        expect(fixture.nativeElement.textContent).toContain('This object has no imported GLB clips.');
        expect(workspace.error()).toBeNull();
        expect(open).not.toHaveBeenCalled();
    });

    it('inspects immutable imported keys and scrubs to a listed time', async () => {
        const {fixture, workspace} = setup();
        await fixture.whenStable();
        const root = fixture.nativeElement as HTMLElement;
        const button = (label: string) => Array.from(root.querySelectorAll('button'))
            .find(button => button.textContent?.trim() === label)!;
        button('Inspect Imported Keys').click();
        await fixture.whenStable();
        expect(root.textContent).toContain('Node 2 · Rotation · Linear');
        button('0.750 s').click();
        await fixture.whenStable();
        expect(workspace.seconds()).toBe(0.75);
        expect(button('Next Keys').disabled).toBe(true);
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

    it('binds distinct scene instance owners and reopens an Animation preview after Play with a fresh lease', async () => {
        const {fixture, selected, entities, revision, open, workspace, views, playing, close} = setup(true);
        views.selectView('animation-workspace');
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
        expect(views.activeViewKind()).toBe('game-preview');
        expect(workspace.preview()).toBeNull();
        expect(close).toHaveBeenCalledWith(active.token);
        expect(open).toHaveBeenCalledTimes(2);
        revision.set(6);
        playing.set(false);
        await fixture.whenStable();
        expect(views.activeViewKind()).toBe('animation-workspace');
        expect(views.animationOrigin()).toBe('scene');
        const resumed = workspace.preview();
        expect(resumed?.revision).toBe(6);
        expect(resumed?.target.entity_key).toBe('member-2');
        expect(resumed?.token).not.toBe(active.token);
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
        expect(fixture.nativeElement.querySelector('input[role="combobox"]')?.value).toBe('Performer › Knight');
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
    it('does not open an audio-linked clip after its selected object changes during review', async () => {
        const {fixture, workspace, views, selected, openAuthoredClip} = setup(true);
        await fixture.whenStable();
        let resolveReview!: (allowed: boolean) => void;
        workspace.registerDraftReview(() => new Promise(resolve => {
            resolveReview = resolve;
        }));
        const opening = workspace.revealAuthoredClip('audio-event-clip', 'marker');
        selected.set({...actor, key: 'other'});
        resolveReview(true);
        await opening;
        expect(views.activeViewKind()).toBe('scene-viewport');
        expect(openAuthoredClip).not.toHaveBeenCalled();
    });

    it('opens the saved asset from another pane without an animated object selection', async () => {
        const {fixture, selected, saveAuthoredClip, workspace, views, openClipTimelineDraft} = setup(true);
        selected.set(null);
        const saved = await saveAuthoredClip('clips/turn.clip.json', {
            schema_version: 1, name: 'Camera Turn', owner: {kind: 'scene', scene_id: 'arena'},
            duration_ticks: 1000, tracks: [],
        });
        expect(await workspace.openAuthoredAsset(saved.asset.asset_id)).toBe(true);
        await fixture.whenStable();
        expect(views.activeViewKind()).toBe('animation-workspace');
        expect(workspace.authoredTimelineOpen()).toBe(true);
        expect(openClipTimelineDraft).toHaveBeenCalledWith(saved.asset.asset_id, 1, 4);
        const root: HTMLElement = fixture.nativeElement;
        expect(root.querySelector('[aria-label="Clip timeline"] h3')?.textContent).toContain('Camera Turn');
        expect(saveAuthoredClip).toHaveBeenCalledTimes(1);

        expect(await workspace.openAuthoredAsset(saved.asset.asset_id)).toBe(true);
        await fixture.whenStable();
        expect(openClipTimelineDraft).toHaveBeenCalledTimes(2);
        expect(root.querySelector('[aria-label="Clip timeline"] h3')?.textContent).toContain('Camera Turn');
    });

    it('keeps the Timeline open when its preview fails and resets the layout when switching libraries', async () => {
        const {fixture, openAuthoredClipPreview, saveAuthoredClip, workspace} = setup(true);
        const saved = await saveAuthoredClip('clips/turn.clip.json', {
            schema_version: 1, name: 'Camera Turn', owner: {kind: 'scene', scene_id: 'arena'},
            duration_ticks: 1000, tracks: [],
        });
        openAuthoredClipPreview.mockRejectedValueOnce(new Error('Object unavailable'));
        expect(await workspace.openAuthoredAsset(saved.asset.asset_id)).toBe(true);
        await fixture.whenStable();

        const root: HTMLElement = fixture.nativeElement;
        expect(workspace.authoredTimelineOpen()).toBe(true);
        expect(workspace.authoredTimelinePreviewError()).toContain('Object unavailable');
        expect(root.querySelector('[aria-label="Clip timeline"] h3')?.textContent).toContain('Camera Turn');
        expect(root.querySelector('[aria-label="Clip timeline"] [role="alert"]')?.textContent).toContain('Object unavailable');

        await workspace.selectLibrary('imported');
        await fixture.whenStable();
        expect(workspace.authoredTimelineOpen()).toBe(false);
        expect(workspace.authoredTimelinePreviewError()).toBeNull();
        expect(root.querySelector('[aria-label="Clip timeline"]')).toBeNull();
    });

    it('rejects a saved clip response from a replaced project before opening a Timeline', async () => {
        const {
            fixture,
            workspace,
            generation,
            views,
            openAuthoredClip,
            openClipTimelineDraft,
            saveAuthoredClip
        } = setup(true);
        const saved = await saveAuthoredClip('clips/turn.clip.json', {
            schema_version: 1, name: 'Camera Turn', owner: {kind: 'scene', scene_id: 'arena'},
            duration_ticks: 1000, tracks: [],
        });
        let finish!: (document: ClipAssetDocument) => void;
        openAuthoredClip.mockImplementationOnce(() => new Promise(resolve => {
            finish = resolve;
        }));
        const opening = workspace.openAuthoredAsset(saved.asset.asset_id);
        await vi.waitFor(() => expect(openAuthoredClip).toHaveBeenCalledWith(saved.asset.asset_id, 1));
        generation.set(2);
        finish(saved);
        expect(await opening).toBe(false);
        await fixture.whenStable();
        expect(views.activeViewKind()).toBe('scene-viewport');
        expect(openClipTimelineDraft).not.toHaveBeenCalled();
    });

    it('retains or discards a new clip draft before opening a different saved asset', async () => {
        const prototype = HTMLDialogElement.prototype;
        const show = Object.getOwnPropertyDescriptor(prototype, 'showModal');
        const close = Object.getOwnPropertyDescriptor(prototype, 'close');
        Object.defineProperty(prototype, 'showModal', {
            configurable: true, value(this: HTMLDialogElement) {
                this.setAttribute('open', '');
            }
        });
        Object.defineProperty(prototype, 'close', {
            configurable: true, value(this: HTMLDialogElement) {
                this.removeAttribute('open');
                this.dispatchEvent(new Event('close'));
            }
        });
        try {
            const {
                fixture,
                entities,
                saveAuthoredClip,
                workspace,
                views,
                openAuthoredClip,
                openClipTimelineDraft,
                library
            } =
                setup(true);
            const saved = await saveAuthoredClip('clips/turn.clip.json', {
                schema_version: 1, name: 'Camera Turn', owner: {kind: 'scene', scene_id: 'arena'},
                duration_ticks: 1000, tracks: [],
            });
            entities.set([{...actor, key: 'pivot', renderable: false}, {
                ...actor, key: 'lever', name: 'Lever', animation: null, parent: 'pivot',
            }]);
            views.selectView('animation-workspace');
            await fixture.whenStable();
            const root: HTMLElement = fixture.nativeElement;
            const button = (name: string, target: ParentNode = root) => Array.from(target.querySelectorAll('button'))
                .find(item => (item.getAttribute('aria-label') ?? item.textContent)?.trim() === name)!;
            button('Authored Clips').click();
            await fixture.whenStable();
            await pickAnimationOption(fixture, 'New clip object', 'Lever');
            button('New Visual Clip').click();
            await fixture.whenStable();
            const name = Array.from(root.querySelectorAll('label'))
                .find(item => item.textContent?.trim() === 'Clip name')!.querySelector('input')!;
            name.value = 'In-progress clip';
            name.dispatchEvent(new Event('input'));
            await fixture.whenStable();

            const canceled = workspace.openAuthoredAsset(saved.asset.asset_id);
            await fixture.whenStable();
            const dialog = root.querySelector<HTMLDialogElement>('dialog[open]')!;
            expect(dialog.textContent).toContain('Unsaved clip changes');
            button('Keep Editing', dialog).click();
            await fixture.whenStable();
            expect(await canceled).toBe(false);
            expect(name.value).toBe('In-progress clip');
            expect(openAuthoredClip).not.toHaveBeenCalled();

            const opened = workspace.openAuthoredAsset(saved.asset.asset_id);
            await fixture.whenStable();
            button('Discard Changes', dialog).click();
            expect(await opened).toBe(true);
            await fixture.whenStable();
            expect(root.querySelector('[aria-label="Clip timeline"] h3')?.textContent).toContain('Camera Turn');
            expect(openClipTimelineDraft).toHaveBeenCalledExactlyOnceWith(saved.asset.asset_id, 1, 4);
            expect(saveAuthoredClip).toHaveBeenCalledTimes(1);
            expect([...library.keys()]).toEqual([saved.asset.asset_id]);
        } finally {
            if (show) Object.defineProperty(prototype, 'showModal', show);
            else Reflect.deleteProperty(prototype, 'showModal');
            if (close) Object.defineProperty(prototype, 'close', close);
            else Reflect.deleteProperty(prototype, 'close');
        }
    });

    it('creates a clip, opens its saved Timeline, and replaces the same asset when editing clip settings', async () => {
        const {
            fixture, selected, entities, views, workspace, saveAuthoredClip, openAuthoredClip, openClipTimelineDraft,
            sampleAuthoredClipPreview, closeAuthoredClipPreview, library
        } = setup(true);
        views.selectView('animation-workspace');
        const lever: SceneEntity = {...actor, name: 'Lever', key: 'lever', animation: null, parent: 'pivot'};
        selected.set(null);
        entities.set([{...actor, key: 'pivot', renderable: false}, {...lever, key: 'pulley', name: 'Pulley'}, lever]);
        await fixture.whenStable();
        const root = fixture.nativeElement as HTMLElement;
        const button = (name: string) => Array.from(root.querySelectorAll('button'))
            .find(item => item.textContent?.trim() === name)!;
        button('Authored Clips').click();
        await fixture.whenStable();
        await pickAnimationOption(fixture, 'New clip object', 'Lever');
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
        }), null, {workspaceGeneration: 1, nativeGeneration: 11, revision: 4});
        expect(saveAuthoredClip.mock.calls[0][1].tracks[0]).toEqual({
            target: {kind: 'scene', entity_key: 'lever'},
            track: {kind: 'local_position', interpolation: 'linear', keys: [
                {at: 0, value: [0, 0, 0]}, {at: 1000, value: [4, 0, 0]},
            ]},
        });
        await pickAnimationOption(fixture, 'Saved visual clips', 'lever-turn.clip.json');
        expect(openAuthoredClip).toHaveBeenCalledWith('authored-clip-id', 1);
        expect(openClipTimelineDraft).toHaveBeenCalledExactlyOnceWith('authored-clip-id', 1, 4);
        expect(root.querySelector('[aria-label="Clip timeline"] h3')?.textContent).toContain('Lever Turn');
        expect(root.querySelector('.clip-file')?.textContent).toContain('lever-turn.clip.json');
        const trackInput = root.querySelector<HTMLInputElement>('.track-choice [role="combobox"]')!;
        expect(trackInput.value).toBe('Position · Lever');
        await pickAnimationOption(fixture, 'Track', 'Rotation · Lever');
        expect(trackInput.value).toBe('Rotation · Lever');
        expect(root.querySelector('input[aria-label="End position X"]')).toBeNull();
        expect(root.textContent).not.toContain('New clip object');
        expect(saveAuthoredClip).toHaveBeenCalledTimes(1);

        button('Clip Settings…').click();
        await fixture.whenStable();
        const savedEnd = root.querySelector<HTMLInputElement>('input[aria-label="End position X"]')!;
        expect(savedEnd.value).toBe('4');
        savedEnd.value = '8';
        savedEnd.dispatchEvent(new Event('input'));
        button('Save Clip').click();
        await fixture.whenStable();
        expect(saveAuthoredClip).toHaveBeenLastCalledWith('lever-turn.clip.json', expect.objectContaining({
            name: 'Lever Turn',
        }), {asset_id: 'authored-clip-id', fingerprint: 'e'.repeat(64)}, {
            workspaceGeneration: 1, nativeGeneration: 11, revision: 4,
        });
        expect([...library.keys()]).toEqual(['authored-clip-id']);
        expect(library.get('authored-clip-id')?.asset.revision).toBe(2);

        await pickAnimationOption(fixture, 'Saved visual clips', 'lever-turn.clip.json');
        expect(openClipTimelineDraft).toHaveBeenCalledTimes(2);
        expect(root.querySelector('[aria-label="Clip timeline"] h3')?.textContent).toContain('Lever Turn');
        button('Close Timeline').click();
        await fixture.whenStable();
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
