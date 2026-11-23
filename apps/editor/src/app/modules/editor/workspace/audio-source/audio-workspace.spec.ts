import {signal} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import {vi} from 'vitest';
import type {ComponentRecord} from '../../../../core/editor/editor-components';
import {EditorService} from '../../../../core/editor/editor.service';
import {ViewportEditorService} from '../../../../core/editor/viewport-editor.service';
import {AnimationWorkspaceService} from '../animation/animation-workspace.service';
import {ComponentWorkspaceSession} from '../components/component-workspace-session';
import type {InspectorSubject} from '../inspector-panel/inspector-subject';
import {PropPreviewService} from '../prop-workspace/prop-preview.service';
import {PropWorkspaceService} from '../prop-workspace/prop-workspace.service';
import {SceneEditingService} from '../scene-editing.service';
import {WorkspaceViewService} from '../scene-viewport/workspace-view.service';
import {ScopedPaneRequests} from '../scoped-pane-request';
import {WorkspaceDraftReview} from '../workspace-draft-review';
import {AudioAuditionService} from './audio-audition.service';
import {AudioWorkspace} from './audio-workspace';

const clip = 'c2a281ca-df03-4431-a23b-2a1e982293ae';
const sceneSubject: InspectorSubject = {
    address: {kind: 'scene', scene: 'courtyard', object: 'guard'},
    scope: {workspaceGeneration: 1, nativeGeneration: 23, revision: 6},
    contentRevision: 4,
    access: 'scene-edit',
};
const propSubject: InspectorSubject = {
    ...sceneSubject, access: 'prop-edit',
    address: {kind: 'prop', draftId: 'guard-prop', sourceObject: 'guard'},
};
const music: ComponentRecord = {schema_version: 1, id: 'background-audio', enabled: true, data: {
    kind: 'audio_source', settings: {
        clip: {asset: clip}, gain: .5, loop: false, playback: 'on_event', bus: 'music',
        spatial: {mode: 'spatial', near: 1, far: 12},
        event: {kind: 'animation_marker', clip: 'movement', marker: 'step-left'},
    },
}};
const effects: ComponentRecord = {schema_version: 1, id: 'effects-audio', enabled: true, data: {
    kind: 'audio_source', settings: {
        clip: null, gain: 1, loop: false, playback: 'on_scene_enter', bus: 'effects',
    },
}};

function setup() {
    const workspaceGeneration = signal(1);
    const playing = signal(false);
    const snapshot = signal({active_scene_id: 'courtyard', mode: 'edit', transaction: null, revision: 6, scene_revision: 4});
    const selected = signal({key: 'guard', name: 'Guard', component_views: [music, effects]});
    const propSelection = signal({key: 'guard', name: 'Guard Prop', components: [music, effects]});
    const execute = vi.fn(async () => snapshot());
    const activeViewKind = signal('scene-viewport');
    const selectView = vi.fn((view: string) => activeViewKind.set(view));
    const openMarker = vi.fn(async () => true);
    const editComponent = vi.fn(async () => true);
    const audition = {isPlaying: vi.fn(() => false), play: vi.fn(), stop: vi.fn()};
    TestBed.configureTestingModule({providers: [
        ComponentWorkspaceSession, ScopedPaneRequests, WorkspaceDraftReview,
        {provide: EditorService, useValue: {workspaceGeneration, snapshot, playing, execute, busy: signal(false),
            modelAssets: signal([{asset_id: clip, path: 'audio/steps.wav', name: 'Steps', kind: 'audio_clip'}]),
            authoredClipCatalogVersion: signal(0), listAuthoredClips: async () => [],
            openAuthoredClip: async () => ({asset: {asset_id: 'movement'}, clip: {markers: []}}),
        }},
        {provide: ViewportEditorService, useValue: {state: signal({workspace_generation: 23})}},
        {provide: SceneEditingService, useValue: {selectedEntity: selected, editable: signal(true),
            withCommittedEdit: async (action: () => Promise<void>) => { await action(); return true; }}},
        {provide: WorkspaceViewService, useValue: {activeViewKind, selectView, audioView: () => activeViewKind() === 'audio-workspace'}},
        {provide: PropWorkspaceService, useValue: {inspectorSubject: signal(propSubject), selectedObject: propSelection,
            canEditDraft: signal(true), editComponent}},
        {provide: PropPreviewService, useValue: {ready: signal(true),
            currentState: signal({selection: 'guard', workspace_generation: 23, token: 'preview-guard'})}},
        {provide: AnimationWorkspaceService, useValue: {openScoped: openMarker}},
        {provide: AudioAuditionService, useValue: audition},
    ]});
    return {session: TestBed.inject(ComponentWorkspaceSession), reviews: TestBed.inject(WorkspaceDraftReview),
        selectView, selected, playing, openMarker, editComponent, execute, audition};
}

describe('Audio workspace', () => {
    it('selects the requested source, shows its spatial setup, and sends the exact saved marker to Animation', async () => {
        const {session, openMarker, audition} = setup();
        expect(await session.open('audio-workspace', sceneSubject, music.id)).toBe(true);
        const fixture = TestBed.createComponent(AudioWorkspace);
        fixture.detectChanges();
        await fixture.whenStable();
        expect(fixture.nativeElement.textContent).toContain('audio/steps.wav');
        expect(fixture.nativeElement.textContent).toContain('Far distance');
        const buttons = Array.from(fixture.nativeElement.querySelectorAll('button') as NodeListOf<HTMLButtonElement>);
        buttons.find(button => button.textContent?.includes('Open Saved Animation Marker'))?.click();
        expect(openMarker).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({
            library: 'authored', asset: {id: 'movement'}, markerId: 'step-left', componentId: music.id,
        }), expect.any(Function));
        buttons.find(button => button.textContent?.includes('Preview Audio'))?.click();
        expect(audition.play).toHaveBeenCalledExactlyOnceWith(expect.any(Symbol), expect.objectContaining({
            owner: {kind: 'scene', scene_id: 'courtyard', key: 'guard'},
        }), music.id, .5, expect.any(Function));
        audition.stop.mockClear();
        fixture.destroy();
        expect(audition.stop).toHaveBeenCalledOnce();
    });

    it('keeps the original pane on a cancelled review or a changed selection', async () => {
        const {session, reviews, selectView, selected} = setup();
        reviews.setReviewers([() => false]);
        expect(await session.open('audio-workspace', sceneSubject, effects.id)).toBe(false);
        reviews.setReviewers([() => {
            selected.set({key: 'other', name: 'Other', component_views: [effects]});
            return true;
        }]);
        expect(await session.open('audio-workspace', sceneSubject, effects.id)).toBe(false);
        expect(selectView).not.toHaveBeenCalled();
    });

    it('writes only the selected source Prop component and disables audition in Play', async () => {
        const {session, editComponent, execute, playing} = setup();
        expect(await session.open('audio-workspace', propSubject, effects.id)).toBe(true);
        expect(session.audioScope()?.owner).toEqual({kind: 'prop', key: 'guard', preview_token: 'preview-guard'});
        const intent = {kind: 'replace', component: {...effects, enabled: false}, expected: JSON.stringify(effects)} as const;
        expect(await session.apply({...intent, component: music})).toBe(false);
        expect(await session.apply(intent)).toBe(true);
        expect(editComponent).toHaveBeenCalledExactlyOnceWith('guard', intent);
        expect(execute).not.toHaveBeenCalled();
        playing.set(true);
        expect(session.audioScope()).toBeNull();
        expect(await session.apply(intent)).toBe(false);
    });
});
