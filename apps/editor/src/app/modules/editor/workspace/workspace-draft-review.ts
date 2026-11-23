import {DestroyRef, inject, Injectable, signal} from '@angular/core';
import type {SceneId} from '../../../core/editor/editor.models';
import {EditorService} from '../../../core/editor/editor.service';

export interface WorkspaceDraftTarget {
    readonly generation: number;
    readonly sceneId: SceneId | null;
}

type DraftReviewer = () => boolean | Promise<boolean>;

@Injectable()
export class WorkspaceDraftReview {
    private readonly editor = inject(EditorService);
    private readonly reviewing = signal(false);
    readonly pending = this.reviewing.asReadonly();
    private reviewers: readonly DraftReviewer[] = [];
    private destroyed = false;

    constructor() {
        inject(DestroyRef).onDestroy(() => this.destroyed = true);
    }

    setReviewers(reviewers: readonly DraftReviewer[]): void {
        this.reviewers = reviewers;
    }

    capture(): WorkspaceDraftTarget {
        return {
            generation: this.editor.workspaceGeneration(),
            sceneId: this.editor.snapshot()?.active_scene_id ?? null,
        };
    }

    current(target: WorkspaceDraftTarget): boolean {
        return !this.destroyed && target.generation === this.editor.workspaceGeneration() &&
            target.sceneId === (this.editor.snapshot()?.active_scene_id ?? null);
    }

    /** Review local drafts before the action takes its scene-edit reservation. */
    async run(target: WorkspaceDraftTarget, action: () => Promise<void> = async () => undefined): Promise<boolean> {
        if (this.pending() || !this.current(target)) return false;
        this.reviewing.set(true);
        try {
            for (const review of this.reviewers) {
                if (!await review() || !this.current(target)) return false;
            }
            await action();
            return true;
        } finally {
            this.reviewing.set(false);
        }
    }
}
