import {computed, DestroyRef, inject, Injectable, signal} from '@angular/core';
import type {EditorSnapshot, SceneId} from '../../../../core/editor/editor.models';
import {EditorService, type WorkspaceAssetScope} from '../../../../core/editor/editor.service';
import {SceneEditingService} from '../scene-editing.service';
import {type WorkspaceDraftTarget, WorkspaceDraftReview} from '../workspace-draft-review';
import {WorkspaceNavigation} from '../workspace-navigation';

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
type SceneTarget = { readonly sceneId: SceneId } | { readonly createPath: string };
type SceneNavigation = 'opened' | 'registered' | 'stayed';

/** One scene-opening flow for the toolbar, asset browser, and viewport drops. */
@Injectable()
export class SceneSwitchService {
    private readonly editor = inject(EditorService);
    private readonly drafts = inject(WorkspaceDraftReview);
    private readonly history = inject(WorkspaceNavigation, {optional: true});
    private readonly editing = inject(SceneEditingService, {optional: true});
    private readonly pendingState = signal(false);
    readonly pending = this.pendingState.asReadonly();
    readonly canSwitch = computed(() => {
        const snapshot = this.editor.snapshot();
        return this.editor.nativeAvailable && !this.pending() && !this.drafts.pending() && snapshot?.mode === 'edit' &&
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

    async request(sceneId: SceneId, recordNavigation = true): Promise<boolean> {
        if (this.editor.snapshot()?.active_scene_id === sceneId) return false;
        return await this.navigate({sceneId}, recordNavigation) === 'opened';
    }

    createAndOpen(createPath: string): Promise<SceneNavigation> {
        return this.navigate({createPath});
    }

    private async navigate(target: SceneTarget, recordNavigation = true): Promise<SceneNavigation> {
        if (this.destroyed || !this.canSwitch()) return 'stayed';
        const scope = this.drafts.capture();
        this.pendingState.set(true);
        this.clearFeedback();
        let registeredPath: string | null = null;
        let switched = false;
        const report = (message: string) => {
            if (this.destroyed || this.editor.workspaceGeneration() !== scope.generation) return;
            this.errorState.set(registeredPath
                ? `${message} Created ${registeredPath}. Open it from Assets → Scenes and use Save All to save the project.`
                : message);
        };

        const current = () => this.currentFor(scope);
        const readCreateScope = async (revision: number, original?: WorkspaceAssetScope) => {
            const native = await this.editor.authoringScope(scope.generation);
            if (native.workspaceGeneration !== scope.generation ||
                (original && native.nativeGeneration !== original.nativeGeneration)) {
                throw new Error('The native project changed. Choose Create Scene again.');
            }
            if (native.revision !== revision || current().revision !== revision) {
                throw new Error('The project changed while preparing the new scene. Choose Create Scene again.');
            }
            return native;
        };

        const open = async () => {
            const previous = current();
            const from = previous.scenes.find(scene => scene.id === scope.sceneId);
            const existing = 'sceneId' in target ? previous.scenes.find(scene => scene.id === target.sceneId) : null;
            const path = 'createPath' in target ? target.createPath : existing?.asset_path;
            if (!from || !path) throw new Error('The scene is no longer registered. Choose a scene again.');

            let createScope = 'createPath' in target ? await readCreateScope(previous.revision) : null;
            let kept = false;
            if (from.dirty) {
                const decision = await this.confirm(from.asset_path, path);
                if (decision === 'cancel') return;
                if (current().revision !== previous.revision) {
                    throw new Error('The scene changed while the confirmation was open. Try again.');
                }
                if (createScope) createScope = await readCreateScope(previous.revision, createScope);
                if (decision === 'save') {
                    await this.editor.save(scope.generation, previous.revision);
                    if (createScope) createScope = await readCreateScope(current().revision, createScope);
                } else kept = true;
            }

            let sceneId: SceneId;
            if ('createPath' in target) {
                if (!createScope) throw new Error('The editor could not identify the current project.');
                const saved = await this.editor.createScene(path, createScope);
                const created = saved.scenes.find(scene => scene.asset_path === path);
                if (!created) throw new Error('The new scene was not registered. Check Assets before trying again.');
                registeredPath = path;
                sceneId = created.id;
                await readCreateScope(saved.revision, createScope);
            } else sceneId = target.sceneId;

            const destination = {sceneId, path};
            // Background change receipts can enqueue reads here. The editor queue
            // orders the switch behind them and validates this generation and revision.
            const ready = current();
            this.loadingState.set(destination);
            const opened = await this.editor.switchScene(sceneId, scope.generation, ready.revision);
            if (this.destroyed || this.editor.workspaceGeneration() !== scope.generation) return;
            if (opened.active_scene_id !== sceneId) throw new Error('The requested scene could not be opened.');
            this.completedState.set({
                ...destination,
                sceneRevision: opened.scene_revision,
                generation: scope.generation,
            });
            this.messageState.set(kept
                ? `Opened ${path}. Edits to ${from.asset_path} are kept in this session.`
                : `Opened ${path}.`);
            if (recordNavigation) this.history?.record({kind: 'scene', id: from.id}, {kind: 'scene', id: sceneId});
            switched = true;
        };

        try {
            await this.drafts.run(scope, async () => {
                if (!this.editing) await open();
                else if (!await this.editing.withCommittedEdit(open)) {
                    report(this.editing.error() ?? 'Finish the current edit and try again.');
                }
            });
        } catch (error) {
            report(error instanceof Error ? error.message : String(error));
        } finally {
            this.loadingState.set(null);
            this.pendingState.set(false);
        }
        return switched ? 'opened' : registeredPath ? 'registered' : 'stayed';
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

    private currentFor(scope: WorkspaceDraftTarget): EditorSnapshot {
        const snapshot = this.editor.snapshot();
        if (this.destroyed || !this.drafts.current(scope) || snapshot?.mode !== 'edit' ||
            snapshot.transaction !== null) {
            throw new Error('The editor state changed. Try opening the scene again.');
        }
        return snapshot;
    }
}
