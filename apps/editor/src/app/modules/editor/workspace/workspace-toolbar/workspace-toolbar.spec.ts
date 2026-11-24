import {signal} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import {vi} from 'vitest';
import type {EditorSnapshot} from '../../../../core/editor/editor.models';
import {EditorService} from '../../../../core/editor/editor.service';
import {SceneEditingService} from '../scene-editing.service';
import {WorkspaceDraftReview} from '../workspace-draft-review';
import {WorkspaceToolbar} from './workspace-toolbar';

function deferred() {
    let resolve!: (value: boolean) => void;
    const promise = new Promise<boolean>(finish => {
        resolve = finish;
    });
    return {resolve, promise};
}

async function setup(
    reviewPlay: () => Promise<boolean>,
    withCommittedEdit = vi.fn(async (action: () => Promise<void>) => {
        await action();
        return true;
    }),
) {
    const snapshot = signal<Pick<EditorSnapshot, 'mode' | 'transaction' | 'active_scene_id' | 'revision'>>({
        mode: 'edit', transaction: null, active_scene_id: 'main', revision: 7,
    });
    const generation = signal(2);
    const execute = vi.fn(async () => undefined);
    TestBed.configureTestingModule({
        providers: [
            WorkspaceDraftReview,
            {
                provide: EditorService, useValue: {
                    nativeAvailable: true, snapshot, workspaceGeneration: generation,
                    busy: () => false, playing: () => false, savedFiles: () => [], execute,
                }
            },
            {provide: SceneEditingService, useValue: {ready: () => true, withCommittedEdit}},
        ]
    });
    TestBed.inject(WorkspaceDraftReview).setReviewers([reviewPlay]);
    const fixture = TestBed.createComponent(WorkspaceToolbar);
    fixture.componentRef.setInput('sceneActionsVisible', false);
    const selectView = vi.fn();
    fixture.componentInstance.viewSelected.subscribe(selectView);
    await fixture.whenStable();
    const root = fixture.nativeElement as HTMLElement;
    const play = () => root.querySelector<HTMLButtonElement>('.play-action')!.click();
    return {fixture, snapshot, generation, execute, selectView, play, withCommittedEdit};
}

describe('Workspace Toolbar Play review', () => {
    it.each(['project', 'scene'])('drops a deferred Play request when the %s changes during review', async changed => {
        const review = deferred();
        const reviewPlay = vi.fn(() => review.promise);
        const {fixture, snapshot, generation, execute, selectView, play, withCommittedEdit} = await setup(reviewPlay);

        play();
        expect(reviewPlay).toHaveBeenCalledTimes(1);
        if (changed === 'project') generation.set(3);
        else snapshot.update(current => ({...current, active_scene_id: 'other'}));
        review.resolve(true);
        await fixture.whenStable();

        expect(withCommittedEdit).not.toHaveBeenCalled();
        expect(execute).not.toHaveBeenCalled();
        expect(selectView).not.toHaveBeenCalled();
    });

    it('checks the Play target after waiting for an outstanding scene edit', async () => {
        const committed = deferred();
        const withCommittedEdit = vi.fn(async (action: () => Promise<void>) => {
            await committed.promise;
            await action();
            return true;
        });
        const {fixture, snapshot, execute, selectView, play} = await setup(async () => true, withCommittedEdit);

        play();
        await vi.waitFor(() => expect(withCommittedEdit).toHaveBeenCalledOnce());
        snapshot.update(current => ({...current, active_scene_id: 'other'}));
        committed.resolve(true);
        await fixture.whenStable();

        expect(execute).not.toHaveBeenCalled();
        expect(selectView).not.toHaveBeenCalled();
    });

    it('starts Play with the current revision after saving a timeline in the same scene', async () => {
        const review = deferred();
        const {fixture, snapshot, execute, selectView, play} = await setup(() => review.promise);

        play();
        snapshot.update(current => ({...current, revision: 8}));
        review.resolve(true);
        await fixture.whenStable();

        expect(execute).toHaveBeenCalledExactlyOnceWith({type: 'start_play'}, 2, 8);
        expect(selectView).toHaveBeenCalledExactlyOnceWith('game-preview');
    });
});
