import {Component, computed, effect, inject, signal, untracked} from '@angular/core';
import {DatePipe} from '@angular/common';
import type {RecoveryReview} from '../../../core/editor/document-persistence.models';
import {EditorService} from '../../../core/editor/editor.service';
import {EditorDialog} from '../../../shared/components/dialog/dialog';

const CHECKPOINT_INTERVAL_MS = 15_000;

@Component({
    selector: 'editor-document-recovery',
    imports: [DatePipe, EditorDialog],
    templateUrl: './document-recovery.html',
})
export class EditorDocumentRecovery {
    private readonly editor = inject(EditorService);
    private readonly ready = computed(() => this.editor.capabilities()?.document_recovery_supported === true &&
        this.editor.snapshot() !== null);
    private readonly review = signal<RecoveryReview | null>(null);
    protected readonly offer = computed(() => this.review()?.offer ?? null);
    protected readonly open = signal(false);
    protected readonly error = signal<string | null>(null);
    protected readonly applying = signal(false);
    private checkpointPending = false;

    constructor() {
        effect(onCleanup => {
            const generation = this.editor.workspaceGeneration();
            if (!this.ready()) return;
            this.review.set(null);
            this.open.set(false);
            this.error.set(null);
            untracked(() => void this.reviewDrafts(generation));
            const timer = setInterval(() => void this.checkpoint(generation), CHECKPOINT_INTERVAL_MS);
            onCleanup(() => clearInterval(timer));
        });
    }

    protected async reviewDrafts(generation = this.editor.workspaceGeneration()): Promise<void> {
        try {
            const review = await this.editor.reviewDocumentRecovery(generation);
            if (generation !== this.editor.workspaceGeneration()) return;
            this.review.set(review);
            this.open.set(review.offer !== null);
            this.error.set(null);
        } catch (error) {
            if (generation === this.editor.workspaceGeneration()) this.report(error);
        }
    }

    protected async apply(restore: boolean): Promise<void> {
        const generation = this.editor.workspaceGeneration();
        const review = this.review();
        if (!review?.offer || this.applying()) return;
        this.applying.set(true);
        try {
            await this.editor.applyDocumentRecovery(generation, review, restore);
            if (generation !== this.editor.workspaceGeneration()) return;
            this.review.set({...review, offer: null});
            this.open.set(false);
            this.error.set(null);
        } catch (error) {
            if (generation === this.editor.workspaceGeneration()) this.report(error);
        } finally {
            this.applying.set(false);
        }
    }

    private async checkpoint(generation: number): Promise<void> {
        const review = this.review();
        const snapshot = this.editor.snapshot();
        if (generation !== this.editor.workspaceGeneration() || !review || review.offer ||
            this.checkpointPending || !snapshot?.dirty || snapshot.transaction || snapshot.mode !== 'edit') return;
        this.checkpointPending = true;
        try {
            const published = await this.editor.checkpointDocuments(review.workspace_generation);
            if (published && generation === this.editor.workspaceGeneration()) this.error.set(null);
        } catch (error) {
            if (generation === this.editor.workspaceGeneration()) this.report(error);
        } finally {
            this.checkpointPending = false;
        }
    }

    private report(error: unknown): void {
        this.error.set(error instanceof Error ? error.message : String(error));
    }
}
