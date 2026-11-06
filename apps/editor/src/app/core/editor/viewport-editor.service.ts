import {computed, DestroyRef, effect, inject, Injectable, signal} from '@angular/core';
import {invoke} from '@tauri-apps/api/core';
import {listen, type UnlistenFn} from '@tauri-apps/api/event';
import {EditorService} from './editor.service';
import {DEFAULT_VIEWPORT_SETTINGS, type ViewportAction, type ViewportDocumentChange, type ViewportEditorState} from './viewport-editor.models';

/** One editor workspace owns native view actions and its notification subscriptions. */
@Injectable()
export class ViewportEditorService {
    private readonly editor = inject(EditorService);
    readonly nativeAvailable = this.editor.nativeAvailable;
    private readonly current = signal<ViewportEditorState | null>(null);
    private readonly localContext = signal(this.editor.workspaceGeneration());
    readonly state = computed(() => this.localContext() === this.editor.workspaceGeneration() ? this.current() : null);
    readonly settings = computed(() => this.state()?.settings ?? DEFAULT_VIEWPORT_SETTINGS);
    readonly authoringGesture = computed(() => {
        const state = this.state();
        return state?.gesture === 'transform' || state?.gesture === 'placement' ||
            state?.gesture === 'asset_loading' || !!state?.placement;
    });
    private readonly errorState = signal<string | null>(null);
    readonly error = this.errorState.asReadonly();
    private readonly pendingState = signal(0);
    readonly pending = this.pendingState.asReadonly();
    private queue: Promise<void> = Promise.resolve();
    private startRequest: Promise<void> | null = null;
    private readonly subscriptions: UnlistenFn[] = [];
    private destroyed = false;
    private aperture: HTMLElement | null = null;
    private placementUpdate: {action: Extract<ViewportAction, {type: 'update_placement'}>; promise: Promise<ViewportEditorState>} | null = null;
    private refreshRequest: Promise<void> | null = null;
    private requestedRevision = -1;
    private requestedGeneration = -1;
    private nativeContext: number | null = null;
    private retiredNativeContext = -1;
    private futureState: ViewportEditorState | null = null;

    constructor() {
        effect(() => {this.editor.workspaceGeneration(); this.synchronizeContext();});
        inject(DestroyRef).onDestroy(() => {
            this.destroyed = true;
            for (const stop of this.subscriptions.splice(0)) this.stopListener(stop);
            this.aperture = null;
        });
    }

    start(): Promise<void> {
        if (!this.nativeAvailable || this.destroyed) return Promise.resolve();
        if (this.startRequest) return this.startRequest;
        this.startRequest = this.subscribe().catch(error => {
            this.errorState.set(this.message(error));
            this.startRequest = null;
            throw error;
        });
        return this.startRequest;
    }

    action(action: ViewportAction): Promise<ViewportEditorState> {
        if (action.type === 'update_placement') return this.updatePlacement(action.normalized);
        // Semantic boundaries cannot be replaced by later pointer positions.
        this.placementUpdate = null;
        return this.enqueue(() => action);
    }

    updatePlacement(normalized: readonly [number, number] | null): Promise<ViewportEditorState> {
        const action: Extract<ViewportAction, {type: 'update_placement'}> = {type: 'update_placement', normalized};
        if (this.placementUpdate) {
            this.placementUpdate.action = action;
            return this.placementUpdate.promise;
        }
        let update!: {action: Extract<ViewportAction, {type: 'update_placement'}>; promise: Promise<ViewportEditorState>};
        const promise = this.enqueue(() => {
            if (this.placementUpdate === update) this.placementUpdate = null;
            return update.action;
        });
        update = {action, promise};
        this.placementUpdate = update;
        return update.promise;
    }

    registerSurface(element: HTMLElement): () => void {
        this.aperture = element;
        return () => {if (this.aperture === element) this.aperture = null;};
    }

    normalizedPoint(clientX: number, clientY: number): readonly [number, number] | null {
        const bounds = this.aperture?.getBoundingClientRect();
        if (!bounds || bounds.width <= 0 || bounds.height <= 0) return null;
        const x = (clientX - bounds.left) / bounds.width;
        const y = (clientY - bounds.top) / bounds.height;
        return Number.isFinite(x) && Number.isFinite(y) && x >= 0 && x <= 1 && y >= 0 && y <= 1 ? [x, y] : null;
    }

    clearError(): void {this.errorState.set(null);}

    private enqueue(readAction: () => ViewportAction): Promise<ViewportEditorState> {
        if (!this.nativeAvailable || this.destroyed) return this.reject('Native viewport tools require the desktop editor.');
        if (this.pendingState() >= 32) return this.reject('Viewport action queue is full. Finish the current operation and try again.');
        const workspaceGeneration = this.editor.workspaceGeneration();
        const assertCurrentWorkspace = () => {
            if (workspaceGeneration !== this.editor.workspaceGeneration()) {
                throw new Error('The open project changed before the viewport action completed. Try again in the current workspace.');
            }
        };
        this.pendingState.update(count => count + 1);
        const result = this.queue.then(async () => {
            if (this.destroyed) throw new Error('The viewport workspace has closed.');
            assertCurrentWorkspace();
            await this.start();
            assertCurrentWorkspace();
            const action = readAction();
            const state = await invoke<ViewportEditorState>('editor_viewport_action', {action});
            assertCurrentWorkspace();
            this.accept(state);
            this.errorState.set(state.error);
            if (state.error) throw new Error(state.error);
            return state;
        }).catch(error => {
            this.errorState.set(this.message(error));
            throw error;
        });
        this.queue = result.then(() => undefined, () => undefined);
        return result.finally(() => this.pendingState.update(count => count - 1));
    }

    private async subscribe(): Promise<void> {
        const stops: UnlistenFn[] = [];
        try {
            stops.push(await listen<ViewportEditorState>('editor://viewport-editor-state', event => {
                if (!this.destroyed) this.accept(event.payload);
            }));
            if (this.destroyed) return;
            stops.push(await listen<ViewportDocumentChange>('editor://viewport-document-changed', event => {
                if (!this.destroyed) this.documentChanged(event.payload);
            }));
            if (this.destroyed) return;
            this.accept(await invoke<ViewportEditorState>('editor_viewport_editor_state'));
            if (!this.destroyed) this.subscriptions.push(...stops.splice(0));
        } finally {
            for (const stop of stops) this.stopListener(stop);
        }
    }

    private accept(state: ViewportEditorState): void {
        if (this.destroyed || !state?.settings) return;
        this.synchronizeContext();
        const generation = state.workspace_generation ?? -1;
        if (generation <= this.retiredNativeContext) return;
        if (this.nativeContext !== null && generation !== this.nativeContext) {
            // Native project installation can publish before Angular receives
            // its open reply. Hold one future state until the local context moves.
            const future = this.futureState;
            if (generation > this.nativeContext && (!future || generation > (future.workspace_generation ?? -1) ||
                (generation === future.workspace_generation && state.revision >= future.revision))) this.futureState = state;
            return;
        }
        if (generation >= 0) this.nativeContext = generation;
        const previous = this.current();
        const oldGeneration = previous?.workspace_generation ?? -1;
        if (generation < oldGeneration || (generation === oldGeneration && previous && state.revision < previous.revision)) return;
        this.current.set(state);
        if (state.error) this.errorState.set(state.error);
    }

    private synchronizeContext(): void {
        const context = this.editor.workspaceGeneration();
        if (context === this.localContext()) return;
        if (this.nativeContext !== null) this.retiredNativeContext = Math.max(this.retiredNativeContext, this.nativeContext);
        this.nativeContext = null;
        this.localContext.set(context);
        this.current.set(null);
        this.errorState.set(null);
        this.placementUpdate = null;
        const future = this.futureState;
        this.futureState = null;
        if (future) this.accept(future);
    }

    private documentChanged(change: ViewportDocumentChange): void {
        if (change.workspace_generation < this.requestedGeneration) return;
        if (change.workspace_generation > this.requestedGeneration) {
            this.requestedGeneration = change.workspace_generation;
            this.requestedRevision = change.revision;
        } else this.requestedRevision = Math.max(this.requestedRevision, change.revision);
        if (this.refreshRequest || !this.editor.snapshot()) return;
        this.refreshRequest = Promise.resolve().then(async () => {
            let observedGeneration = this.requestedGeneration;
            let observedRevision = this.requestedRevision;
            let snapshot = await this.editor.refresh();
            // A burst while one refresh waits on the document queue needs at most
            // one follow-up once its authoritative revision is already current.
            while (!this.destroyed && (observedGeneration !== this.requestedGeneration || snapshot.revision < this.requestedRevision)) {
                observedGeneration = this.requestedGeneration;
                observedRevision = this.requestedRevision;
                snapshot = await this.editor.refresh();
                if (snapshot.revision < observedRevision && observedGeneration === this.requestedGeneration) break;
            }
        }).catch(error => {
            if (!this.destroyed) this.errorState.set(this.message(error));
        }).finally(() => this.refreshRequest = null);
    }

    private stopListener(stop: UnlistenFn): void {
        try {void Promise.resolve(stop()).catch(() => undefined);} catch {/* Window is already closing. */}
    }

    private reject(message: string): Promise<ViewportEditorState> {
        this.errorState.set(message);
        return Promise.reject(new Error(message));
    }

    private message(error: unknown): string {return error instanceof Error ? error.message : String(error);}
}
