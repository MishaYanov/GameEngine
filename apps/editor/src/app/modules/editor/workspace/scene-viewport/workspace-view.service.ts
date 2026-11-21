import {computed, effect, inject, Injectable, signal} from '@angular/core';
import {EditorService} from '../../../../core/editor/editor.service';
import type {ViewportView} from '../../../../core/editor/viewport-editor.models';
import {ViewportEditorService} from '../../../../core/editor/viewport-editor.service';
import type {WorkspaceViewKind} from '../../../../core/editor/workspace/workspace-view';

/** Coordinates workspace tabs with the native viewport and the current play run. */
@Injectable()
export class WorkspaceViewService {
    readonly gameView = computed(() => this.activeViewKind() === 'game-preview');
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

    constructor() {
        effect(() => this.activeView.set(this.editor.playing() ? 'game-preview' : 'scene-viewport'));
        effect(() => {
            const state = this.viewport.state();
            const view = this.gameView() ? 'game' : 'scene';
            if (!state || state.settings.view === view || this.requestedView === view) return;
            this.requestedView = view;
            void this.viewport.action({type: 'set_view', view}).catch(() => undefined).finally(() => {
                if (this.requestedView === view) this.requestedView = null;
            });
        });
    }

    selectView(kind: WorkspaceViewKind): void {
        this.activeView.set(kind);
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
