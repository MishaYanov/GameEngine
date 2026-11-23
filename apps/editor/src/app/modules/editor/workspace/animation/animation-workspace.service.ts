import {computed, DestroyRef, effect, inject, Injectable, signal, untracked} from '@angular/core';
import {listen} from '@tauri-apps/api/event';
import type {AnimationReview, AnimationSource} from '../../../../core/editor/animation.models';
import {
    matchesAnimationTarget, reserveAnimationPreviewRequest, type ScopedAnimationPreview,
} from '../../../../core/editor/animation-workspace.models';
import type {ClipAssetDocument} from '../../../../core/editor/authored-clip.models';
import {EditorService, type WorkspaceAssetScope} from '../../../../core/editor/editor.service';
import {ViewportEditorService} from '../../../../core/editor/viewport-editor.service';
import {type PropWorkspaceDocument, PropWorkspaceService} from '../prop-workspace/prop-workspace.service';
import {SceneEditingService} from '../scene-editing.service';
import {WorkspaceViewService} from '../scene-viewport/workspace-view.service';
import {ScopedPaneRequests, type PreparedScopedPane, type ScopedPaneTarget} from '../scoped-pane-request';
import type {AnimationPaneRequest} from './animation-pane-request';
import {type AnimationTargetOption, boundAnimationTarget, propAnimationTargets, sceneAnimationTarget} from './animation-workspace-target';

interface AuthoredClipReveal {
    readonly document: ClipAssetDocument;
    readonly markerId: string;
    readonly openTimeline: boolean;
    readonly current: () => boolean;
}

interface PreparedImportedClip {
    readonly library: 'imported';
    readonly scope: 'scene' | 'prop';
    readonly workspace: WorkspaceAssetScope;
    readonly option: AnimationTargetOption;
    readonly review: AnimationReview;
    readonly clipIndex: number;
}

interface PreparedAuthoredClip {
    readonly library: 'authored';
    readonly scope: 'scene' | 'prop';
    readonly document: ClipAssetDocument;
    readonly markerId: string;
}

type PreparedAnimationClip = PreparedImportedClip | PreparedAuthoredClip;

interface PreviewOwner {
    readonly key: string;
    readonly option: AnimationTargetOption;
    readonly generation: number;
    readonly revision: number;
}

interface ReviewedPreview {
    readonly owner: PreviewOwner;
    readonly review: AnimationReview;
}

interface OpenPreview {
    readonly context: ReviewedPreview;
    readonly clip: number;
    readonly request: number;
}

interface ScrubPreview {
    readonly preview: ScopedAnimationPreview;
    readonly generation: number;
    readonly seconds: number;
}

/** Retains at most one native aperture and one in-flight seek for the Animation tab. */
@Injectable()
export class AnimationWorkspaceService {
    private readonly editor = inject(EditorService);
    private readonly views = inject(WorkspaceViewService);
    private readonly props = inject(PropWorkspaceService);
    private readonly editing = inject(SceneEditingService);
    private readonly panes = inject(ScopedPaneRequests, {optional: true});
    private readonly viewport = inject(ViewportEditorService, {optional: true});
    private readonly preparedImport = signal<PreparedImportedClip | null>(null);
    private readonly pinnedScope = signal<'scene' | 'prop' | null>(null);
    private readonly pin = signal<ScopedAnimationPreview | null>(null);
    private readonly failure = signal<string | null>(null);
    private readonly status = signal<'loading' | 'opening' | null>(null);
    private readonly context = signal<ReviewedPreview | null>(null);
    private readonly targetKey = signal('');
    private readonly retryVersion = signal(0);
    private readonly time = signal(0);
    private destroyed = false;
    private openFlight = false;
    private seekFlight = false;
    private nextOpen: OpenPreview | null = null;
    private nextSeek: ScrubPreview | null = null;
    private request = 0;
    private reviewTimeline: (() => Promise<boolean>) | null = null;

    private readonly authoredReveal = signal<AuthoredClipReveal | null>(null);
    readonly authoredClipReveal = this.authoredReveal.asReadonly();
    readonly scope = signal<'scene' | 'prop'>('scene');
    readonly library = signal<'imported' | 'authored'>('imported');
    readonly authoredReady = signal(false);
    readonly clipIndex = signal(0);
    readonly nativeAvailable = this.editor.nativeAvailable;
    readonly propAvailable = computed(() => this.props.document() !== null);
    readonly options = computed<readonly AnimationTargetOption[]>(() => {
        const snapshot = this.editor.snapshot();
        if (!this.views.animationView() || !snapshot) return [];
        if (this.scope() === 'prop') {
            const document = this.props.document();
            return document
                ? propAnimationTargets(document, this.props.selectedObject(), this.props.selectedPlacement(), snapshot.active_scene_id)
                : [];
        }
        const option = sceneAnimationTarget(this.editing.entity(), snapshot.document.entities, snapshot.active_scene_id);
        return option ? [option] : [];
    });

    readonly target = computed(() =>
        this.options().find(option => option.key === this.targetKey()) ?? this.options()[0] ?? null,
    );
    readonly clips = computed(() => this.context()?.review.metadata.clips ?? []);
    readonly preview = computed(() => {
        const pin = this.pin();
        return pin && this.isActive(pin) ? pin : null;
    });
    readonly ready = computed(() => this.library() === 'authored' ? this.authoredReady() : this.preview() !== null);
    readonly loading = computed(() => this.status() === 'loading');
    readonly opening = computed(() => this.status() === 'opening');
    readonly seconds = this.time.asReadonly();
    readonly error = this.failure.asReadonly();

    private readonly owner = computed<PreviewOwner | null>(() => {
        const snapshot = this.editor.snapshot();
        if (!this.nativeAvailable || this.library() !== 'imported' || !this.views.animationView() || snapshot?.mode !== 'edit' ||
            snapshot.transaction !== null || this.editor.playing()) return null;
        const selection = this.target();
        if (!selection) return null;
        const generation = this.editor.workspaceGeneration();
        const prepared = this.preparedImport();
        const option = this.matchesPreparedImport(prepared, selection, generation, snapshot.revision) ? prepared.option : selection;
        return {
            option, generation, revision: snapshot.revision,
            key: JSON.stringify([generation, snapshot.revision, option.key, this.retryVersion()]),
        };
    }, {equal: (first, second) => first?.key === second?.key});

    constructor() {
        effect(() => {
            const origin = this.views.animationOrigin();
            const active = this.views.animationView();
            if (active) this.scope.set(this.pinnedScope() ?? origin);
            else untracked(() => {
                this.pinnedScope.set(null);
                this.preparedImport.set(null);
            });
        });
        effect(() => {
            if (!this.views.animationView() || this.library() !== 'authored') this.authoredReveal.set(null);
        });
        effect(() => {
            const owner = this.owner();
            untracked(() => this.synchronize(owner));
        });
        let unsubscribe: (() => void) | undefined;
        if (this.nativeAvailable) {
            void listen<unknown>('editor://prop-preview-error', ({payload}) => {
                if (typeof payload !== 'object' || payload === null) return;
                const error = payload as {token?: unknown; workspace_generation?: unknown; message?: unknown};
                const preview = this.pin();
                if (preview && error.token === preview.token && error.workspace_generation === preview.workspace_generation &&
                    typeof error.message === 'string') {
                    this.pin.set(null);
                    this.failure.set(error.message);
                }
            }).then(release => {
                if (this.destroyed) release();
                else unsubscribe = release;
            }).catch(() => undefined);
        }
        inject(DestroyRef).onDestroy(() => {
            this.destroyed = true;
            this.request = reserveAnimationPreviewRequest();
            this.nextOpen = null;
            this.nextSeek = null;
            this.releasePreview();
            unsubscribe?.();
        });
    }

    chooseScope(value: string): void {
        if (value !== 'scene' && value !== 'prop') return;
        if (value !== this.scope()) this.preparedImport.set(null);
        this.pinnedScope.set(value);
        this.scope.set(value);
    }

    openAuthored(): void {
        if (this.editor.playing()) return;
        this.library.set('authored');
        this.views.selectView('animation-workspace');
    }

    async openScoped(request: AnimationPaneRequest, resolve: () => ScopedPaneTarget | null): Promise<boolean> {
        if (!this.panes || !this.nativeAvailable || this.editor.playing()) return false;
        const origin = this.views.activeViewKind();
        const propOrigin = this.views.propView() || this.views.animationView() && this.scope() === 'prop';
        let activated = false;
        return this.panes.open<PreparedAnimationClip>({...request, pane: 'animation-workspace'}, {
            resolve: () => {
                if (this.destroyed || this.editor.playing() ||
                    this.views.activeViewKind() !== (activated ? 'animation-workspace' : origin)) return null;
                return resolve();
            },
            load: target => this.prepareScopedClip(request, target, propOrigin),
            activate: (prepared, _target, stillCurrent) => {
                if (!this.views.availableViews().includes('animation-workspace')) return false;
                this.views.selectView('animation-workspace');
                activated = true;
                this.chooseScope(prepared.scope);
                this.library.set(prepared.library);
                if (prepared.library === 'imported') {
                    this.preparedImport.set(prepared);
                    this.targetKey.set(prepared.option.key);
                    this.retryVersion.update(version => version + 1);
                } else {
                    this.authoredReveal.set({
                        document: prepared.document, markerId: prepared.markerId, openTimeline: true, current: stillCurrent,
                    });
                }
                return true;
            },
        });
    }

    private async prepareScopedClip(
        request: AnimationPaneRequest, target: ScopedPaneTarget, propOrigin: boolean,
    ): Promise<PreparedScopedPane<PreparedAnimationClip> | null> {
        const snapshot = this.editor.snapshot();
        const workspace = target.subject.scope;
        const asset = target.asset;
        if (!asset || !snapshot || snapshot.mode !== 'edit' || snapshot.revision !== workspace.revision) return null;
        try {
            if (request.library === 'authored') {
                const document = await this.loadAuthoredClip(asset.id, request.markerId, workspace);
                return document ? {
                    value: {library: 'authored', document, markerId: request.markerId ?? '',
                        scope: target.subject.address.kind === 'prop' || propOrigin ? 'prop' : 'scene'},
                    asset: {id: document.asset.asset_id, revision: document.asset.revision, fingerprint: document.asset.fingerprint},
                } : null;
            }
            const bound = boundAnimationTarget(target.subject.address, request.componentId, snapshot.active_scene_id,
                this.editing.entity(), snapshot.document.entities, this.props.document(), this.props.selectedObject(), propOrigin);
            const source = bound?.option.settings.source;
            if (!bound || !source || source.asset_id !== asset.id) return null;
            const review = await this.editor.inspectAnimation(asset.id, workspace.workspaceGeneration, workspace.revision);
            if (review.workspace_generation !== workspace.nativeGeneration || review.revision !== workspace.revision ||
                !sameAnimationSource(review.metadata.source, source) ||
                !review.metadata.clips.some(clip => clip.index === request.clipIndex)) return null;
            const revision = asset.revision === undefined ? undefined
                : this.editor.modelAssets()?.find(model => model.asset_id === asset.id)?.revision ?? undefined;
            return {
                value: {library: 'imported', ...bound, workspace, review, clipIndex: request.clipIndex},
                asset: {id: source.asset_id, fingerprint: source.expected_sha256, revision},
            };
        } catch {
            return null;
        }
    }

    async revealAuthoredClip(assetId: string, markerId: string): Promise<void> {
        const snapshot = this.editor.snapshot();
        if (!assetId || snapshot?.mode !== 'edit' || this.editor.playing()) return;
        const origin = this.views.activeViewKind();
        const prop = this.views.propView() ? this.props.document() : null;
        const subject = this.clipRevealSubject(prop);
        const generation = this.editor.workspaceGeneration();
        const nativeGeneration = this.viewport?.state()?.workspace_generation;
        if (nativeGeneration == null) return;
        const sameSubject = () => {
            const live = this.editor.snapshot();
            return !this.destroyed && live?.mode === 'edit' && !this.editor.playing() &&
                live.active_scene_id === snapshot.active_scene_id && this.editor.workspaceGeneration() === generation &&
                this.viewport?.state()?.workspace_generation === nativeGeneration &&
                (!prop || this.props.document() === prop) && this.clipRevealSubject(prop) === subject;
        };
        if (!await this.reviewNavigation() || !sameSubject() || this.views.activeViewKind() !== origin) return;
        const revision = this.editor.snapshot()!.revision;
        const current = () => sameSubject() && this.editor.snapshot()?.revision === revision;
        try {
            const document = await this.loadAuthoredClip(assetId, markerId, {workspaceGeneration: generation, nativeGeneration, revision});
            if (!document || !current() || this.views.activeViewKind() !== origin) return;
            this.authoredReveal.set({document, markerId, openTimeline: !!markerId, current});
            this.openAuthored();
        } catch {
            // The originating field already reports unavailable clip assets.
        }
    }

    private async loadAuthoredClip(
        id: string, markerId: string | undefined, scope: WorkspaceAssetScope,
    ): Promise<ClipAssetDocument | null> {
        const document = await this.editor.openAuthoredClip(id, scope.workspaceGeneration);
        return document?.asset.asset_id === id && document.workspace_generation === scope.nativeGeneration &&
            (!markerId || document.clip.markers?.some(marker => marker.id === markerId))
            ? document : null;
    }

    isCurrentClipReveal(request: AuthoredClipReveal): boolean {
        return this.authoredReveal() === request && this.library() === 'authored' &&
            this.views.animationView() && request.current();
    }

    finishClipReveal(request: AuthoredClipReveal): void {
        if (this.authoredReveal() === request) this.authoredReveal.set(null);
    }

    private clipRevealSubject(prop: PropWorkspaceDocument | null): string | null {
        if (!prop) return this.editing.entity()?.key ?? null;
        const object = this.props.selectedObject();
        const placement = this.props.selectedPlacement();
        return object ? `object:${object.key}` : placement ? `placement:${placement.id}` : null;
    }

    registerTimelineReview(review: () => Promise<boolean>): () => void {
        this.reviewTimeline = review;
        return () => {
            if (this.reviewTimeline === review) this.reviewTimeline = null;
        };
    }

    reviewNavigation(): Promise<boolean> {
        return this.reviewTimeline?.() ?? Promise.resolve(true);
    }

    async selectLibrary(library: 'imported' | 'authored'): Promise<void> {
        if (library === this.library() || !await this.reviewNavigation() || !this.views.animationView()) return;
        this.library.set(library);
    }

    chooseTarget(key: string): void {
        if (this.options().some(option => option.key === key)) this.targetKey.set(key);
    }

    chooseClip(value: number): void {
        const context = this.context();
        if (!context || !this.current(context.owner) || !this.clips().some(clip => clip.index === value)) return;
        if (value !== this.clipIndex()) this.openClip(context, value);
    }

    retry(): void {
        this.retryVersion.update(version => version + 1);
    }

    seek(seconds: number): void {
        const preview = this.pin();
        const owner = this.owner();
        if (!preview || !owner || !this.isActive(preview) || !Number.isFinite(seconds) || seconds < 0 || seconds > preview.duration_seconds) return;
        this.time.set(seconds);
        this.nextSeek = {preview, generation: owner.generation, seconds};
        void this.drainSeeks();
    }

    async frame(): Promise<void> {
        const preview = this.pin();
        if (!preview || !this.owner()) return;
        try {
            await this.editor.frameAnimationWorkspacePreview(preview.token);
        } catch (error) {
            if (this.pin()?.token === preview.token) this.failure.set(String(error));
        }
    }

    private synchronize(owner: PreviewOwner | null): void {
        this.request = reserveAnimationPreviewRequest();
        this.nextOpen = null;
        this.nextSeek = null;
        this.releasePreview();
        this.context.set(null);
        this.failure.set(null);
        this.time.set(0);
        this.status.set(owner ? 'loading' : null);
        if (!owner) return;
        const prepared = this.preparedImport();
        if (this.matchesPreparedImport(prepared, owner.option, owner.generation, owner.revision)) {
            const context = {owner, review: prepared.review};
            this.context.set(context);
            this.openClip(context, prepared.clipIndex);
        } else void this.inspect(owner);
    }

    private matchesPreparedImport(
        prepared: PreparedImportedClip | null, option: AnimationTargetOption, generation: number, revision: number,
    ): prepared is PreparedImportedClip {
        return prepared !== null && prepared.option.key === option.key &&
            prepared.workspace.workspaceGeneration === generation && prepared.workspace.revision === revision &&
            prepared.workspace.nativeGeneration === this.viewport?.state()?.workspace_generation;
    }

    private async inspect(owner: PreviewOwner): Promise<void> {
        try {
            const expected = owner.option.settings.source;
            const review = await this.editor.inspectAnimation(expected.asset_id, owner.generation, owner.revision);
            if (!this.current(owner)) return;
            if (!sameAnimationSource(review.metadata.source, expected)) {
                throw new Error('The imported animation source changed. Review it in the Animation component.');
            }
            const context = {owner, review};
            this.context.set(context);
            const playback = owner.option.settings.playback;
            const configured = playback.kind === 'single_clip' ? playback.clip_index : playback.idle;
            const clip = review.metadata.clips.find(item => item.index === configured) ?? review.metadata.clips[0];
            this.openClip(context, clip.index);
        } catch (error) {
            if (this.current(owner)) {
                this.failure.set(String(error));
                this.status.set(null);
            }
        }
    }

    private openClip(context: ReviewedPreview, clip: number): void {
        this.releasePreview();
        this.nextSeek = null;
        this.time.set(0);
        this.failure.set(null);
        this.status.set('opening');
        this.clipIndex.set(clip);
        this.request = reserveAnimationPreviewRequest();
        this.nextOpen = {context, clip, request: this.request};
        void this.drainOpens();
    }

    private async drainOpens(): Promise<void> {
        if (this.openFlight) return;
        this.openFlight = true;
        try {
            while (this.nextOpen && !this.destroyed) {
                const {context, clip, request} = this.nextOpen;
                this.nextOpen = null;
                const {owner, review} = context;
                const current = () => this.current(owner) && request === this.request;
                if (!current()) continue;
                try {
                    const preview = await this.editor.openAnimationWorkspacePreview(
                        owner.option.target, clip, owner.generation, owner.revision,
                        review.workspace_generation, request,
                    );
                    if (!current()) {
                        this.closeToken(preview.token);
                        continue;
                    }
                    if (preview.request_id !== request || preview.clip_index !== clip ||
                        preview.revision !== owner.revision || preview.workspace_generation !== review.workspace_generation ||
                        !matchesAnimationTarget(preview.target, owner.option.target)) {
                        this.closeToken(preview.token);
                        throw new Error('Animation preview returned a different target.');
                    }
                    this.pin.set(preview);
                } catch (error) {
                    if (current()) this.failure.set(String(error));
                } finally {
                    if (current()) this.status.set(null);
                }
            }
        } finally {
            this.openFlight = false;
        }
    }

    private async drainSeeks(): Promise<void> {
        if (this.seekFlight) return;
        this.seekFlight = true;
        try {
            while (this.nextSeek && !this.destroyed) {
                const {preview, generation, seconds} = this.nextSeek;
                this.nextSeek = null;
                if (!this.isActive(preview)) continue;
                try {
                    const result = await this.editor.sampleAnimationWorkspacePreview(preview.token, seconds, generation);
                    if (!this.isActive(preview) || this.nextSeek) continue;
                    if (result.token !== preview.token || result.request_id !== preview.request_id ||
                        result.revision !== preview.revision || result.workspace_generation !== preview.workspace_generation ||
                        result.clip_index !== preview.clip_index || !matchesAnimationTarget(result.target, preview.target)) {
                        throw new Error('Animation sample returned a different target.');
                    }
                    this.pin.set(result);
                    this.time.set(result.time_seconds);
                    this.failure.set(null);
                } catch (error) {
                    if (this.isActive(preview) && !this.nextSeek) this.failure.set(String(error));
                }
            }
        } finally {
            this.seekFlight = false;
        }
    }

    private current(owner: PreviewOwner): boolean {
        return !this.destroyed && owner.key === this.owner()?.key;
    }

    private isActive(preview: ScopedAnimationPreview): boolean {
        const owner = this.owner();
        return !this.destroyed && owner !== null && this.pin()?.token === preview.token &&
            preview.revision === owner.revision && matchesAnimationTarget(preview.target, owner.option.target);
    }

    private releasePreview(): void {
        const token = this.pin()?.token;
        this.pin.set(null);
        if (token) this.closeToken(token);
    }

    private closeToken(token: string): void {
        void this.editor.closeAnimationPreview(token).catch(() => undefined);
    }
}

function sameAnimationSource(first: AnimationSource, second: AnimationSource): boolean {
    return first.asset_id === second.asset_id && first.expected_sha256 === second.expected_sha256 &&
        first.scene_index === second.scene_index && first.mesh_node === second.mesh_node && first.skin_index === second.skin_index;
}
