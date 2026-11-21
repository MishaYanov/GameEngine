import {computed, DestroyRef, inject, Service, signal} from '@angular/core';
import {invoke, isTauri} from '@tauri-apps/api/core';
import {listen, type UnlistenFn} from '@tauri-apps/api/event';
import {applyEditorUpdate} from './editor-updates';
import {validGameplayModules} from './gameplay-modules';
import {validComponentTypes} from './editor-components';
import {validMachineDescriptor} from './state-machine.models';
import {readAnimationReview, readAnimationPreview, type AnimationReview, type AnimationPreview} from './animation.models';
import {readRuntimeInspection, type RuntimeInspection} from './runtime-inspection';
import type {LinkedPrefabStatus, PrefabSourceDocument, PrefabSourceDraft, PrefabSourceSave, PrefabSourceValidation} from './linked-prefabs.models';
import type {
    BuildLaunchResult,
    AssetOperation,
    CollisionSnapshot,
    EditorCapabilities,
    EditorCommand,
    EditorCommandRequest,
    EditorSnapshot,
    ModelAsset,
    Prefab,
    ProjectBuildPreparation,
    ProjectBuildResult,
    SceneId,
    Vec3,
    ViewportBounds,
    ViewportInput,
    ViewportStatus,
} from './editor.models';

export type EditorErrorKind = 'unavailable' | 'not_open' | 'conflict' | 'busy' | 'backend';

export interface ProjectOpenOptions {
    readonly expectedRevision?: number | null;
    readonly discardUnsaved?: boolean;
}

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
    private readonly workspaceGenerationState = signal(0);
    readonly workspaceGeneration = this.workspaceGenerationState.asReadonly();
    readonly document = computed(() => this.snapshotState()?.document ?? null);
    readonly selection = computed(() => this.snapshotState()?.selection ?? []);
    readonly dirty = computed(() => this.snapshotState()?.dirty ?? false);
    readonly playing = computed(() => this.snapshotState()?.mode === 'play');
    private readonly capabilitiesState = signal<EditorCapabilities | null>(null);
    readonly capabilities = this.capabilitiesState.asReadonly();
    private readonly assetsState = signal<readonly ModelAsset[] | null>(null);
    readonly modelAssets = this.assetsState.asReadonly();
    private assetCatalogStale = false;
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
    private assetRequest: { generation: number; promise: Promise<readonly ModelAsset[]> } | null = null;
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
                if (capabilities.protocol_version !== 2 || ![3, 4, 5].includes(capabilities.scene_schema_version)) {
                    throw new EditorClientError(
                        'backend',
                        'editor_capabilities',
                        'The native editor protocol or scene schema is incompatible with this frontend.',
                    );
                }
                if (capabilities.gameplay_modules !== undefined && !validGameplayModules(capabilities.gameplay_modules)) {
                    throw new EditorClientError(
                        'backend',
                        'editor_capabilities',
                        'The native editor returned invalid gameplay module metadata.',
                    );
                }
                if (capabilities.component_types !== undefined && !validComponentTypes(capabilities.component_types)) {
                    throw new EditorClientError('backend', 'editor_capabilities', 'The native editor returned invalid component metadata.');
                }
                if (capabilities.state_machines !== undefined && !validMachineDescriptor(capabilities.state_machines)) {
                    throw new EditorClientError('backend', 'editor_capabilities', 'The native state-machine definition is incompatible with this frontend.');
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
            this.workspaceGenerationState.update(generation => generation + 1);
            return this.accept(result);
        });
    }

    async chooseProjectManifest(): Promise<string | null> {
        try {
            return await this.native<string | null>('choose_project_manifest');
        } catch (cause: unknown) {
            throw this.recordError('choose_project_manifest', cause);
        }
    }

    async confirmProjectDiscard(): Promise<boolean> {
        try {
            return await this.native<boolean>('confirm_project_discard');
        } catch (cause: unknown) {
            throw this.recordError('confirm_project_discard', cause);
        }
    }

    requestExit(): Promise<boolean> {
        return this.enqueue('editor_exit', () =>
                this.native<boolean>('editor_exit', {expectedRevision: this.snapshotState()?.revision ?? null}),
            true,
        );
    }

    openProject(manifestPath: string, options: ProjectOpenOptions = {}): Promise<EditorSnapshot> {
        // Capture the reviewed revision now, rather than after queued edits finish.
        const expectedRevision = options.expectedRevision === undefined
            ? this.snapshotState()?.revision ?? null
            : options.expectedRevision;
        return this.enqueue('editor_open_project', async () => {
            const result = await this.native<EditorSnapshot>('editor_open_project', {
                manifestPath,
                expectedRevision,
                discardUnsaved: options.discardUnsaved ?? false,
            });
            this.assetsState.set(null);
            this.workspaceGenerationState.update(generation => generation + 1);
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

    saveAll(): Promise<EditorSnapshot> {
        return this.snapshotOperation('editor_save_all');
    }

    prepareBuild(destination: string): Promise<ProjectBuildPreparation> {
        return this.enqueue('editor_prepare_project_build', () =>
            this.native<ProjectBuildPreparation>('editor_prepare_project_build', {
                destination, expectedRevision: this.revision(),
            }), true);
    }

    /** Inputs are staged before compilation, which does not occupy the edit queue. */
    async finishBuild(buildId: number): Promise<ProjectBuildResult> {
        try {
            return await this.native<ProjectBuildResult>('editor_finish_project_build', {buildId});
        } catch (cause: unknown) {
            throw this.recordError('editor_finish_project_build', cause);
        }
    }

    async runBuild(expectedPath: string): Promise<BuildLaunchResult> {
        try {
            return await this.native<BuildLaunchResult>('editor_run_build', {expectedPath});
        } catch (cause: unknown) {
            throw this.recordError('editor_run_build', cause);
        }
    }

    restoreWorkspace(): Promise<EditorSnapshot | null> {
        const current = this.snapshotState();
        if (current || !this.nativeAvailable) return Promise.resolve(current);
        return this.enqueue('editor_snapshot', async () => {
            // Project Setup may have populated the session while this read was queued.
            const known = this.snapshotState();
            if (known) return known;
            let retained: EditorSnapshot;
            try {
                retained = await this.native<EditorSnapshot>('editor_snapshot');
            } catch (cause: unknown) {
                // The existing native command reports a missing workspace as this error.
                if ((cause instanceof Error ? cause.message : cause) === 'Open a scene before using the editor') return null;
                throw cause;
            }
            this.workspaceGenerationState.update(generation => generation + 1);
            return this.accept(retained);
        });
    }

    refresh(): Promise<EditorSnapshot> {
        return this.enqueue('editor_snapshot', async () =>
            this.accept(await this.native<EditorSnapshot>('editor_snapshot')),
        );
    }

    execute(
        command: EditorCommand,
        expectedGeneration?: number,
        expectedRevision?: number,
    ): Promise<EditorSnapshot> {
        return this.enqueue(
            'editor_execute_incremental',
            async () => {
                if (
                    expectedGeneration !== undefined &&
                    expectedGeneration !== this.workspaceGeneration()
                ) {
                    throw new Error(
                        'The workspace changed. Select the gameplay setting again.'
                    );
                }

                const request: EditorCommandRequest = {
                    expected_revision: expectedRevision ?? this.revision(),
                    command,
                };
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
            expectedGeneration,
        );
    }

    save(): Promise<EditorSnapshot> {
        return this.snapshotOperation('editor_save');
    }

    reload(): Promise<EditorSnapshot> {
        return this.snapshotOperation('editor_reload', {policy: 'discard_unsaved'}, true);
    }

    assets(refresh = false): Promise<readonly ModelAsset[]> {
        const cached = this.assetsState();
        if (cached && !refresh && !this.assetCatalogStale) return Promise.resolve(cached);
        const generation = this.workspaceGeneration();
        if (this.assetRequest?.generation === generation) return this.assetRequest.promise;
        const current = () => !this.destroyed && this.workspaceGeneration() === generation &&
            this.assetRequest?.promise === promise;
        // Discovery reads the asset root. Keep it independent of the document command queue.
        const promise: Promise<readonly ModelAsset[]> = this.native<readonly ModelAsset[]>('editor_assets')
            .then(models => {
                if (current()) {
                    this.assetsState.set(models);
                    this.assetCatalogStale = false;
                }
                return models;
            })
            .catch((cause: unknown) => {
                throw current() ? this.recordError('editor_assets', cause) : cause;
            })
            .finally(() => {
                if (this.assetRequest?.promise === promise) this.assetRequest = null;
            });
        this.assetRequest = {generation, promise};
        return promise;
    }

    importModel(path: string, position: Vec3, expectedGeneration?: number, expectedRevision?: number): Promise<EditorSnapshot> {
        return this.snapshotOperation('editor_import_model', {path, position}, false, {expectedGeneration, expectedRevision});
    }

    async chooseExternalGlb(): Promise<string | null> {
        const generation = this.workspaceGeneration();
        const current = () => !this.destroyed && generation === this.workspaceGeneration();
        try {
            const source = await this.native<string | null>('editor_choose_external_glb');
            return current() ? source : null;
        } catch (cause) {
            if (!current()) throw this.clientError('editor_choose_external_glb', cause);
            throw this.recordError('editor_choose_external_glb', cause);
        }
    }

    importExternalGlb(sourcePath: string, destinationPath: string, expectedGeneration?: number, expectedRevision?: number): Promise<AssetOperation> {
        return this.mutateAsset('editor_import_external_glb', {sourcePath, destinationPath}, expectedGeneration, expectedRevision);
    }

    reimportAsset(
        assetId: string, expectedFingerprint: string, sourcePath?: string | null,
        expectedGeneration?: number, expectedRevision?: number,
    ): Promise<AssetOperation> {
        return this.mutateAsset('editor_reimport_asset', {assetId, expectedFingerprint, sourcePath: sourcePath ?? null},
            expectedGeneration, expectedRevision);
    }

    moveAsset(assetId: string, destinationPath: string, expectedGeneration?: number, expectedRevision?: number): Promise<AssetOperation> {
        return this.mutateAsset('editor_move_asset', {assetId, destinationPath}, expectedGeneration, expectedRevision);
    }

    private mutateAsset(
        operation: string, args: Record<string, unknown>, expectedGeneration?: number, expectedRevision?: number,
    ): Promise<AssetOperation> {
        return this.enqueue(operation, async () => {
            this.requireGeneration(expectedGeneration);
            const result = await this.native<AssetOperation>(operation, {...args, expectedRevision: expectedRevision ?? this.revision()});
            this.requireGeneration(expectedGeneration);
            this.accept(result.snapshot);
            this.invalidateAssetCatalog();
            return result;
        }, true, expectedGeneration);
    }

    exportPrefab(): Promise<Prefab> {
        return this.enqueue('editor_export_prefab', () => this.native<Prefab>('editor_export_prefab'));
    }

    instantiatePrefab(prefab: Prefab, position: Vec3): Promise<EditorSnapshot> {
        return this.snapshotOperation('editor_instantiate_prefab', {prefab, position});
    }

    savePrefab(path: string, expectedGeneration?: number, expectedRevision?: number): Promise<{ readonly path: string }> {
        return this.enqueue(
            'editor_save_prefab',
            async () => {
                this.requireGeneration(expectedGeneration);
                const result = await this.native<{ readonly path: string }>('editor_save_prefab', {
                    path,
                    expectedRevision: expectedRevision ?? this.revision(),
                });
                this.invalidateAssetCatalog();
                return result;
            },
            true,
            expectedGeneration,
        );
    }

    loadPrefab(path: string, position: Vec3, expectedGeneration?: number, expectedRevision?: number): Promise<EditorSnapshot> {
        return this.snapshotOperation('editor_load_prefab', {path, position}, false, {expectedGeneration, expectedRevision});
    }

    openPrefabSource(path: string, expectedGeneration?: number, expectedRevision?: number): Promise<PrefabSourceDraft> {
        return this.prefabDraftOperation('editor_prefab_source_open', {path}, expectedGeneration, expectedRevision);
    }

    createLinkedPrefabSource(path: string, referenceSlots: Readonly<Record<string, string>>, expectedGeneration?: number, expectedRevision?: number): Promise<PrefabSourceDraft> {
        return this.enqueue('editor_create_linked_prefab_source', async () => {
            this.requireGeneration(expectedGeneration);
            const draft = await this.native<PrefabSourceDraft>('editor_create_linked_prefab_source', {path, referenceSlots, expectedRevision: expectedRevision ?? this.revision()});
            this.invalidateAssetCatalog();
            return draft;
        }, true, expectedGeneration);
    }

    validatePrefabSource(draftId: string, prefab: PrefabSourceDocument, expectedGeneration?: number, expectedRevision?: number): Promise<PrefabSourceValidation> {
        return this.prefabDraftOperation('editor_prefab_source_validate', {draftId, prefab}, expectedGeneration, expectedRevision);
    }

    savePrefabSource(draftId: string, prefab: PrefabSourceDocument, expectedGeneration?: number, expectedRevision?: number): Promise<PrefabSourceSave> {
        return this.enqueue('editor_prefab_source_save', async () => {
            this.requireGeneration(expectedGeneration);
            const result = await this.native<PrefabSourceSave>('editor_prefab_source_save', {draftId, prefab, expectedRevision: expectedRevision ?? this.revision()});
            this.accept(result.snapshot);
            this.invalidateAssetCatalog();
            return result;
        }, true, expectedGeneration);
    }

    closePrefabSource(draftId: string): Promise<void> {
        return this.native<void>('editor_prefab_source_close', {draftId});
    }

    linkedPrefabStatus(rootKey: string, expectedGeneration?: number, expectedRevision?: number): Promise<LinkedPrefabStatus | null> {
        return this.queue.then(async () => {
            this.requireGeneration(expectedGeneration);
            const result = await this.native<LinkedPrefabStatus | null>('editor_linked_prefab_status', {rootKey, expectedRevision: expectedRevision ?? this.revision()});
            this.requireGeneration(expectedGeneration);
            return result;
        });
    }

    instantiateLinkedPrefab(path: string, draftId: string, position: Vec3, bindings: Readonly<Record<string, string>>, expectedGeneration?: number, expectedRevision?: number): Promise<EditorSnapshot> {
        return this.snapshotOperation('editor_instantiate_linked_prefab', {path, draftId, position, bindings}, false, {expectedGeneration, expectedRevision});
    }

    updateLinkedPrefab(rootKey: string, draftId: string, resolution: 'reject' | 'keep_local' | 'use_source', bindings: Readonly<Record<string, string>> | null,
        expectedGeneration?: number, expectedRevision?: number): Promise<EditorSnapshot> {
        return this.snapshotOperation('editor_update_linked_prefab', {rootKey, draftId, resolution, bindings}, false, {expectedGeneration, expectedRevision});
    }

    private prefabDraftOperation<T>(operation: string, args: Record<string, unknown>, expectedGeneration?: number, expectedRevision?: number): Promise<T> {
        return this.enqueue(operation, () => {
            this.requireGeneration(expectedGeneration);
            return this.native<T>(operation, {...args, expectedRevision: expectedRevision ?? this.revision()});
        }, false, expectedGeneration);
    }

    queryCollisions(): Promise<CollisionSnapshot> {
        return this.enqueue('editor_collisions', async () => {
            const result = await this.native<CollisionSnapshot>('editor_collisions');
            this.collisionsState.set(result);
            return result;
        });
    }

    async inspectRuntime(offset = 0, limit = 32): Promise<RuntimeInspection> {
        if (!Number.isSafeInteger(offset) || offset < 0 || !Number.isSafeInteger(limit) || limit < 1 || limit > 128) {
            throw new Error('Choose a runtime page with a nonnegative offset and 1–128 entities.');
        }
        return readRuntimeInspection(await this.native<unknown>('editor_runtime_inspect', {offset, limit}), limit);
    }

    inspectAnimation(
        assetId: string,
        expectedGeneration: number,
        expectedRevision: number,
    ): Promise<AnimationReview> {
        return this.enqueue(
            'editor_animation_metadata',
            async () => {
                this.requireGeneration(expectedGeneration);
                const result = readAnimationReview(
                    await this.native<unknown>('editor_animation_metadata', { assetId, expectedRevision }),
                );
                this.requireGeneration(expectedGeneration);
                if (result.revision !== expectedRevision)
                    throw new Error('Animation review belongs to a different scene revision.');
                return result;
            },
            false,
            expectedGeneration,
        );
    }

    openAnimationPreview(
        entityKey: string,
        clipIndex: number,
        expectedGeneration: number,
        expectedRevision: number,
    ): Promise<AnimationPreview> {
        return this.enqueue(
            'editor_animation_preview_open',
            async () => {
                this.requireGeneration(expectedGeneration);
                return readAnimationPreview(
                    await this.native<unknown>('editor_animation_preview_open', {
                        entityKey,
                        clipIndex,
                        expectedRevision,
                    }),
                );
            },
            false,
            expectedGeneration,
        );
    }

    sampleAnimationPreview(
        token: string,
        timeSeconds: number,
        expectedGeneration: number,
    ): Promise<AnimationPreview> {
        return this.enqueue(
            'editor_animation_preview_sample',
            async () => {
                this.requireGeneration(expectedGeneration);
                return readAnimationPreview(
                    await this.native<unknown>('editor_animation_preview_sample', { token, timeSeconds }),
                );
            },
            false,
            expectedGeneration,
        );
    }

    closeAnimationPreview(token: string): Promise<void> {
        return this.enqueue(
            'editor_animation_preview_close',
            () => this.native<void>('editor_animation_preview_close', { token }),
            false,
        );
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
        replacesWorkspace = false,
        expected: {expectedGeneration?: number; expectedRevision?: number} = {},
    ): Promise<EditorSnapshot> {
        return this.enqueue(
            operation,
            async () => {
                this.requireGeneration(expected.expectedGeneration);
                const snapshot = await this.native<EditorSnapshot>(operation, {
                    ...args,
                    expectedRevision: expected.expectedRevision ?? this.revision(),
                });
                if (replacesWorkspace) this.workspaceGenerationState.update(generation => generation + 1);
                return this.accept(snapshot);
            },
            true,
            expected.expectedGeneration,
        );
    }

    private revision(): number {
        const snapshot = this.snapshotState();
        if (!snapshot)
            throw new EditorClientError('not_open', 'editor_execute', 'Open a scene before editing it.');
        return snapshot.revision;
    }

    private requireGeneration(expected?: number): void {
        if (expected !== undefined && expected !== this.workspaceGeneration()) {
            throw new Error('The workspace changed. Select the asset again.');
        }
    }

    private invalidateAssetCatalog(): void {
        // Retain the displayed catalog until a fresh result can replace it atomically.
        // Dropping an older request also prevents it from repopulating this cache.
        this.assetCatalogStale = true;
        this.assetRequest = null;
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
        expectedGeneration?: number,
    ): Promise<T> {
        if (expectedGeneration !== undefined && expectedGeneration !== this.workspaceGeneration()) {
            return Promise.reject(this.clientError(operation, new Error('The workspace changed. Select the setting or asset again.')));
        }
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
                if (expectedGeneration !== undefined && expectedGeneration !== this.workspaceGeneration()) {
                    throw this.clientError(operation, cause);
                }
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
        const error = this.clientError(operation, cause);
        this.errorState.set(error);
        return error;
    }

    private clientError(operation: string, cause: unknown): EditorClientError {
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
        return (
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
                )
        );
    }
}
