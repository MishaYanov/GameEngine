import {computed, DestroyRef, effect, inject, Injectable, signal, untracked} from '@angular/core';
import {listen} from '@tauri-apps/api/event';
import type {AnimationReview, AnimationSource} from '../../../../core/editor/animation.models';
import {
    matchesAnimationTarget, reserveAnimationPreviewRequest, type ScopedAnimationPreview,
} from '../../../../core/editor/animation-workspace.models';
import {EditorService} from '../../../../core/editor/editor.service';
import {PropWorkspaceService} from '../prop-workspace/prop-workspace.service';
import {SceneEditingService} from '../scene-editing.service';
import {WorkspaceViewService} from '../scene-viewport/workspace-view.service';
import {type AnimationTargetOption, propAnimationTargets, sceneAnimationTarget} from './animation-workspace-target';

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
        const option = this.target();
        if (!option) return null;
        const generation = this.editor.workspaceGeneration();
        return {
            option, generation, revision: snapshot.revision,
            key: JSON.stringify([generation, snapshot.revision, option.key, this.retryVersion()]),
        };
    }, {equal: (first, second) => first?.key === second?.key});

    constructor() {
        effect(() => {
            const origin = this.views.animationOrigin();
            const active = this.views.animationView();
            if (active) untracked(() => this.chooseScope(origin));
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
        if (value === 'scene' || value === 'prop') this.scope.set(value);
    }

    openAuthored(): void {
        if (this.editor.playing()) return;
        this.library.set('authored');
        this.views.selectView('animation-workspace');
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
        if (owner) void this.inspect(owner);
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
