import {type AudioClipDetails, readAudioClipDetails} from './audio-clip.models';
import {readSkyPanoramas, type SkyPanoramaAsset, type SkyPanoramaScope} from './sky.models';
import {
    type ControlActionSample, type ControlGraph, type ControlGraphAsset, type ControlGraphCapture, type ControlGraphDocument,
    type ControlGraphProbe, readControlGraphCatalog, readControlGraphDocument, readControlGraphProbe,
} from './controls-graph.models';
import {computed, DestroyRef, inject, Service, signal} from '@angular/core';
import {invoke, isTauri} from '@tauri-apps/api/core';
import type {UnlistenFn} from '@tauri-apps/api/event';
import {
    type AnimatorActorValidation, type AnimatorAsset, type AnimatorAssetCapture, type AnimatorDocument, type AnimatorGraph,
    readAnimatorCatalog, readAnimatorDocument, readAnimatorValidation,
} from './animator.models';
import {EditorClientError} from './editor-client-error';
import {
    type InputActionMap, type InputActionsAsset, type InputActionsCapture, type InputActionsDocument,
    readInputActionsAsset, readInputActionsCatalog, readInputActionsDocument,
} from '../game-input/input-actions.models';
import {applyEditorUpdate} from './editor-updates';
import {ViewportAttachment} from './viewport/viewport-attachment';
import {validGameplayModules} from './gameplay-modules';
import {type SceneVerificationReport, validSceneVerification} from './scene-verification.models';
import {validComponentTypes} from './editor-components';
import {CameraPreviewController, readCameraPreview} from './camera-preview';
import {validMachineDescriptor} from './state-machine.models';
import {
    type FeedbackBusMix,
    type FeedbackCueAssignment,
    type FeedbackCueDraft,
    readFeedbackCueDraft,
    validCuePath
} from './feedback-audio.models';
import {
    type AnimationPreview,
    type AnimationReview,
    readAnimationPreview,
    readAnimationReview
} from './animation.models';
import {
    readScopedAnimationPreview, type AnimationWorkspaceTarget, type ImportedKeyPage, type ScopedAnimationPreview,
} from './animation-workspace.models';
import {
    type AuthoredClip, type AuthoredClipPreviewRequest, type ClipAssetDocument, type ClipAssetIndex,
    type ClipAssetSave, type ClipAssetVersion, readAuthoredClipPreview, type VisualClipPreview,
    type ClipDraftOperation, type ClipTimelineDraft, type ClipTimelinePreviewRequest, type SavedClipTimeline, type TimelineKeyMove,
} from './authored-clip.models';
import {readRuntimeInspection, type RuntimeInspection} from './runtime-inspection';
import {isSceneSchemaVersion} from './editor.models';
import type {
    LinkedPrefabOverrideResolution,
    LinkedPrefabOverrideReview,
    LinkedPrefabProperty,
    LinkedPrefabStatus,
    LinkedPrefabRecoveryReview,
    PropAddress,
    PrefabSourceDocument,
    PrefabSourceDraft,
    PrefabSourceSave,
    PrefabSourceValidation
} from './linked-prefabs.models';
import type {
    AssetOperation,
    BuildLaunchResult,
    CollisionSnapshot,
    EditorCapabilities,
    EditorCommand,
    EditorCommandRequest,
    EditorSnapshot,
    EntityKey,
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

export {EditorClientError, type EditorErrorKind} from './editor-client-error';

export interface ProjectOpenOptions {
    readonly expectedRevision?: number | null;
    readonly discardUnsaved?: boolean;
}

export interface WorkspaceAssetScope {
    readonly workspaceGeneration: number;
    readonly nativeGeneration: number;
    readonly revision: number;
}
export type InputActionsScope = WorkspaceAssetScope;

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
    private readonly authoredClipCatalogVersionState = signal(0);
    readonly authoredClipCatalogVersion = this.authoredClipCatalogVersionState.asReadonly();
    private readonly collisionsState = signal<CollisionSnapshot | null>(null);
    readonly collisions = this.collisionsState.asReadonly();
    readonly cameraPreview = new CameraPreviewController({
        open: async definition => readCameraPreview(
            await this.native<unknown>('editor_camera_preview_open', {...definition})),
        sample: async (token, nextSample, intent) => readCameraPreview(
            await this.native<unknown>('editor_camera_preview_sample', {token, nextSample, intent})),
        // An in-flight Open can finish during service teardown; its token still needs to be closed.
        close: token => invoke<void>('editor_camera_preview_close', {token}),
    });
    private readonly viewportAttachment = new ViewportAttachment({
        nativeAvailable: this.nativeAvailable,
        loadCapabilities: () => this.loadCapabilities(),
        assertNative: operation => this.assertNative(operation),
        invoke: (operation, args) => this.native(operation, args),
        recordError: (operation, cause) => this.recordError(operation, cause),
    });
    private readonly errorState = signal<EditorClientError | null>(null);
    readonly error = this.errorState.asReadonly();
    private readonly pendingState = signal(0);
    readonly pending = this.pendingState.asReadonly();
    readonly busy = computed(() => this.pendingState() > 0);
    private queue: Promise<void> = Promise.resolve();
    private capabilitiesRequest: Promise<EditorCapabilities> | null = null;
    private assetRequest: { generation: number; promise: Promise<readonly ModelAsset[]> } | null = null;
    private destroyed = false;
    readonly viewport = this.viewportAttachment.status;

    constructor() {
        this.destroyRef.onDestroy(() => {
            this.cameraPreview.close();
            this.destroyed = true;
            this.viewportAttachment.destroy();
        });
    }

    loadCapabilities(): Promise<EditorCapabilities> {
        const current = this.capabilitiesState();
        if (current) return Promise.resolve(current);
        if (this.capabilitiesRequest) return this.capabilitiesRequest;
        this.capabilitiesRequest = this.native<EditorCapabilities>('editor_capabilities')
            .then((capabilities) => {
                if (capabilities.protocol_version !== 2 || !isSceneSchemaVersion(capabilities.scene_schema_version)) {
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

    setSceneWarmingMode(mode: import('./viewport-editor.models').EditorWarmingMode, expectedGeneration: number): Promise<number> {
        return this.native<number>('editor_set_scene_warming_mode', {mode, expectedGeneration});
    }

    async checkScenes(sceneId: SceneId, expectedRevision: number, expectedGeneration: number | null): Promise<SceneVerificationReport> {
        const result = await this.native<unknown>('editor_check_scenes', {
            sceneId,
            expectedRevision,
            expectedGeneration
        });
        if (!validSceneVerification(result)) {
            throw new EditorClientError('backend', 'editor_check_scenes', 'The native scene verification report was invalid.');
        }
        return result;
    }

    exportWarmingReport(expectedGeneration: number): Promise<number | null> {
        return this.native<number | null>('editor_export_warming_report', {expectedGeneration});
    }

    warmScene(sceneId: SceneId | null, expectedRevision: number, expectedGeneration: number, expectedModeRevision: number): Promise<void> {
        return this.native<void>('editor_warm_scene', {
            sceneId,
            expectedRevision,
            expectedGeneration,
            expectedModeRevision
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

    listInputActions(scope: InputActionsScope): Promise<readonly InputActionsAsset[]> {
        return this.workspaceAssetOperation('editor_list_input_actions', {}, scope, readInputActionsCatalog);
    }

    openInputActions(assetId: string, scope: InputActionsScope): Promise<InputActionsDocument> {
        return this.workspaceAssetOperation('editor_open_input_actions', {assetId}, scope, readInputActionsDocument);
    }

    saveInputActions(
        destination: string, map: InputActionMap, capture: InputActionsCapture | null, scope: InputActionsScope,
    ): Promise<InputActionsDocument> {
        return this.workspaceAssetOperation('editor_save_input_actions', {destination, map, capture}, scope, readInputActionsDocument);
    }

    moveInputActions(destination: string, capture: InputActionsCapture, scope: InputActionsScope): Promise<InputActionsAsset> {
        return this.workspaceAssetOperation('editor_move_input_actions', {destination, capture}, scope, readInputActionsAsset);
    }

    async listAnimators(scope: WorkspaceAssetScope): Promise<readonly AnimatorAsset[]> {
        this.requireGeneration(scope.workspaceGeneration);
        const entries = await this.native<unknown>('editor_list_animators', {
            expectedGeneration: scope.nativeGeneration, expectedRevision: scope.revision,
        });
        this.requireGeneration(scope.workspaceGeneration);
        return readAnimatorCatalog(entries);
    }

    openAnimator(assetId: string, scope: WorkspaceAssetScope): Promise<AnimatorDocument> {
        return this.workspaceAssetOperation('editor_open_animator', {assetId}, scope, readAnimatorDocument);
    }

    saveAnimator(
        destination: string, graph: AnimatorGraph, capture: AnimatorAssetCapture | null, scope: WorkspaceAssetScope,
    ): Promise<AnimatorDocument> {
        return this.workspaceAssetOperation('editor_save_animator', {destination, graph, capture}, scope, readAnimatorDocument);
    }

    validateAnimatorActor(entityKey: EntityKey, scope: WorkspaceAssetScope): Promise<AnimatorActorValidation> {
        return this.workspaceAssetOperation('editor_validate_animator_actor', {entityKey}, scope, readAnimatorValidation);
    }

    async listControlGraphs(scope: WorkspaceAssetScope): Promise<readonly ControlGraphAsset[]> {
        this.requireGeneration(scope.workspaceGeneration);
        const entries = await this.native<unknown>('editor_list_control_graphs', {
            expectedGeneration: scope.nativeGeneration, expectedRevision: scope.revision,
        });
        this.requireGeneration(scope.workspaceGeneration);
        return readControlGraphCatalog(entries);
    }

    openControlGraph(assetId: string, scope: WorkspaceAssetScope): Promise<ControlGraphDocument> {
        return this.workspaceAssetOperation('editor_open_control_graph', {assetId}, scope, readControlGraphDocument);
    }

    saveControlGraph(
        destination: string, graph: ControlGraph, capture: ControlGraphCapture | null, scope: WorkspaceAssetScope,
    ): Promise<ControlGraphDocument> {
        return this.workspaceAssetOperation('editor_save_control_graph', {destination, graph, capture}, scope, readControlGraphDocument);
    }

    async probeControlsGraph(
        graph: ControlGraph, actions: readonly ControlActionSample[], pin: ControlGraphCapture | null,
        entityKey: EntityKey | null, scope: WorkspaceAssetScope,
    ): Promise<ControlGraphProbe> {
        this.requireGeneration(scope.workspaceGeneration);
        const result = await this.native<unknown>('editor_probe_controls_graph', {
            graph, actions, assetId: pin?.id ?? null, fingerprint: pin?.fingerprint ?? null, entityKey,
            expectedGeneration: scope.nativeGeneration, expectedRevision: scope.revision,
        });
        this.requireGeneration(scope.workspaceGeneration);
        return readControlGraphProbe(result);
    }

    private workspaceAssetOperation<T>(
        operation: string, args: Record<string, unknown>, scope: WorkspaceAssetScope, read: (value: unknown) => T,
    ): Promise<T> {
        return this.enqueue(operation, async () => {
            this.requireGeneration(scope.workspaceGeneration);
            const response = await this.native<unknown>(operation, {
                ...args, expectedGeneration: scope.nativeGeneration, expectedRevision: scope.revision,
            });
            this.requireGeneration(scope.workspaceGeneration);
            return read(response);
        }, false, scope.workspaceGeneration);
    }

    importModel(path: string, position: Vec3, expectedGeneration?: number, expectedRevision?: number): Promise<EditorSnapshot> {
        return this.snapshotOperation('editor_import_model', {path, position}, false, {expectedGeneration, expectedRevision});
    }

    chooseExternalGlb(): Promise<string | null> {
        return this.chooseExternalAssetSource('editor_choose_external_glb');
    }

    chooseExternalWav(): Promise<string | null> {
        return this.chooseExternalAssetSource('editor_choose_external_wav');
    }

    chooseExternalSkyPng(): Promise<string | null> {
        return this.chooseExternalAssetSource('editor_choose_external_sky_png');
    }

    private async chooseExternalAssetSource(
        operation: 'editor_choose_external_glb' | 'editor_choose_external_wav' | 'editor_choose_external_sky_png',
    ): Promise<string | null> {
        const generation = this.workspaceGeneration();
        const current = () => !this.destroyed && generation === this.workspaceGeneration();
        try {
            const source = await this.native<string | null>(operation);
            return current() ? source : null;
        } catch (cause) {
            if (!current()) throw this.clientError(operation, cause);
            throw this.recordError(operation, cause);
        }
    }

    async audioClipDetails(
        assetId: string, expectedGeneration: number, expectedRevision: number, nativeGeneration: number | null,
    ): Promise<AudioClipDetails> {
        this.requireGeneration(expectedGeneration);
        const response = await this.native<unknown>('editor_audio_clip_info', {
            assetId, expectedRevision, expectedGeneration: nativeGeneration,
        });
        this.requireGeneration(expectedGeneration);
        return readAudioClipDetails(response);
    }

    async listSkyPanoramas(scope: SkyPanoramaScope): Promise<readonly SkyPanoramaAsset[]> {
        this.requireGeneration(scope.workspaceGeneration);
        const result = await this.native<unknown>('editor_list_sky_panoramas', {
            expectedGeneration: scope.nativeGeneration, expectedRevision: scope.revision,
        });
        this.requireGeneration(scope.workspaceGeneration);
        return readSkyPanoramas(result);
    }

    importSkyPanorama(sourcePath: string, destinationPath: string, scope: SkyPanoramaScope): Promise<AssetOperation> {
        return this.mutateAsset('editor_import_sky_panorama', {
            sourcePath, destinationPath, expectedGeneration: scope.nativeGeneration,
        }, scope.workspaceGeneration, scope.revision);
    }

    feedbackCues(expectedGeneration: number): Promise<FeedbackCueDraft> {
        return this.enqueue('editor_feedback_cues_read', async () => {
            this.requireGeneration(expectedGeneration);
            const draft = await this.native<unknown>('editor_feedback_cues_read');
            this.requireGeneration(expectedGeneration);
            return readFeedbackCueDraft(draft);
        }, false, expectedGeneration);
    }

    async importFeedbackCue(
        workspaceGeneration: number, slug: string, expectedGeneration: number,
    ): Promise<string | null> {
        this.requireGeneration(expectedGeneration);
        const result = await this.native<unknown>('editor_feedback_cue_import', {workspaceGeneration, slug});
        this.requireGeneration(expectedGeneration);
        if (result !== null && !validCuePath(result)) {
            throw new Error('The editor returned an invalid imported feedback cue path.');
        }
        return result;
    }

    applyFeedbackCues(
        workspaceGeneration: number, expectedDigest: string | null, cues: readonly FeedbackCueAssignment[],
        expectedGeneration: number, mix: FeedbackBusMix,
    ): Promise<FeedbackCueDraft> {
        const operation = 'editor_feedback_cues_apply';
        return this.enqueue(operation, async () => {
            this.requireGeneration(expectedGeneration);
            const result = await this.native<unknown>(operation, {workspaceGeneration, expectedDigest, cues, mix});
            this.requireGeneration(expectedGeneration);
            return readFeedbackCueDraft(result);
        }, false, expectedGeneration);
    }

    async auditionAudioSource(
        scope: import('./audio-source.models').AudioAuditionScope, componentId: string, requestId: number,
    ): Promise<unknown> {
        this.requireGeneration(scope.generation);
        return this.native('editor_audio_source_audition', {
            target: {...scope.owner, component_id: componentId}, expectedGeneration: scope.nativeGeneration,
            expectedRevision: scope.revision, requestId,
        });
    }

    stopAudioSourceAudition(token: string): Promise<void> {
        return this.native('editor_audio_source_audition_stop', {token});
    }

    async previewFeedbackCue(
        workspaceGeneration: number, relativePath: string, expectedGeneration: number,
    ): Promise<unknown> {
        this.requireGeneration(expectedGeneration);
        const bytes = await this.native<unknown>('editor_feedback_cue_preview', {workspaceGeneration, relativePath});
        this.requireGeneration(expectedGeneration);
        return bytes;
    }
    importExternalGlb(sourcePath: string, destinationPath: string, expectedGeneration?: number, expectedRevision?: number): Promise<AssetOperation> {
        return this.mutateAsset('editor_import_external_glb', {sourcePath, destinationPath}, expectedGeneration, expectedRevision);
    }

    importExternalWav(sourcePath: string, destinationPath: string, expectedGeneration?: number, expectedRevision?: number): Promise<AssetOperation> {
        return this.mutateAsset('editor_import_external_wav', {sourcePath, destinationPath}, expectedGeneration, expectedRevision);
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

    createLinkedPrefabSource(
        path: string, referenceSlots: Readonly<Record<string, string>>, expectedGeneration?: number,
        expectedRevision?: number, pivot?: EntityKey,
    ): Promise<PrefabSourceDraft> {
        return this.enqueue('editor_create_linked_prefab_source', async () => {
            this.requireGeneration(expectedGeneration);
            const draft = await this.native<PrefabSourceDraft>('editor_create_linked_prefab_source', {
                path, referenceSlots, expectedRevision: expectedRevision ?? this.revision(),
                ...(pivot === undefined ? {} : {pivot}),
            });
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

    editPropDraft(
        draftId: string, expectedDraftRevision: number,
        edit: import('./linked-prefabs.models').PropDraftEdit, expectedGeneration: number,
    ): Promise<PrefabSourceDraft> {
        return this.enqueue('editor_prop_draft_edit', async () => {
            this.requireGeneration(expectedGeneration);
            return this.native<PrefabSourceDraft>('editor_prop_draft_edit', {draftId, expectedDraftRevision, edit});
        }, false, expectedGeneration);
    }

    savePropDraft(draftId: string, expectedDraftRevision: number, expectedGeneration: number): Promise<PrefabSourceDraft> {
        return this.enqueue('editor_prop_draft_save', async () => {
            this.requireGeneration(expectedGeneration);
            const result = await this.native<PrefabSourceSave>('editor_prop_draft_save', {draftId, expectedDraftRevision});
            this.accept(result.snapshot);
            this.invalidateAssetCatalog();
            return result.draft;
        }, false, expectedGeneration);
    }

    reviewNestedProp(assetId: string, fingerprint: string, nativeGeneration: number, expectedGeneration: number): Promise<import('./linked-prefabs.models').NestedPropSource> {
        return this.enqueue('editor_prop_nested_source', async () => {
            this.requireGeneration(expectedGeneration);
            return this.native<import('./linked-prefabs.models').NestedPropSource>('editor_prop_nested_source', {
                assetId, fingerprint, expectedGeneration: nativeGeneration,
            });
        }, false, expectedGeneration);
    }

    addNestedProp(
        draftId: string, expectedDraftRevision: number, placement: import('./linked-prefabs.models').NestedPropPlacementRequest,
        expectedGeneration: number,
    ): Promise<PrefabSourceDraft> {
        return this.enqueue('editor_prop_draft_add_nested', async () => {
            this.requireGeneration(expectedGeneration);
            return this.native<PrefabSourceDraft>('editor_prop_draft_add_nested', {draftId, expectedDraftRevision, placement});
        }, false, expectedGeneration);
    }

    refreshNestedProp(
        draftId: string, expectedDraftRevision: number, placementId: string, fingerprint: string, expectedGeneration: number,
    ): Promise<PrefabSourceDraft> {
        return this.enqueue('editor_prop_draft_refresh_nested', async () => {
            this.requireGeneration(expectedGeneration);
            return this.native<PrefabSourceDraft>('editor_prop_draft_refresh_nested', {
                draftId, expectedDraftRevision, placementId, fingerprint,
            });
        }, false, expectedGeneration);
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

    reviewLinkedPropOverrides(
        rootKey: EntityKey, draftId: string, bindings: Readonly<Record<string, EntityKey>>,
        expectedGeneration: number, expectedRevision: number,
    ): Promise<readonly LinkedPrefabOverrideReview[]> {
        return this.prefabDraftOperation(
            'editor_review_linked_prop_overrides', {rootKey, draftId, bindings}, expectedGeneration, expectedRevision,
        );
    }

    updateLinkedPropReviewed(
        rootKey: EntityKey, draftId: string, bindings: Readonly<Record<string, EntityKey>>,
        resolutions: readonly LinkedPrefabOverrideResolution[], expectedGeneration: number, expectedRevision: number,
    ): Promise<EditorSnapshot> {
        return this.snapshotOperation(
            'editor_update_linked_prop_reviewed', {rootKey, draftId, bindings, resolutions},
            false, {expectedGeneration, expectedRevision},
        );
    }

    reviewLinkedPropRecovery(
        rootKey: EntityKey, path: string, draftId: string, expectedGeneration: number, expectedRevision: number,
    ): Promise<LinkedPrefabRecoveryReview> {
        return this.prefabDraftOperation(
            'editor_review_linked_prop_recovery', {rootKey, path, draftId}, expectedGeneration, expectedRevision,
        );
    }

    recoverLinkedProp(
        rootKey: EntityKey, path: string, draftId: string, expectedGeneration: number, expectedRevision: number,
    ): Promise<EditorSnapshot> {
        return this.snapshotOperation(
            'editor_recover_linked_prop', {rootKey, path, draftId}, false, {expectedGeneration, expectedRevision},
        );
    }

    applyLinkedPropOverride(
        rootKey: EntityKey, draftId: string, address: PropAddress, property: LinkedPrefabProperty,
        expectedGeneration: number, expectedRevision: number,
    ): Promise<PrefabSourceSave> {
        return this.enqueue('editor_apply_linked_prop_override', async () => {
            this.requireGeneration(expectedGeneration);
            const result = await this.native<PrefabSourceSave>('editor_apply_linked_prop_override', {
                rootKey, draftId, address, property, expectedRevision,
            });
            this.requireGeneration(expectedGeneration);
            this.accept(result.snapshot);
            this.invalidateAssetCatalog();
            return result;
        }, true, expectedGeneration);
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

    listAuthoredClips(generation: number): Promise<readonly ClipAssetIndex[]> {
        return this.enqueue('editor_animation_clips', async () => {
            this.requireGeneration(generation);
            const clips = await this.native<readonly ClipAssetIndex[]>('editor_animation_clips');
            this.requireGeneration(generation);
            return clips;
        }, false, generation);
    }

    openAuthoredClip(assetId: string, generation: number): Promise<ClipAssetDocument> {
        return this.enqueue('editor_open_authored_clip', async () => {
            this.requireGeneration(generation);
            const document = await this.native<ClipAssetDocument>('editor_open_authored_clip', {assetId});
            this.requireGeneration(generation);
            return document;
        }, false, generation);
    }

    saveAuthoredClip(
        path: string, clip: AuthoredClip, replacement: ClipAssetVersion | null,
        generation: number, expectedRevision: number,
    ): Promise<ClipAssetDocument> {
        return this.enqueue('editor_save_authored_clip', async () => {
            this.requireGeneration(generation);
            const result = await this.native<ClipAssetSave>('editor_save_authored_clip', {
                path, clip, replacement, expectedRevision,
            });
            this.requireGeneration(generation);
            this.accept(result.snapshot);
            this.invalidateAssetCatalog();
            this.authoredClipCatalogVersionState.update(version => version + 1);
            return result.document;
        }, true, generation);
    }

    openAuthoredClipPreview(request: AuthoredClipPreviewRequest, generation: number): Promise<VisualClipPreview> {
        return this.enqueue('editor_authored_clip_preview_open', async () => {
            this.requireGeneration(generation);
            return readAuthoredClipPreview(await this.native<unknown>('editor_authored_clip_preview_open', {...request}));
        }, false, generation);
    }

    sampleAuthoredClipPreview(token: string, tick: number, generation: number): Promise<VisualClipPreview> {
        return this.enqueue('editor_authored_clip_preview_sample', async () => {
            this.requireGeneration(generation);
            return readAuthoredClipPreview(await this.native<unknown>('editor_authored_clip_preview_sample', {token, tick}));
        }, false, generation);
    }

    closeAuthoredClipPreview(token: string): Promise<void> {
        return this.enqueue('editor_authored_clip_preview_close', () =>
            this.native<void>('editor_authored_clip_preview_close', {token}));
    }

    openClipTimelineDraft(assetId: string, generation: number, expectedRevision: number): Promise<ClipTimelineDraft> {
        return this.prefabDraftOperation('editor_clip_timeline_draft_open', {assetId}, generation, expectedRevision);
    }

    editClipTimelineDraft(
        draftId: string, expectedDraftRevision: number, operation: ClipDraftOperation, generation: number,
    ): Promise<ClipTimelineDraft> {
        return this.enqueue('editor_clip_timeline_draft_edit', () => {
            this.requireGeneration(generation);
            return this.native<ClipTimelineDraft>('editor_clip_timeline_draft_edit', {draftId, expectedDraftRevision, operation});
        }, false, generation);
    }

    saveClipTimelineDraft(
        draftId: string, expectedDraftRevision: number, generation: number, expectedRevision: number,
    ): Promise<ClipTimelineDraft> {
        return this.enqueue('editor_clip_timeline_draft_save', async () => {
            this.requireGeneration(generation);
            const result = await this.native<SavedClipTimeline>('editor_clip_timeline_draft_save', {
                draftId, expectedDraftRevision, expectedRevision,
            });
            this.requireGeneration(generation);
            this.accept(result.snapshot);
            this.invalidateAssetCatalog();
            this.authoredClipCatalogVersionState.update(version => version + 1);
            return result.draft;
        }, true, generation);
    }

    closeClipTimelineDraft(draftId: string, discard: boolean): Promise<void> {
        return this.enqueue('editor_clip_timeline_draft_close', () =>
            this.native<void>('editor_clip_timeline_draft_close', {draftId, discard}));
    }

    openClipTimelinePreview(request: ClipTimelinePreviewRequest, generation: number): Promise<VisualClipPreview> {
        return this.enqueue('editor_clip_timeline_preview_open', async () => {
            this.requireGeneration(generation);
            return readAuthoredClipPreview(await this.native<unknown>('editor_clip_timeline_preview_open', {...request}));
        }, false, generation);
    }

    sampleClipTimelinePreview(
        token: string, tick: number, drag: TimelineKeyMove | null, generation: number,
    ): Promise<VisualClipPreview> {
        return this.enqueue('editor_clip_timeline_preview_sample', async () => {
            this.requireGeneration(generation);
            return readAuthoredClipPreview(await this.native<unknown>('editor_clip_timeline_preview_sample', {token, tick, drag}));
        }, false, generation);
    }

    openAnimationWorkspacePreview(
        target: AnimationWorkspaceTarget,
        clipIndex: number,
        expectedGeneration: number,
        expectedRevision: number,
        nativeGeneration: number,
        requestId: number,
    ): Promise<ScopedAnimationPreview> {
        return this.enqueue('editor_animation_workspace_open', async () => {
            this.requireGeneration(expectedGeneration);
            return readScopedAnimationPreview(await this.native<unknown>('editor_animation_workspace_open', {
                target, clipIndex, expectedRevision, expectedGeneration: nativeGeneration, requestId,
            }));
        }, false, expectedGeneration);
    }

    sampleAnimationWorkspacePreview(token: string, timeSeconds: number, expectedGeneration: number): Promise<ScopedAnimationPreview> {
        return this.enqueue('editor_animation_preview_sample', async () => {
            this.requireGeneration(expectedGeneration);
            return readScopedAnimationPreview(await this.native<unknown>('editor_animation_preview_sample', {token, timeSeconds}));
        }, false, expectedGeneration);
    }

    importedAnimationKeys(token: string, track: number, offset: number, generation: number): Promise<ImportedKeyPage> {
        return this.enqueue('editor_animation_preview_keys', () => {
            this.requireGeneration(generation);
            return this.native<ImportedKeyPage>('editor_animation_preview_keys', {token, track, offset});
        }, false, generation);
    }

    frameAnimationWorkspacePreview(token: string): Promise<void> {
        return this.native<void>('editor_prop_preview_frame', {token});
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

    reserveViewport(): () => void {
        return this.viewportAttachment.reserve();
    }

    attachViewport(bounds: ViewportBounds): Promise<ViewportStatus> {
        return this.viewportAttachment.attach(bounds);
    }

    updateViewportBounds(bounds: ViewportBounds): Promise<ViewportStatus> {
        return this.viewportAttachment.updateBounds(bounds);
    }

    detachViewport(): Promise<ViewportStatus> {
        return this.viewportAttachment.detach();
    }

    refreshViewportStatus(): Promise<ViewportStatus> {
        return this.viewportAttachment.refreshStatus();
    }

    listenViewportInput(handler: (input: ViewportInput) => void): Promise<UnlistenFn> {
        return this.viewportAttachment.listenInput(handler);
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
