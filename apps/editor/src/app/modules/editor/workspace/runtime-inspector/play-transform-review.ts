import {Component, computed, inject, linkedSignal, signal} from '@angular/core';
import type {EntityKey, PlayTransformReviewDto, Transform} from '../../../../core/editor/editor.models';
import {EditorService} from '../../../../core/editor/editor.service';
import {ViewportEditorService} from '../../../../core/editor/viewport-editor.service';
import {EditorDialog} from '../../../../shared/components/dialog/dialog';

@Component({
    selector: 'editor-play-transform-review',
    imports: [EditorDialog],
    templateUrl: './play-transform-review.html',
    styleUrl: './play-transform-review.scss',
})
export class PlayTransformReview {
    private readonly editor = inject(EditorService);
    private readonly viewport = inject(ViewportEditorService);
    protected readonly supported = computed(() => this.editor.capabilities()?.commands?.some(
        capability => capability.name === 'apply_play_transforms') ?? false);
    protected readonly ready = computed(() =>
        this.editor.snapshot()?.mode === 'play' && this.viewport.state()?.workspace_generation != null &&
        !this.editor.busy() && !this.pending());
    protected readonly pending = signal(false);
    protected readonly error = signal<string | null>(null);
    protected readonly selected = signal<ReadonlySet<EntityKey>>(new Set());
    protected readonly review = linkedSignal({
        source: () => JSON.stringify([this.editor.workspaceGeneration(), this.viewport.gameStatus()?.run_id]),
        computation: (): PlayTransformReviewDto | null => null,
    });
    protected readonly canApply = computed(() => {
        const review = this.review();
        const snapshot = this.editor.snapshot();
        return this.ready() && !!review && snapshot?.paused === true &&
            snapshot.revision === review.revision && snapshot.scene_revision === review.scene_revision &&
            snapshot.active_scene_id === review.scene_id && this.selected().size > 0;
    });

    protected async capture(): Promise<void> {
        const snapshot = this.editor.snapshot();
        const generation = this.editor.workspaceGeneration();
        const native = this.viewport.state()?.workspace_generation;
        if (!this.supported() || !this.ready() || !snapshot || native == null) return;
        this.pending.set(true);
        this.error.set(null);
        try {
            const paused = snapshot.paused ? snapshot :
                await this.editor.execute({type: 'set_paused', paused: true}, generation, snapshot.revision);
            const result = await this.editor.reviewPlayTransforms(native, paused.revision);
            if (this.editor.workspaceGeneration() !== generation || this.editor.snapshot()?.revision !== result.revision) return;
            this.review.set(result);
            this.selected.set(new Set());
        } catch (error) {
            if (this.editor.workspaceGeneration() === generation) this.report(error);
        } finally {
            this.pending.set(false);
        }
    }

    protected toggle(key: EntityKey, checked: boolean): void {
        this.selected.update(previous => {
            const next = new Set(previous);
            if (checked) next.add(key);
            else next.delete(key);
            return next;
        });
    }

    protected async apply(): Promise<void> {
        const review = this.review();
        if (!review || !this.canApply()) return;
        const generation = this.editor.workspaceGeneration();
        this.pending.set(true);
        this.error.set(null);
        try {
            await this.editor.execute({type: 'apply_play_transforms', run_id: review.run_id,
                scene_revision: review.scene_revision, keys: [...this.selected()]}, generation, review.revision);
            this.close();
        } catch (error) {
            if (this.editor.workspaceGeneration() === generation) this.report(error);
        } finally {
            this.pending.set(false);
        }
    }

    protected close(): void {
        this.review.set(null);
        this.selected.set(new Set());
        this.error.set(null);
    }

    protected transform(value: Transform): string {
        const vector = (items: readonly number[]) => items.map(item => Number(item.toFixed(3))).join(', ');
        return 'Position ' + vector(value.position) + ' · Rotation ' +
            vector(value.rotation.map(item => item * 180 / Math.PI)) + '° · Scale ' + vector(value.scale);
    }

    private report(error: unknown): void {
        this.error.set(error instanceof Error ? error.message :
            typeof error === 'string' ? error : 'Play transforms could not be reviewed.');
    }
}
