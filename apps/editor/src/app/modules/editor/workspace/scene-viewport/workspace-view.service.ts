import {computed, effect, inject, Injectable, signal, untracked} from '@angular/core';
import {EditorService} from '../../../../core/editor/editor.service';
import type {ViewportView} from '../../../../core/editor/viewport-editor.models';
import {ViewportEditorService} from '../../../../core/editor/viewport-editor.service';
import type {WorkspaceViewKind} from '../../../../core/editor/workspace/workspace-view';

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
    private readonly activeView = signal<WorkspaceViewKind>('scene-viewport');
    readonly activeViewKind = this.activeView.asReadonly();
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
            untracked(() => {
                if (playing && !this.wasPlaying) {
                    const previous = this.activeViewKind();
                    this.returnFromPlay = previous === 'game-preview' ? 'scene-viewport' : previous;
                    this.activeView.set('game-preview');
                } else if (!playing && this.wasPlaying) {
                    this.activeView.set(this.returnFromPlay);
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

    selectView(kind: WorkspaceViewKind): void {
        if ((kind === 'prop-workspace' || kind === 'animation-workspace' || kind === 'animation-controller' || kind === 'controls-graph') && this.editor.playing()) return;
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
