import {computed, effect, inject, Injectable, signal, untracked} from '@angular/core';
import {EditorService} from '../../../../core/editor/editor.service';
import type {ViewportView} from '../../../../core/editor/viewport-editor.models';
import {ViewportEditorService} from '../../../../core/editor/viewport-editor.service';
import type {WorkspaceViewKind} from '../../../../core/editor/workspace/workspace-view';

interface AuthoringAvailability {
    readonly prop: () => boolean;
    readonly animator: () => boolean;
    readonly controlsGraph: () => boolean;
}

const PRIMARY_VIEWS = ['scene-viewport', 'game-preview'] as const;

/** Coordinates workspace tabs with the native viewport and the current play run. */
@Injectable()
export class WorkspaceViewService {
    readonly gameView = computed(() => this.activeViewKind() === 'game-preview');
    readonly propView = computed(() => this.activeViewKind() === 'prop-workspace');
    readonly controlsGraphView = computed(() => this.activeViewKind() === 'controls-graph');
    readonly animatorView = computed(() => this.activeViewKind() === 'animation-controller');
    readonly animationView = computed(() => this.activeViewKind() === 'animation-workspace');
    private readonly animationScope = signal<'scene' | 'prop'>('scene');
    readonly animationOrigin = this.animationScope.asReadonly();
    private readonly editor = inject(EditorService);
    private readonly viewport = inject(ViewportEditorService);
    readonly gameStatus = this.viewport.gameStatus;
    readonly restartAvailable = computed(() =>
        this.gameView() && this.gameStatus() !== null && this.editor.viewport()?.attached === true &&
        !this.editor.busy() && this.viewport.pending() === 0
    );
    readonly gameKeyboardCaptured = computed(() => {
        const state = this.viewport.state();
        return state?.settings.view === 'game' && state.focused;
    });
    private readonly authoringAvailability = signal<AuthoringAvailability | null>(null);
    readonly availableViews = computed<readonly WorkspaceViewKind[]>(() => {
        if (this.editor.playing() || !this.editor.snapshot()) return PRIMARY_VIEWS;
        const authoring = this.authoringAvailability();
        const views: WorkspaceViewKind[] = [...PRIMARY_VIEWS];
        if (authoring?.prop()) views.push('prop-workspace');
        views.push('animation-workspace');
        if (authoring?.animator()) views.push('animation-controller');
        if (authoring?.controlsGraph()) views.push('controls-graph');
        return views;
    });
    private readonly activeView = signal<WorkspaceViewKind>('scene-viewport');
    readonly activeViewKind = computed(() => {
        const selected = this.activeView();
        if (this.availableViews().includes(selected)) return selected;
        return this.editor.playing() ? 'game-preview' : 'scene-viewport';
    });
    private readonly restartFailure = signal<{ runId: string; generation: number; message: string } | null>(null);
    readonly gameError = computed(() => {
        const status = this.gameStatus();
        if (!status) return null;
        const failure = this.restartFailure();
        return failure?.runId === status.run_id && failure.generation === this.editor.workspaceGeneration()
            ? failure.message : this.viewport.state()?.error ?? null;
    });
    private requestedView: ViewportView | null = null;
    private returnFromPlay: WorkspaceViewKind = 'scene-viewport';
    private wasPlaying = false;

    constructor() {
        effect(() => {
            const playing = this.editor.playing();
            const available = this.availableViews();
            untracked(() => {
                const previous = this.activeView();
                if (playing && !this.wasPlaying) {
                    this.returnFromPlay = previous === 'game-preview' ? 'scene-viewport' : previous;
                    this.activeView.set('game-preview');
                } else {
                    const intended = this.wasPlaying && !playing ? this.returnFromPlay : previous;
                    this.activeView.set(available.includes(intended) ? intended : 'scene-viewport');
                }
                this.wasPlaying = playing;
            });
        });
        effect(() => {
            const state = this.viewport.state();
            if (!state || this.propView() || this.animationView() || this.animatorView() || this.controlsGraphView()) return;
            const view = this.gameView() ? 'game' : 'scene';
            if (state.settings.view === view || this.requestedView === view) return;
            this.requestedView = view;
            void this.viewport.action({type: 'set_view', view}).catch(() => undefined).finally(() => {
                if (this.requestedView === view) this.requestedView = null;
            });
        });
    }

    setAuthoringAvailability(availability: AuthoringAvailability): void {
        this.authoringAvailability.set(availability);
    }

    selectView(kind: WorkspaceViewKind): void {
        if (!this.availableViews().includes(kind)) return;
        if (kind === 'animation-workspace' && !this.animationView()) {
            this.animationScope.set(this.propView() ? 'prop' : 'scene');
        }
        this.activeView.set(kind);
    }

    closePropView(): void {
        if (this.returnFromPlay === 'prop-workspace') this.returnFromPlay = 'scene-viewport';
        if (this.propView()) this.activeView.set('scene-viewport');
    }

    async restartRun(blocked: boolean): Promise<void> {
        const snapshot = this.editor.snapshot();
        const status = this.gameStatus();
        if (blocked || !snapshot || !status || !this.restartAvailable()) return;
        const generation = this.editor.workspaceGeneration();
        this.restartFailure.set(null);
        try {
            await this.viewport.action({
                type: 'restart_run', run_id: status.run_id, expected_revision: snapshot.revision,
            });
        } catch (error) {
            if (this.editor.workspaceGeneration() === generation && this.gameStatus()?.run_id === status.run_id) {
                this.restartFailure.set({
                    runId: status.run_id,
                    generation,
                    message: error instanceof Error ? error.message : String(error),
                });
            }
        }
    }
}
