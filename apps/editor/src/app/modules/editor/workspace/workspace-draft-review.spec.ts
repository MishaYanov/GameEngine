import {signal} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import {vi} from 'vitest';
import {EditorService} from '../../../core/editor/editor.service';
import {WorkspaceDraftReview} from './workspace-draft-review';

function setup() {
    const generation = signal(2);
    const snapshot = signal({active_scene_id: 'main', revision: 10});
    TestBed.configureTestingModule({
        providers: [
            WorkspaceDraftReview,
            {provide: EditorService, useValue: {workspaceGeneration: generation, snapshot}},
        ]
    });
    return {review: TestBed.inject(WorkspaceDraftReview), generation, snapshot};
}

describe('WorkspaceDraftReview', () => {
    it('holds the review and action together so a concurrent transition cannot reuse the approval', async () => {
        const {review, snapshot} = setup();
        let decide!: (approved: boolean) => void;
        const first = vi.fn(() => new Promise<boolean>(resolve => decide = resolve));
        const second = vi.fn(() => true);
        let finish!: () => void;
        const action = vi.fn(() => new Promise<void>(resolve => finish = resolve));
        review.setReviewers([first, second]);

        const running = review.run(review.capture(), action);
        expect(review.pending()).toBe(true);
        expect(await review.run(review.capture(), action)).toBe(false);
        expect(first).toHaveBeenCalledOnce();
        expect(second).not.toHaveBeenCalled();
        snapshot.update(value => ({...value, revision: 11}));
        decide(true);
        await vi.waitFor(() => expect(action).toHaveBeenCalledOnce());
        expect(second).toHaveBeenCalledOnce();
        expect(await review.run(review.capture(), action)).toBe(false);
        finish();
        expect(await running).toBe(true);
        expect(review.pending()).toBe(false);
    });

    it.each(['scene', 'workspace', 'cancel'])('stops before a later reviewer when a pending review sees %s', async change => {
        const {review, generation, snapshot} = setup();
        let decide!: (approved: boolean) => void;
        const second = vi.fn(() => true);
        const action = vi.fn(async () => undefined);
        review.setReviewers([() => new Promise<boolean>(resolve => decide = resolve), second]);

        const running = review.run(review.capture(), action);
        if (change === 'scene') snapshot.set({active_scene_id: 'other', revision: 11});
        if (change === 'workspace') generation.set(3);
        decide(change !== 'cancel');
        expect(await running).toBe(false);
        expect(second).not.toHaveBeenCalled();
        expect(action).not.toHaveBeenCalled();
        expect(review.pending()).toBe(false);
    });
});
