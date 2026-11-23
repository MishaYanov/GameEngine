import {signal} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import {vi} from 'vitest';
import {EditorService} from '../../../core/editor/editor.service';
import {
    type OpenPaneRequest, type PreparedScopedPane, ScopedPaneRequests, type ScopedPaneTarget,
} from './scoped-pane-request';
import {WorkspaceDraftReview} from './workspace-draft-review';

function deferred<T>() {
    let resolve!: (value: T) => void;
    const pending = new Promise<T>(accept => resolve = accept);
    return {pending, resolve};
}

const request: OpenPaneRequest = {
    pane: 'animation-controller',
    subject: {
        address: {kind: 'scene', scene: 'arena', object: 'guard'},
        scope: {workspaceGeneration: 4, nativeGeneration: 9, revision: 14},
        contentRevision: 7,
        access: 'scene-edit',
    },
    componentId: 'animation',
    asset: {id: 'guard-controller'},
};
const asset = {id: 'guard-controller', revision: 3, fingerprint: 'sha-3'};

function setup() {
    const workspaceGeneration = signal(4);
    const snapshot = signal({active_scene_id: 'arena', revision: 14});
    const target = signal<ScopedPaneTarget>({...request, asset});
    TestBed.configureTestingModule({
        providers: [
            WorkspaceDraftReview,
            ScopedPaneRequests,
            {provide: EditorService, useValue: {workspaceGeneration, snapshot}},
        ],
    });
    const resolve = vi.fn(() => target());
    const load = vi.fn(async (_target: ScopedPaneTarget): Promise<PreparedScopedPane<string> | null> => ({
        value: 'controller',
        asset,
    }));
    const activate = vi.fn((_value: string, _target: ScopedPaneTarget, _stillCurrent: () => boolean) => true);
    return {
        panes: TestBed.inject(ScopedPaneRequests), reviews: TestBed.inject(WorkspaceDraftReview),
        workspaceGeneration, snapshot, target, transition: {resolve, load, activate},
    };
}

describe('ScopedPaneRequests', () => {
    it('loads after review with the current workspace revision and activates the exact destination', async () => {
        const {panes, reviews, target, transition} = setup();
        const save = deferred<boolean>();
        reviews.setReviewers([() => save.pending]);

        const opening = panes.open(request, transition);
        expect(transition.load).not.toHaveBeenCalled();
        target.update(current => ({
            ...current, subject: {...current.subject, scope: {...current.subject.scope, revision: 15}},
        }));
        save.resolve(true);

        expect(await opening).toBe(true);
        expect(transition.load).toHaveBeenCalledExactlyOnceWith(target());
        expect(transition.activate).toHaveBeenCalledExactlyOnceWith('controller', target(), expect.any(Function));
        const [, , stillCurrent] = transition.activate.mock.calls[0];
        expect(stillCurrent()).toBe(true);
        target.update(current => ({...current, componentId: 'different-component'}));
        expect(stillCurrent()).toBe(false);
    });

    it.each(['cancel', 'scene', 'workspace', 'native workspace', 'object', 'component', 'asset', 'content'])(
        'preserves the current pane when review sees %s', async change => {
            const {panes, reviews, workspaceGeneration, snapshot, target, transition} = setup();
            const review = deferred<boolean>();
            reviews.setReviewers([() => review.pending]);

            const opening = panes.open(request, transition);
            if (change === 'scene') snapshot.set({active_scene_id: 'annex', revision: 14});
            if (change === 'workspace') workspaceGeneration.set(5);
            if (change === 'native workspace') target.update(current => ({
                ...current, subject: {...current.subject, scope: {...current.subject.scope, nativeGeneration: 10}},
            }));
            if (change === 'object') target.update(current => ({
                ...current, subject: {...current.subject, address: {kind: 'scene', scene: 'arena', object: 'guard-copy'}},
            }));
            if (change === 'component') target.update(current => ({...current, componentId: 'new-animation'}));
            if (change === 'asset') target.update(current => ({...current, asset: {...asset, revision: 4}}));
            if (change === 'content') target.update(current => ({
                ...current, subject: {...current.subject, contentRevision: 8},
            }));
            review.resolve(change !== 'cancel');

            expect(await opening).toBe(false);
            expect(transition.load).not.toHaveBeenCalled();
            expect(transition.activate).not.toHaveBeenCalled();
        },
    );

    it('drops a loaded pane when its exact asset or preparation revision changes', async () => {
        const {panes, target, transition} = setup();
        const load = deferred<PreparedScopedPane<string>>();
        transition.load.mockReturnValueOnce(load.pending);

        const opening = panes.open(request, transition);
        expect(transition.load).toHaveBeenCalledOnce();
        target.update(current => ({
            ...current, subject: {...current.subject, scope: {...current.subject.scope, revision: 15}},
        }));
        load.resolve({value: 'controller', asset});

        expect(await opening).toBe(false);
        transition.load.mockResolvedValueOnce({value: 'wrong controller revision', asset: {...asset, fingerprint: 'sha-4'}});
        expect(await panes.open(request, transition)).toBe(false);
        expect(transition.activate).not.toHaveBeenCalled();
    });

    it('releases the review after a failed load without showing the destination', async () => {
        const {panes, reviews, transition} = setup();
        transition.load.mockRejectedValueOnce(new Error('Controller file is unavailable'));
        await expect(panes.open(request, transition)).rejects.toThrow('Controller file is unavailable');
        expect(transition.activate).not.toHaveBeenCalled();
        expect(reviews.pending()).toBe(false);
    });

    it('serializes transitions and prevents an earlier deferred focus after another request', async () => {
        const {panes, reviews, transition} = setup();
        const review = deferred<boolean>();
        reviews.setReviewers([() => review.pending]);
        const first = panes.open(request, transition);
        expect(await panes.open(request, transition)).toBe(false);
        review.resolve(true);
        expect(await first).toBe(true);

        const firstFocusGuard = transition.activate.mock.calls[0][2];
        const next = await panes.open({...request, pane: 'animation-workspace'}, transition);
        expect(next).toBe(true);
        const nextFocusGuard = transition.activate.mock.calls[1][2];
        expect(firstFocusGuard()).toBe(false);
        expect(nextFocusGuard()).toBe(true);
        panes.invalidate();
        expect(nextFocusGuard()).toBe(false);
    });
});
