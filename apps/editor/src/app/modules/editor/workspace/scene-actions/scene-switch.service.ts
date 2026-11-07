import {computed, DestroyRef, inject, Injectable, signal} from '@angular/core';
import type {EditorSnapshot, SceneId} from '../../../../core/editor/editor.models';
import {EditorService} from '../../../../core/editor/editor.service';
import {SceneEditingService} from '../scene-editing.service';

interface SceneDestination {
    readonly sceneId: SceneId;
    readonly path: string;
}

interface SceneConfirmation {
    readonly fromPath: string;
    readonly toPath: string;
}

interface CompletedSwitch extends SceneDestination {
    readonly sceneRevision: number;
    readonly generation: number;
}

type SceneChoice = 'save' | 'keep' | 'cancel';

/** One scene-opening flow for the toolbar, asset browser, and viewport drops. */
@Injectable()
export class SceneSwitchService {
    private readonly editor = inject(EditorService);
    private readonly editing = inject(SceneEditingService, {optional: true});
    private readonly pendingState = signal(false);
    readonly pending = this.pendingState.asReadonly();
    readonly canSwitch = computed(() => {
        const snapshot = this.editor.snapshot();
        return this.editor.nativeAvailable && !this.pending() && snapshot?.mode === 'edit' &&
            (this.editing?.ready() ?? (!this.editor.busy() && snapshot.transaction === null));
    });
    private readonly loadingState = signal<SceneDestination | null>(null);
    readonly loading = this.loadingState.asReadonly();
    private readonly confirmationState = signal<SceneConfirmation | null>(null);
    readonly confirmation = this.confirmationState.asReadonly();
    private readonly completedState = signal<CompletedSwitch | null>(null);
    readonly lastSwitch = this.completedState.asReadonly();
    private readonly errorState = signal<string | null>(null);
    readonly error = this.errorState.asReadonly();
    private readonly messageState = signal<string | null>(null);
    readonly message = this.messageState.asReadonly();
    private answer: ((choice: SceneChoice) => void) | undefined;
    private destroyed = false;

    constructor() {
        inject(DestroyRef).onDestroy(() => {
            this.destroyed = true;
            this.choose('cancel');
        });
    }

    async request(sceneId: SceneId): Promise<boolean> {
        const snapshot = this.editor.snapshot();
        if (this.destroyed || !this.canSwitch() || !snapshot || snapshot.active_scene_id === sceneId) return false;
        const generation = this.editor.workspaceGeneration();
        const fromId = snapshot.active_scene_id;
        this.pendingState.set(true);
        this.clearFeedback();

        let switched = false;
        const open = async () => {
            const previous = this.currentFor(generation, fromId);
            const from = previous.scenes.find(scene => scene.id === fromId);
            const to = previous.scenes.find(scene => scene.id === sceneId);
            if (!from || !to) throw new Error('The scene is no longer registered. Choose a scene again.');

            let kept = false;
            if (from.dirty) {
                const decision = await this.confirm(from.asset_path, to.asset_path);
                if (decision === 'cancel') return;
                if (this.currentFor(generation, fromId).revision !== previous.revision) {
                    throw new Error('The scene changed while the confirmation was open. Try again.');
                }
                if (decision === 'save') {
                    await this.editor.save();
                    this.currentFor(generation, fromId);
                } else kept = true;
            }

            const destination = {sceneId, path: to.asset_path};
            this.loadingState.set(destination);
            const opened = await this.editor.switchScene(sceneId);
            if (this.destroyed || this.editor.workspaceGeneration() !== generation) return;
            this.completedState.set({...destination, sceneRevision: opened.scene_revision, generation});
            this.messageState.set(kept
                ? `Opened ${destination.path}. Edits to ${from.asset_path} are kept in this session.`
                : `Opened ${destination.path}.`);
            switched = true;
        };

        try {
            if (this.editing) {
                if (!await this.editing.withCommittedEdit(open)) {
                    this.errorState.set(this.editing.error() ?? 'Finish the current edit and try again.');
                }
            } else await open();
        } catch (error) {
            this.errorState.set(error instanceof Error ? error.message : String(error));
        } finally {
            this.loadingState.set(null);
            this.pendingState.set(false);
        }
        return switched;
    }

    choose(choice: SceneChoice): void {
        const answer = this.answer;
        if (!answer) return;
        this.answer = undefined;
        this.confirmationState.set(null);
        answer(choice);
    }

    clearFeedback(): void {
        this.errorState.set(null);
        this.messageState.set(null);
    }

    private confirm(fromPath: string, toPath: string): Promise<SceneChoice> {
        return new Promise(resolve => {
            this.answer = resolve;
            this.confirmationState.set({fromPath, toPath});
        });
    }

    private currentFor(generation: number, sceneId: SceneId): EditorSnapshot {
        const snapshot = this.editor.snapshot();
        if (this.destroyed || this.editor.workspaceGeneration() !== generation || snapshot?.mode !== 'edit' ||
            snapshot.active_scene_id !== sceneId || snapshot.transaction !== null || this.editor.busy()) {
            throw new Error('The editor state changed. Try opening the scene again.');
        }
        return snapshot;
    }
}
