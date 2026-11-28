import {signal} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import {vi} from 'vitest';
import type {SaveFileReport} from '../../../../core/editor/document-persistence.models';
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
    const snapshot = signal<Pick<EditorSnapshot, 'mode' | 'transaction' | 'active_scene_id' | 'revision' | 'can_undo' | 'can_redo'>>({
        mode: 'edit', transaction: null, active_scene_id: 'main', revision: 7, can_undo: false, can_redo: false,
    });
    const generation = signal(2);
    const execute = vi.fn(async () => undefined);
    const dirty = signal(false);
    const savedFiles = signal<readonly SaveFileReport[]>([]);
    const save = vi.fn(async () => undefined);
    const clearSaveReport = vi.fn(() => savedFiles.set([]));
    TestBed.configureTestingModule({
        providers: [
            WorkspaceDraftReview,
            {
                provide: EditorService, useValue: {
                    nativeAvailable: true, snapshot, workspaceGeneration: generation,
                    busy: () => false, playing: () => snapshot().mode === 'play', savedFiles, execute,
                    dirty, selection: () => [], saveAll: save, clearSaveReport,
                }
            },
            {provide: SceneEditingService, useValue: {ready: () => true, hasChanges: () => false, withCommittedEdit}},
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
    return {
        fixture,
        root,
        snapshot,
        generation,
        execute,
        selectView,
        play,
        withCommittedEdit,
        dirty,
        savedFiles,
        save,
        clearSaveReport
    };
}


describe('Workspace Toolbar presentation', () => {
    it('keeps compact edit actions accessible and announces the authoritative save state', async () => {
        const {
            fixture,
            root,
            dirty,
            snapshot,
            save,
            savedFiles,
            clearSaveReport,
            execute
        } = await setup(async () => true);
        fixture.componentRef.setInput('sceneActionsVisible', true);
        await fixture.whenStable();

        const action = (name: string) => root.querySelector<HTMLButtonElement>(`button[aria-label="${name}"]`)!;
        expect(action('Save Scene').querySelector('editor-icon')).not.toBeNull();
        expect(action('Save Scene').title).toContain('Ctrl/Cmd + S');
        expect(action('Save Scene').disabled).toBe(true);
        expect(action('Undo').disabled).toBe(true);
        expect(action('Undo').title).toContain('History is at the oldest edit');
        expect(action('Redo').getAttribute('aria-keyshortcuts')).toContain('Control+Shift+Z');
        expect(action('Duplicate').disabled).toBe(true);
        expect(action('Delete').getAttribute('aria-keyshortcuts')).toBe('Delete');
        expect(action('Frame Selection').title).toContain('Scene view');
        expect(action('Commands').getAttribute('aria-haspopup')).toBe('dialog');

        dirty.set(true);
        snapshot.update(current => ({...current, can_undo: true}));
        await fixture.whenStable();
        expect(root.querySelector('.save-state')?.textContent?.trim()).toBe('Unsaved changes');
        expect(action('Save All').disabled).toBe(false);
        action('Save All').click();
        await fixture.whenStable();
        expect(save).toHaveBeenCalledOnce();

        savedFiles.set([{path: 'scenes/maze.json', outcome: 'conflict', message: 'The file changed on disk.'}]);
        await fixture.whenStable();
        const summary = root.querySelector<HTMLButtonElement>('.save-summary')!;
        expect(summary.textContent?.trim()).toBe('1 file needs attention');
        expect(summary.getAttribute('popovertarget')).toBe('workspace-save-results');
        expect(root.querySelector('#workspace-save-results')?.textContent).toContain('scenes/maze.json');
        root.querySelector<HTMLButtonElement>('#workspace-save-results button')!.click();
        await fixture.whenStable();
        expect(clearSaveReport).toHaveBeenCalledOnce();
        expect(root.querySelector('#workspace-save-results')).toBeNull();
        expect(document.activeElement).toBe(root.querySelector('.play-action'));

        action('Undo').click();
        await fixture.whenStable();
        expect(execute).toHaveBeenCalledWith({type: 'undo'});
    });

    it('keeps one preview control while switching between Play and Stop', async () => {
        const {fixture, root, snapshot, execute} = await setup(async () => true);
        const button = root.querySelector<HTMLButtonElement>('.play-action')!;
        expect(button.textContent?.trim()).toBe('Play');
        snapshot.update(current => ({...current, mode: 'play'}));
        await fixture.whenStable();
        expect(root.querySelector('.play-action')).toBe(button);
        expect(button.textContent?.trim()).toBe('Stop');
        button.click();
        await fixture.whenStable();
        expect(execute).toHaveBeenCalledWith({type: 'stop_play'});
    });
});

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
