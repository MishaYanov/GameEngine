import {computed, signal} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import {EditorService} from '../../../../core/editor/editor.service';
import {ViewportEditorService} from '../../../../core/editor/viewport-editor.service';
import {WorkspaceViewService} from './workspace-view.service';

function setup() {
    const mode = signal<'edit' | 'play' | null>('edit');
    const prop = signal(true);
    const animator = signal(true);
    const controlsGraph = signal(true);
    TestBed.configureTestingModule({
        providers: [
            WorkspaceViewService,
            {
                provide: EditorService,
                useValue: {
                    snapshot: computed(() => mode() === null ? null : {mode: mode()}),
                    playing: computed(() => mode() === 'play'),
                },
            },
            {provide: ViewportEditorService, useValue: {state: signal(null), gameStatus: signal(null)}},
        ],
    });
    const views = TestBed.inject(WorkspaceViewService);
    views.setAuthoringAvailability({prop, animator, controlsGraph});
    TestBed.tick();
    return {views, mode, prop, animator, controlsGraph};
}

describe('WorkspaceViewService', () => {
    it.each([
        ['animation-controller', 'animator'],
        ['controls-graph', 'controlsGraph'],
        ['prop-workspace', 'prop'],
    ] as const)('falls back to Scene when the active %s disappears', (kind, feature) => {
        const workspace = setup();
        const {views} = workspace;
        views.selectView(kind);
        expect(views.activeViewKind()).toBe(kind);
        workspace[feature].set(false);
        expect(views.availableViews()).not.toContain(kind);
        expect(views.activeViewKind()).toBe('scene-viewport');
        views.selectView(kind);
        TestBed.tick();
        expect(views.activeViewKind()).toBe('scene-viewport');
        workspace[feature].set(true);
        TestBed.tick();
        expect(views.availableViews()).toContain(kind);
        expect(views.activeViewKind()).toBe('scene-viewport');
    });

    it('clears authoring views when the active project closes', () => {
        const {views, mode} = setup();
        views.selectView('animation-workspace');
        mode.set(null);
        TestBed.tick();
        expect(views.availableViews()).toEqual(['scene-viewport', 'game-preview']);
        expect(views.activeViewKind()).toBe('scene-viewport');
        mode.set('edit');
        TestBed.tick();
        expect(views.activeViewKind()).toBe('scene-viewport');
    });

    it('returns to Scene when the pre-Play pane is no longer available on Stop', () => {
        const {views, mode, controlsGraph} = setup();
        views.selectView('controls-graph');
        mode.set('play');
        TestBed.tick();
        expect(views.activeViewKind()).toBe('game-preview');
        controlsGraph.set(false);
        mode.set('edit');
        TestBed.tick();
        expect(views.activeViewKind()).toBe('scene-viewport');
    });

    it('retains the Prop origin when a committed Play run returns to Animation', () => {
        const {views, mode} = setup();
        views.selectView('prop-workspace');
        views.selectView('animation-workspace');
        expect(views.animationOrigin()).toBe('prop');
        mode.set('play');
        TestBed.tick();
        views.selectView('animation-workspace');
        expect(views.activeViewKind()).toBe('game-preview');
        mode.set('edit');
        TestBed.tick();
        expect(views.activeViewKind()).toBe('animation-workspace');
        expect(views.animationOrigin()).toBe('prop');
    });
});
