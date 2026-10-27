import {computed, DestroyRef, inject, Service, signal} from '@angular/core';
import {invoke, isTauri} from '@tauri-apps/api/core';
import {listen, type UnlistenFn} from '@tauri-apps/api/event';
import {applyEditorUpdate} from './editor-updates';
import type {
    CollisionSnapshot,
    EditorCapabilities,
    EditorCommand,
    EditorCommandRequest,
    EditorSnapshot,
    ModelAsset,
    Prefab,
    SceneId,
    Vec3,
    ViewportBounds,
    ViewportInput,
    ViewportStatus,
} from './editor.models';

export type EditorErrorKind = 'unavailable' | 'not_open' | 'conflict' | 'busy' | 'backend';

export class EditorClientError extends Error {
    constructor(
        readonly kind: EditorErrorKind,
        readonly operation: string,
        message: string,
        override readonly cause?: unknown,
    ) {
        super(message);
        this.name = 'EditorClientError';
    }
}

interface PendingBounds {
    readonly update: { bounds: ViewportBounds };
    readonly promise: Promise<ViewportStatus>;
}

/** One authoritative document queue. UI drafts and input bindings stay in their owners. */
@Service()
export class EditorService {
    readonly nativeAvailable = typeof window !== 'undefined' && isTauri();
    private readonly destroyRef = inject(DestroyRef);
    private readonly snapshotState = signal<EditorSnapshot | null>(null);
    readonly snapshot = this.snapshotState.asReadonly();
    readonly document = computed(() => this.snapshotState()?.document ?? null);
    readonly selection = computed(() => this.snapshotState()?.selection ?? []);
    readonly dirty = computed(() => this.snapshotState()?.dirty ?? false);
    readonly playing = computed(() => this.snapshotState()?.mode === 'play');
    private readonly capabilitiesState = signal<EditorCapabilities | null>(null);
    readonly capabilities = this.capabilitiesState.asReadonly();
    private readonly assetsState = signal<readonly ModelAsset[] | null>(null);
    readonly modelAssets = this.assetsState.asReadonly();
    private readonly collisionsState = signal<CollisionSnapshot | null>(null);
    readonly collisions = this.collisionsState.asReadonly();
    private readonly viewportState = signal<ViewportStatus | null>(null);
    readonly viewport = this.viewportState.asReadonly();
    private readonly errorState = signal<EditorClientError | null>(null);
    readonly error = this.errorState.asReadonly();
    private readonly pendingState = signal(0);
    readonly pending = this.pendingState.asReadonly();
    readonly busy = computed(() => this.pendingState() > 0);
    private readonly listeners = new Set<UnlistenFn>();
    private queue: Promise<void> = Promise.resolve();
    private viewportQueue: Promise<void> = Promise.resolve();
    private capabilitiesRequest: Promise<EditorCapabilities> | null = null;
    private viewportStatusListener: UnlistenFn | null = null;
    private pendingBounds: PendingBounds | null = null;
    private pendingDetach: Promise<ViewportStatus> | null = null;
    private pendingStatus: Promise<ViewportStatus> | null = null;
    private viewportPending = 0;
    private viewportReserved = false;
    private viewportRequested = false;
    private destroyed = false;

    constructor() {
        this.destroyRef.onDestroy(() => {
            this.destroyed = true;
            for (const unlisten of this.listeners) unlisten();
            this.listeners.clear();
            // Finish any already submitted attach before the final detach. Do not
            // leave a native child behind when its Angular owner is destroyed.
            if (this.nativeAvailable && this.viewportRequested) {
                void this.viewportQueue.then(() => invoke('editor_viewport_detach')).catch(() => undefined);
            }
        });
    }

    loadCapabilities(): Promise<EditorCapabilities> {
        const current = this.capabilitiesState();
        if (current) return Promise.resolve(current);
        if (this.capabilitiesRequest) return this.capabilitiesRequest;
        this.capabilitiesRequest = this.native<EditorCapabilities>('editor_capabilities')
            .then((capabilities) => {
                if (capabilities.protocol_version !== 2 || capabilities.scene_schema_version !== 3) {
                    throw new EditorClientError(
                        'backend',
                        'editor_capabilities',
                        'The native editor protocol or scene schema is incompatible with this frontend.',
                    );
                }
                this.capabilitiesState.set(capabilities);
                return capabilities;
            })
            .catch((cause: unknown) => {
                throw this.recordError('editor_capabilities', cause);
            })
            .finally(() => {
                this.capabilitiesRequest = null;
            });
        return this.capabilitiesRequest;
    }

    open(assetRoot: string, scenePath: string): Promise<EditorSnapshot> {
        return this.enqueue('editor_open', async () => {
            const result = await this.native<EditorSnapshot>('editor_open', {assetRoot, scenePath});
            this.assetsState.set(null);
            return this.accept(result);
        });
    }

    openProject(manifestPath: string): Promise<EditorSnapshot> {
        return this.enqueue('editor_open_project', async () => {
            const result = await this.native<EditorSnapshot>('editor_open_project', {manifestPath});
            this.assetsState.set(null);
            return this.accept(result);
        });
    }

    switchScene(sceneId: SceneId): Promise<EditorSnapshot> {
        return this.execute({type: 'switch_scene', scene_id: sceneId});
    }

    registerScene(path: string): Promise<EditorSnapshot> {
        return this.snapshotOperation('editor_register_scene', {path});
    }

    saveSceneAs(path: string): Promise<EditorSnapshot> {
        return this.snapshotOperation('editor_save_scene_as', {path});
    }

    saveProject(): Promise<EditorSnapshot> {
        return this.snapshotOperation('editor_save_project');
    }

    refresh(): Promise<EditorSnapshot> {
        return this.enqueue('editor_snapshot', async () =>
            this.accept(await this.native<EditorSnapshot>('editor_snapshot')),
        );
    }

    execute(command: EditorCommand): Promise<EditorSnapshot> {
        return this.enqueue(
            'editor_execute_incremental',
            async () => {
                const request: EditorCommandRequest = {expected_revision: this.revision(), command};
                const capabilities = await this.loadCapabilities();
                if (!capabilities.incremental_commands) {
                    try {
                        return this.accept(await this.native<EditorSnapshot>('editor_execute', {request}));
                    } catch (cause: unknown) {
                        throw this.recordError('editor_execute', cause);
                    }
                }
                const previous = this.snapshotState()!;
                const update = await this.native<unknown>('editor_execute_incremental', {request});
                return this.accept(applyEditorUpdate(previous, update));
            },
            true,
        );
    }

    save(): Promise<EditorSnapshot> {
        return this.snapshotOperation('editor_save');
    }

    reload(): Promise<EditorSnapshot> {
        return this.snapshotOperation('editor_reload', {policy: 'discard_unsaved'});
    }

    assets(refresh = false): Promise<readonly ModelAsset[]> {
        return this.enqueue('editor_assets', async () => {
            const cached = this.assetsState();
            if (cached && !refresh) return cached;
            const result = await this.native<readonly ModelAsset[]>('editor_assets');
            this.assetsState.set(result);
            return result;
        });
    }

    importModel(path: string, position: Vec3): Promise<EditorSnapshot> {
        return this.snapshotOperation('editor_import_model', {path, position});
    }

    exportPrefab(): Promise<Prefab> {
        return this.enqueue('editor_export_prefab', () => this.native<Prefab>('editor_export_prefab'));
    }

    instantiatePrefab(prefab: Prefab, position: Vec3): Promise<EditorSnapshot> {
        return this.snapshotOperation('editor_instantiate_prefab', {prefab, position});
    }

    savePrefab(path: string): Promise<{ readonly path: string }> {
        return this.enqueue(
            'editor_save_prefab',
            () =>
                this.native<{ readonly path: string }>('editor_save_prefab', {
                    path,
                    expectedRevision: this.revision(),
                }),
            true,
        );
    }

    loadPrefab(path: string, position: Vec3): Promise<EditorSnapshot> {
        return this.snapshotOperation('editor_load_prefab', {path, position});
    }

    queryCollisions(): Promise<CollisionSnapshot> {
        return this.enqueue('editor_collisions', async () => {
            const result = await this.native<CollisionSnapshot>('editor_collisions');
            this.collisionsState.set(result);
            return result;
        });
    }

    /** One Angular host can own the single embedded viewport at a time. */
    reserveViewport(): () => void {
        if (this.viewportReserved) {
            throw this.recordError(
                'editor_viewport_attach',
                new EditorClientError(
                    'busy',
                    'editor_viewport_attach',
                    'Another Angular element already owns the embedded viewport.',
                ),
            );
        }
        this.viewportReserved = true;
        let released = false;
        return () => {
            if (!released) {
                released = true;
                this.viewportReserved = false;
            }
        };
    }

    attachViewport(bounds: ViewportBounds): Promise<ViewportStatus> {
        if (this.viewportPending >= 32) return this.viewportBusy('editor_viewport_attach');
        // Lifecycle boundaries retain their order: bounds submitted after a new
        // attach must never replace bounds belonging to the preceding owner.
        this.pendingBounds = null;
        this.pendingDetach = null;
        this.pendingStatus = null;
        return this.enqueueViewport('editor_viewport_attach', async () => {
            const capabilities = await this.loadCapabilities();
            if (!capabilities.embedded_viewport_supported) {
                throw new EditorClientError(
                    'unavailable',
                    'editor_viewport_attach',
                    'This platform does not support the embedded Vulkan viewport.',
                );
            }
            if (!this.viewportStatusListener) {
                this.viewportStatusListener = await this.listenNative<ViewportStatus>(
                    'editor://viewport-status',
                    (status) => this.viewportState.set(status),
                );
            }
            this.viewportRequested = true;
            const result = await this.native<ViewportStatus>('editor_viewport_attach', {bounds});
            this.viewportState.set(result);
            return result;
        });
    }

    updateViewportBounds(bounds: ViewportBounds): Promise<ViewportStatus> {
        if (this.pendingBounds) {
            this.pendingBounds.update.bounds = bounds;
            return this.pendingBounds.promise;
        }
        if (this.viewportPending >= 32) return this.viewportBusy('editor_viewport_bounds');
        this.pendingStatus = null;
        const update = {bounds};
        const promise = this.enqueueViewport('editor_viewport_bounds', async () => {
            if (this.pendingBounds?.update === update) this.pendingBounds = null;
            const result = await this.native<ViewportStatus>('editor_viewport_bounds', {
                bounds: update.bounds,
            });
            this.viewportState.set(result);
            return result;
        });
        this.pendingBounds = {update, promise};
        return promise;
    }

    detachViewport(): Promise<ViewportStatus> {
        if (this.pendingDetach) return this.pendingDetach;
        this.pendingBounds = null;
        this.pendingStatus = null;
        // One coalesced detach may exceed the regular queue cap so releasing an
        // Angular owner can always finish. New attaches still obey the cap.
        const promise = this.enqueueViewport('editor_viewport_detach', async () => {
            const result = await this.native<ViewportStatus>('editor_viewport_detach');
            this.viewportStatusListener?.();
            this.viewportStatusListener = null;
            this.viewportRequested = false;
            this.viewportState.set(result);
            return result;
        });
        this.pendingDetach = promise;
        void promise
            .finally(() => {
                if (this.pendingDetach === promise) this.pendingDetach = null;
            })
            .catch(() => undefined);
        return promise;
    }

    refreshViewportStatus(): Promise<ViewportStatus> {
        if (this.pendingStatus) return this.pendingStatus;
        if (this.viewportPending >= 32) return this.viewportBusy('editor_viewport_status');
        const promise = this.enqueueViewport('editor_viewport_status', async () => {
            if (this.pendingStatus === promise) this.pendingStatus = null;
            const result = await this.native<ViewportStatus>('editor_viewport_status');
            this.viewportState.set(result);
            return result;
        });
        this.pendingStatus = promise;
        return promise;
    }

    /** Subscribe explicitly; raw movement never sends document commands by itself. */
    listenViewportInput(handler: (input: ViewportInput) => void): Promise<UnlistenFn> {
        return this.listenNative<ViewportInput>('editor://viewport-input', handler);
    }

    clearError(): void {
        this.errorState.set(null);
    }

    private snapshotOperation(
        operation: string,
        args: Record<string, unknown> = {},
    ): Promise<EditorSnapshot> {
        return this.enqueue(
            operation,
            async () =>
                this.accept(
                    await this.native<EditorSnapshot>(operation, {
                        ...args,
                        expectedRevision: this.revision(),
                    }),
                ),
            true,
        );
    }

    private revision(): number {
        const snapshot = this.snapshotState();
        if (!snapshot)
            throw new EditorClientError('not_open', 'editor_execute', 'Open a scene before editing it.');
        return snapshot.revision;
    }

    private accept(snapshot: EditorSnapshot): EditorSnapshot {
        this.snapshotState.set(snapshot);
        this.collisionsState.set(null);
        return snapshot;
    }

    private enqueue<T>(
        operation: string,
        action: () => Promise<T>,
        recoverSnapshot = false,
    ): Promise<T> {
        if (this.pendingState() >= 256) {
            return Promise.reject(
                this.recordError(
                    operation,
                    new EditorClientError(
                        'busy',
                        operation,
                        'Editor command queue is full; coalesce continuous input.',
                    ),
                ),
            );
        }
        this.pendingState.update((pending) => pending + 1);
        const result = this.queue.then(async () => {
            try {
                this.assertNative(operation);
                const value = await action();
                this.errorState.set(null);
                return value;
            } catch (cause: unknown) {
                // Refresh the in-memory authoritative state, never reload the disk file.
                // Rejected edits are not retried automatically against a new revision.
                if (recoverSnapshot && this.snapshotState() && !this.destroyed) {
                    try {
                        this.accept(await this.native<EditorSnapshot>('editor_snapshot'));
                    } catch {
                        /* Retain the last known document if recovery is unavailable. */
                    }
                }
                throw this.recordError(operation, cause);
            }
        });
        this.queue = result.then(
            () => undefined,
            () => undefined,
        );
        return result.finally(() => this.pendingState.update((pending) => pending - 1));
    }

    private enqueueViewport<T>(operation: string, action: () => Promise<T>): Promise<T> {
        this.viewportPending += 1;
        const result = this.viewportQueue.then(async () => {
            try {
                this.assertNative(operation);
                return await action();
            } catch (cause: unknown) {
                throw this.recordError(operation, cause);
            }
        });
        this.viewportQueue = result.then(
            () => undefined,
            () => undefined,
        );
        return result.finally(() => {
            this.viewportPending -= 1;
        });
    }

    private viewportBusy(operation: string): Promise<never> {
        return Promise.reject(
            this.recordError(
                operation,
                new EditorClientError(
                    'busy',
                    operation,
                    'Viewport lifecycle queue is full; coalesce repeated attachment requests.',
                ),
            ),
        );
    }

    private async listenNative<T>(event: string, handler: (payload: T) => void): Promise<UnlistenFn> {
        if (!this.nativeAvailable || this.destroyed) return () => undefined;
        let active = true;
        const nativeUnlisten = await listen<T>(event, (message) => {
            if (active && !this.destroyed) handler(message.payload);
        });
        const unlisten = () => {
            if (active) {
                active = false;
                this.listeners.delete(unlisten);
                try {
                    // Tauri currently returns an async cleanup despite UnlistenFn's void
                    // signature. Window shutdown may reject that IPC; local callbacks
                    // must still remain disabled and teardown must continue.
                    void Promise.resolve(nativeUnlisten()).catch(() => undefined);
                } catch {
                    /* The owning WebView may already have closed. */
                }
            }
        };
        if (this.destroyed) unlisten();
        else this.listeners.add(unlisten);
        return unlisten;
    }

    private native<T>(operation: string, args?: Record<string, unknown>): Promise<T> {
        try {
            this.assertNative(operation);
        } catch (cause: unknown) {
            return Promise.reject(cause);
        }
        return invoke<T>(operation, args);
    }

    private assertNative(operation: string): void {
        if (!this.nativeAvailable || this.destroyed) {
            throw new EditorClientError(
                'unavailable',
                operation,
                this.destroyed
                    ? 'The editor service has been destroyed.'
                    : 'Native editor features require the Tauri desktop app.',
            );
        }
    }

    private recordError(operation: string, cause: unknown): EditorClientError {
        const backend =
            typeof cause === 'object' &&
            cause !== null &&
            'code' in cause &&
            typeof cause.code === 'string' &&
            'message' in cause &&
            typeof cause.message === 'string'
                ? {code: cause.code, message: cause.message}
                : null;
        const message = backend
            ? backend.message
            : cause instanceof Error
                ? cause.message
                : typeof cause === 'string'
                    ? cause
                    : 'The native editor operation failed.';
        const error =
            cause instanceof EditorClientError
                ? cause
                : new EditorClientError(
                    backend
                        ? backend.code === 'conflict'
                            ? 'conflict'
                            : 'backend'
                        : operation !== 'editor_execute_incremental' && /revision conflict/i.test(message)
                            ? 'conflict'
                            : 'backend',
                    operation,
                    message,
                    cause,
                );
        this.errorState.set(error);
        return error;
    }
}
