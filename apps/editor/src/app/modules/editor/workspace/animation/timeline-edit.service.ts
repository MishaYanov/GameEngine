import {computed, DestroyRef, effect, inject, Injectable, signal, untracked} from '@angular/core';
import {listen} from '@tauri-apps/api/event';
import {type AnimationPreviewDocument, reserveAnimationPreviewRequest} from '../../../../core/editor/animation-workspace.models';
import type {ClipAssetDocument, ClipDraftOperation, ClipTimelineDraft, TimelineKeyMove, VisualClipPreview} from '../../../../core/editor/authored-clip.models';
import {EditorService} from '../../../../core/editor/editor.service';
import {AnimationWorkspaceService} from './animation-workspace.service';

interface AssetOwner {
    readonly id: string;
    readonly generation: number;
}

interface PreviewOwner {
    readonly key: string;
    readonly draft: ClipTimelineDraft;
    readonly generation: number;
    readonly revision: number;
    readonly scope: AnimationPreviewDocument | null;
}

interface Seek {
    readonly preview: VisualClipPreview;
    readonly generation: number;
    readonly tick: number;
    readonly drag: TimelineKeyMove | null;
}

@Injectable()
export class TimelineEditService {
    private readonly editor = inject(EditorService);
    private readonly animation = inject(AnimationWorkspaceService);
    private readonly owner = signal<AssetOwner | null>(null);
    private readonly scope = signal<AnimationPreviewDocument | null>(null, {
        equal: (a, b) => JSON.stringify(a) === JSON.stringify(b),
    });
    private destroyed = false;
    private frame: number | null = null;
    private seekFlight = false;
    private nextSeek: Seek | null = null;
    private request = 0;
    private dragOrigin: {at: number; playhead: number} | null = null;
    readonly draft = signal<ClipTimelineDraft | null>(null);
    readonly preview = signal<VisualClipPreview | null>(null);
    readonly playhead = signal(0);
    readonly loopStart = signal(0);
    readonly loopEnd = signal(0);
    readonly looping = signal(false);
    readonly snap = signal(false);
    readonly zoom = signal(1);
    readonly playing = signal(false);
    readonly busy = signal(false);
    readonly error = signal<string | null>(null);
    readonly drag = signal<TimelineKeyMove | null>(null);
    readonly duration = computed(() => this.draft()?.clip.duration_ticks ?? 0);

    private readonly previewOwner = computed<PreviewOwner | null>(() => {
        const draft = this.draft(), owner = this.owner(), snapshot = this.editor.snapshot();
        if (!draft || !owner || snapshot?.mode !== 'edit' || snapshot.transaction !== null ||
            owner.generation !== this.editor.workspaceGeneration()) return null;
        const scope = this.scope();
        return {
            draft, scope, generation: owner.generation, revision: snapshot.revision,
            key: JSON.stringify([draft.draft_id, draft.draft_revision, owner.generation, snapshot.revision, scope]),
        };
    }, {equal: (a, b) => a?.key === b?.key});

    constructor() {
        effect(() => {
            const owner = this.previewOwner();
            untracked(() => {
                this.releasePreview();
                if (owner) void this.openPreview(owner);
            });
        });
        let unsubscribe: (() => void) | undefined;
        if (this.editor.nativeAvailable) {
            void listen<unknown>('editor://prop-preview-error', ({payload}) => {
                const issue = payload as {token?: unknown; workspace_generation?: unknown; message?: unknown} | null;
                const preview = this.preview();
                if (preview && issue?.token === preview.token && issue.workspace_generation === preview.workspace_generation &&
                    typeof issue.message === 'string') {
                    this.releasePreview();
                    this.error.set(issue.message);
                }
            }).then(release => {
                if (this.destroyed) release();
                else unsubscribe = release;
            }).catch(() => undefined);
        }
        inject(DestroyRef).onDestroy(() => {
            this.destroyed = true;
            this.releasePreview();
            const draft = this.draft();
            if (draft && !draft.dirty) this.closeDraft(draft.draft_id);
            unsubscribe?.();
        });
    }

    connect(asset: ClipAssetDocument, scope: AnimationPreviewDocument | null): void {
        this.scope.set(scope);
        const generation = this.editor.workspaceGeneration();
        if (this.owner()?.id === asset.asset.asset_id && this.owner()?.generation === generation) return;
        const owner = {id: asset.asset.asset_id, generation};
        this.owner.set(owner);
        const snapshot = this.editor.snapshot();
        if (snapshot) void this.loadDraft(owner, snapshot.revision);
    }

    seek(tick: number): void {
        if (!Number.isFinite(tick)) return;
        this.playhead.set(this.normalized(tick));
        this.enqueueSeek();
    }

    play(): void {
        const end = this.looping() ? this.loopEnd() : this.duration();
        const start = this.looping() ? this.loopStart() : 0;
        if (this.playing() || !this.preview() || end <= start) return;
        const from = this.playhead() >= end || this.playhead() < start ? start : this.playhead();
        let started: number | null = null;
        this.playing.set(true);
        const advance = (now: number) => {
            if (!this.playing()) return;
            started ??= now;
            const next = from + Math.floor(now - started);
            const tick = this.looping() ? start + (next - start) % (end - start) : Math.min(end, next);
            this.playhead.set(tick);
            this.enqueueSeek();
            if (!this.looping() && next >= end) this.stop();
            else this.frame = requestAnimationFrame(advance);
        };
        this.frame = requestAnimationFrame(advance);
    }

    stop(): void {
        if (this.frame !== null) cancelAnimationFrame(this.frame);
        this.frame = null;
        this.playing.set(false);
    }

    setLoopStart(value: number): void {
        if (Number.isFinite(value)) this.loopStart.set(Math.max(0, Math.min(this.loopEnd() - 1, Math.round(value))));
        this.stop();
    }

    setLoopEnd(value: number): void {
        if (Number.isFinite(value)) this.loopEnd.set(Math.max(this.loopStart() + 1, Math.min(this.duration(), Math.round(value))));
        this.stop();
    }

    beginDrag(keyId: string, at: number): void {
        if (this.busy() || !this.draft()) return;
        this.stop();
        this.dragOrigin = {at, playhead: this.playhead()};
        this.drag.set({key_id: keyId, at});
    }

    moveDrag(tick: number): void {
        const drag = this.drag();
        if (!drag || !Number.isFinite(tick)) return;
        const at = this.normalized(tick);
        this.drag.set({...drag, at});
        this.playhead.set(at);
        this.enqueueSeek();
    }

    async acceptDrag(): Promise<void> {
        const drag = this.drag(), origin = this.dragOrigin;
        if (!drag || !origin) return;
        this.drag.set(null);
        this.dragOrigin = null;
        if (drag.at !== origin.at) await this.edit({type: 'edit', edit: {kind: 'move_key', ...drag}});
        this.enqueueSeek();
    }

    cancelDrag(): void {
        if (!this.drag()) return;
        this.drag.set(null);
        if (this.dragOrigin) this.playhead.set(this.dragOrigin.playhead);
        this.dragOrigin = null;
        this.enqueueSeek();
    }

    async edit(operation: ClipDraftOperation): Promise<void> {
        const draft = this.draft(), owner = this.owner();
        if (!draft || !owner || this.busy()) return;
        this.stop();
        await this.run(owner, async () => {
            const next = await this.editor.editClipTimelineDraft(draft.draft_id, draft.draft_revision, operation, owner.generation);
            if (this.current(owner)) this.draft.set(next);
        });
    }

    async save(): Promise<ClipTimelineDraft | null> {
        const draft = this.draft(), owner = this.owner(), snapshot = this.editor.snapshot();
        if (!draft || !owner || !snapshot || !draft.dirty || this.busy()) return null;
        this.stop();
        let saved: ClipTimelineDraft | null = null;
        await this.run(owner, async () => {
            const next = await this.editor.saveClipTimelineDraft(draft.draft_id, draft.draft_revision, owner.generation, snapshot.revision);
            if (this.current(owner)) {
                this.draft.set(next);
                saved = next;
            }
        });
        return saved;
    }

    async close(discard: boolean): Promise<boolean> {
        const draft = this.draft(), owner = this.owner();
        if (!draft || !owner) return true;
        if (this.busy()) return false;
        let closed = false;
        await this.run(owner, async () => {
            await this.editor.closeClipTimelineDraft(draft.draft_id, discard);
            if (this.current(owner)) {
                this.draft.set(null);
                this.releasePreview();
                closed = true;
            }
        });
        return closed;
    }

    private async loadDraft(owner: AssetOwner, revision: number): Promise<void> {
        await this.run(owner, async () => {
            const draft = await this.editor.openClipTimelineDraft(owner.id, owner.generation, revision);
            if (!this.current(owner)) {
                if (!draft.dirty) this.closeDraft(draft.draft_id);
                return;
            }
            this.draft.set(draft);
            this.loopEnd.set(draft.clip.duration_ticks);
        });
    }

    private async openPreview(owner: PreviewOwner): Promise<void> {
        const request = reserveAnimationPreviewRequest();
        this.request = request;
        const current = () => !this.destroyed && this.request === request && this.previewOwner()?.key === owner.key;
        try {
            const preview = await this.editor.openClipTimelinePreview({
                draftId: owner.draft.draft_id, expectedDraftRevision: owner.draft.draft_revision, scope: owner.scope,
                expectedGeneration: owner.draft.workspace_generation, expectedRevision: owner.revision, requestId: request,
            }, owner.generation);
            if (!current()) {
                this.closePreviewToken(preview.token);
                return;
            }
            if (preview.request_id !== request || preview.workspace_generation !== owner.draft.workspace_generation ||
                preview.revision !== owner.revision) {
                this.closePreviewToken(preview.token);
                throw new Error('The timeline returned a different preview owner.');
            }
            this.preview.set(preview);
            this.animation.authoredReady.set(true);
            this.enqueueSeek();
        } catch (error) {
            if (current()) this.error.set(String(error));
        }
    }

    private enqueueSeek(): void {
        const preview = this.preview(), owner = this.previewOwner();
        if (!preview || !owner || preview.revision !== owner.revision) return;
        this.nextSeek = {preview, generation: owner.generation, tick: this.playhead(), drag: this.drag()};
        void this.drainSeeks();
    }

    private async drainSeeks(): Promise<void> {
        if (this.seekFlight) return;
        this.seekFlight = true;
        try {
            while (this.nextSeek && !this.destroyed) {
                const seek = this.nextSeek;
                this.nextSeek = null;
                const current = () => !this.destroyed && this.preview()?.token === seek.preview.token &&
                    this.previewOwner()?.revision === seek.preview.revision;
                if (!current()) continue;
                try {
                    const next = await this.editor.sampleClipTimelinePreview(seek.preview.token, seek.tick, seek.drag, seek.generation);
                    if (current() && !this.nextSeek && next.token === seek.preview.token &&
                        next.request_id === seek.preview.request_id && next.revision === seek.preview.revision &&
                        next.workspace_generation === seek.preview.workspace_generation) this.preview.set(next);
                } catch (error) {
                    if (current() && !this.nextSeek && !seek.drag) this.error.set(String(error));
                }
            }
        } finally {
            this.seekFlight = false;
        }
    }

    private normalized(tick: number): number {
        const step = this.snap() ? 10 : 1;
        return Math.max(0, Math.min(this.duration(), Math.round(tick / step) * step));
    }

    private releasePreview(): void {
        this.stop();
        this.request = 0;
        this.nextSeek = null;
        const token = this.preview()?.token;
        this.preview.set(null);
        this.animation.authoredReady.set(false);
        if (token) this.closePreviewToken(token);
    }

    private closePreviewToken(token: string): void {
        void this.editor.closeAuthoredClipPreview(token).catch(() => undefined);
    }

    private closeDraft(draftId: string): void {
        void this.editor.closeClipTimelineDraft(draftId, false).catch(() => undefined);
    }

    private current(owner: AssetOwner): boolean {
        return !this.destroyed && this.owner() === owner && this.editor.workspaceGeneration() === owner.generation;
    }

    private async run(owner: AssetOwner, operation: () => Promise<void>): Promise<void> {
        this.busy.set(true);
        this.error.set(null);
        try {
            await operation();
        } catch (error) {
            if (this.current(owner)) this.error.set(String(error));
        } finally {
            this.busy.set(false);
        }
    }
}
