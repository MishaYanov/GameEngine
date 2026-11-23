import {signal} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import {vi} from 'vitest';
import type {
    ClipDraftOperation, ClipTimelineDraft, ClipTimelinePreviewRequest, TimelineEdit, TimelineKeyMove, VisualClipPreview,
} from '../../../../core/editor/authored-clip.models';
import {EditorService} from '../../../../core/editor/editor.service';
import {AnimationWorkspaceService} from './animation-workspace.service';
import {ClipTimeline} from './timeline';
import {TimelineEditService} from './timeline-edit.service';

vi.mock('@tauri-apps/api/event', () => ({listen: vi.fn(async () => () => undefined)}));

const initial: ClipTimelineDraft = {
    draft_id: 'draft-clip', draft_revision: 0, can_undo: false, can_redo: false, dirty: false, workspace_generation: 11,
    asset: {asset_id: 'lever-clip', path: 'lever-turn.clip.json', revision: 1, fingerprint: 'a'.repeat(64)},
    clip: {
        schema_version: 2, name: 'Lever Turn', duration_ticks: 1000, owner: {kind: 'prop', asset_id: 'lever-prop'},
        tracks: [{
            target: {kind: 'prop', address: {kind: 'object', placement_path: [], source_entity: 'lever'}},
            track: {kind: 'local_position', interpolation: 'linear', keys: [
                {id: 'start', at: 0, value: [0, 0, 0]}, {id: 'end', at: 1000, value: [2, 0, 0]},
            ]},
        }],
    },
};

const timedDraft: ClipTimelineDraft = {
    ...initial,
    clip: {
        ...initial.clip,
        tracks: [{
            target: initial.clip.tracks[0].target,
            track: {kind: 'local_position', interpolation: 'linear', keys: [
                {id: 'start', at: 0, value: [0, 0, 0]},
                {id: 'middle', at: 333, value: [1, 0, 0]},
                {id: 'end', at: 1000, value: [2, 0, 0]},
            ]},
        }],
        markers: [
            {id: 'open', at: 79, label: 'Half-open'},
            {id: 'locked', at: 876, label: 'Locked'},
        ],
    },
};

function setup(draft = initial) {
    let revision = 0, cursor = 0;
    let history = [draft.clip];
    const showPreview = signal(false);
    const edit = vi.fn(async (_id: string, _expected: number, operation: ClipDraftOperation): Promise<ClipTimelineDraft> => {
        if (operation.type === 'undo') cursor--;
        if (operation.type === 'redo') cursor++;
        if (operation.type === 'edit') {
            history = history.slice(0, cursor + 1);
            history.push(apply(history[cursor], operation.edit));
            cursor++;
        }
        return {...initial, clip: history[cursor], dirty: cursor > 0,
            draft_revision: ++revision, can_undo: cursor > 0, can_redo: cursor + 1 < history.length};
    });
    const saveDraft = vi.fn(async () => ({
        ...initial, asset: {...initial.asset, revision: 2}, clip: history[cursor],
        draft_revision: ++revision, dirty: false,
    }));
    const closeDraft = vi.fn(async (_id: string, _discard: boolean) => undefined);
    let preview: VisualClipPreview;
    const open = vi.fn(async (request: ClipTimelinePreviewRequest) => {
        preview = {
            token: crypto.randomUUID(), workspace_generation: 11, revision: request.expectedRevision,
            duration_ticks: 1000, tick: 0, request_id: request.requestId, poses: [],
        };
        return preview;
    });
    const sample = vi.fn(async (_token: string, tick: number, _drag: TimelineKeyMove | null, _generation: number) =>
        ({...preview, tick}));
    const close = vi.fn(async (_token: string) => undefined);
    let review: (() => Promise<boolean>) | null = null;
    const reviewNavigation = () => review?.() ?? Promise.resolve(true);
    TestBed.configureTestingModule({providers: [
            {
                provide: AnimationWorkspaceService, useValue: {
                    authoredReady: showPreview,
                    registerTimelineReview: (callback: () => Promise<boolean>) => {
                        review = callback;
                        return () => {
                            review = null;
                        };
                    },
                }
            },
        {provide: EditorService, useValue: {
            nativeAvailable: true, snapshot: () => ({mode: 'edit', revision: 2, transaction: null}),
            workspaceGeneration: () => 1,
            openClipTimelineDraft: async () => structuredClone(draft),
            editClipTimelineDraft: edit,
            openClipTimelinePreview: open,
            sampleClipTimelinePreview: sample,
            closeAuthoredClipPreview: close,
            saveClipTimelineDraft: saveDraft,
            closeClipTimelineDraft: closeDraft,
        }},
    ]});
    const fixture = TestBed.createComponent(ClipTimeline);
    fixture.componentRef.setInput('asset', draft);
    return {
        fixture, edit, open, sample, close, saveDraft, closeDraft, showPreview, reviewNavigation,
        timeline: fixture.debugElement.injector.get(TimelineEditService)
    };
}

function apply(clip: ClipTimelineDraft['clip'], edit: TimelineEdit): ClipTimelineDraft['clip'] {
    if (edit.kind === 'add_marker') return {...clip, markers: [{id: 'bookmark', at: edit.at, label: edit.label}]};
    if (edit.kind === 'rename_marker') {
        return {...clip, markers: clip.markers?.map(marker => marker.id === edit.marker_id ? {...marker, label: edit.label} : marker)};
    }
    if (edit.kind === 'remove_key' || edit.kind === 'move_key') {
        const source = clip.tracks[0], track = source.track;
        if (track.kind === 'local_position') {
            const keys = edit.kind === 'remove_key' ? track.keys.filter(key => key.id !== edit.key_id)
                : track.keys.map(key => key.id === edit.key_id ? {...key, at: edit.at} : key);
            return {...clip, tracks: [{...source, track: {...track, keys}}]};
        }
    }
    return clip;
}

function button(root: HTMLElement, label: string): HTMLButtonElement {
    return Array.from(root.querySelectorAll('button')).find(button => button.textContent?.trim() === label)!;
}

function field(root: HTMLElement, label: string): HTMLInputElement {
    return Array.from(root.querySelectorAll('label')).find(item => item.firstChild?.textContent?.trim() === label)
        ?.querySelector('input') as HTMLInputElement;
}

describe('Authored clip timeline', () => {
    it('previews and cancels a drag without history, then commits exactly one move with undo and redo', async () => {
        const {fixture, timeline, edit, sample, open, showPreview} = setup();
        await fixture.whenStable();
        const root = fixture.nativeElement as HTMLElement;
        button(root, '1000 ms').click();
        await fixture.whenStable();
        const move = field(root, 'Move selected key (ms)');
        move.dispatchEvent(new Event('pointerdown'));
        move.value = '750';
        move.dispatchEvent(new Event('input'));
        await fixture.whenStable();
        expect(sample).toHaveBeenLastCalledWith(expect.any(String), 750, {key_id: 'end', at: 750}, 1);
        move.dispatchEvent(new KeyboardEvent('keydown', {key: 'Escape', bubbles: true}));
        await fixture.whenStable();
        expect(edit).not.toHaveBeenCalled();
        expect(timeline.playhead()).toBe(1000);
        expect(sample).toHaveBeenLastCalledWith(expect.any(String), 1000, null, 1);
        move.dispatchEvent(new Event('pointerdown'));
        move.value = '750';
        move.dispatchEvent(new Event('input'));
        move.dispatchEvent(new Event('change'));
        await fixture.whenStable();
        expect(edit).toHaveBeenCalledTimes(1);
        expect(edit).toHaveBeenCalledWith('draft-clip', 0, {
            type: 'edit', edit: {kind: 'move_key', key_id: 'end', at: 750},
        }, 1);
        expect(open.mock.calls.at(-1)?.[0].expectedDraftRevision).toBe(1);
        expect(showPreview()).toBe(true);
        button(root, 'Undo Key Edit').click();
        await fixture.whenStable();
        expect(button(root, '1000 ms')).toBeTruthy();
        button(root, 'Redo Key Edit').click();
        await fixture.whenStable();
        expect(button(root, '750 ms')).toBeTruthy();
    });

    it('handles keyboard insertion and deletion and keeps the same bookmark ID when renamed', async () => {
        const {fixture, edit, timeline} = setup();
        await fixture.whenStable();
        const root = fixture.nativeElement as HTMLElement;
        const lane = root.querySelector('.scroll-lane')!;
        timeline.seek(200);
        lane.dispatchEvent(new KeyboardEvent('keydown', {key: 'Insert', bubbles: true}));
        await fixture.whenStable();
        expect(edit).toHaveBeenCalledWith('draft-clip', 0, {
            type: 'edit', edit: {kind: 'insert_key', track: 0, at: 200, value: null},
        }, 1);
        lane.dispatchEvent(new KeyboardEvent('keydown', {key: 'Delete', bubbles: true}));
        await fixture.whenStable();
        expect(edit).toHaveBeenCalledWith('draft-clip', 1, {type: 'edit', edit: {kind: 'remove_key', key_id: 'start'}}, 1);
        const label = field(root, 'Bookmark label');
        label.value = 'Lever contact';
        label.dispatchEvent(new Event('input'));
        await fixture.whenStable();
        button(root, 'Add Bookmark').click();
        await fixture.whenStable();
        button(root, 'Lever contact · 200 ms').click();
        await fixture.whenStable();
        label.value = 'Lever locked';
        label.dispatchEvent(new Event('input'));
        await fixture.whenStable();
        button(root, 'Rename Bookmark').click();
        await fixture.whenStable();
        expect(timeline.draft()?.clip.markers).toEqual([{id: 'bookmark', at: 200, label: 'Lever locked'}]);
    });

    it('keeps a dirty draft on cancel or failed save and closes only after a successful save', async () => {
        const prototype = HTMLDialogElement.prototype;
        const show = Object.getOwnPropertyDescriptor(prototype, 'showModal');
        const close = Object.getOwnPropertyDescriptor(prototype, 'close');
        Object.defineProperty(prototype, 'showModal', {configurable: true, value(this: HTMLDialogElement) {
            this.setAttribute('open', '');
        }});
        Object.defineProperty(prototype, 'close', {configurable: true, value(this: HTMLDialogElement) {
            this.removeAttribute('open');
            this.dispatchEvent(new Event('close'));
        }});
        try {
            const {fixture, timeline, saveDraft, closeDraft, reviewNavigation} = setup();
            await fixture.whenStable();
            const root = fixture.nativeElement as HTMLElement;
            const trigger = button(root, 'Close Timeline');
            const events: string[] = [];
            fixture.componentInstance.saved.subscribe(() => events.push('saved'));
            fixture.componentInstance.closed.subscribe(() => events.push('closed'));
            await timeline.edit({type: 'edit', edit: {kind: 'remove_key', key_id: 'start'}});
            await fixture.whenStable();

            trigger.focus();
            const kept = reviewNavigation();
            await fixture.whenStable();
            const dialog = root.querySelector<HTMLDialogElement>('dialog[open]')!;
            button(dialog, 'Keep Editing').focus();
            button(dialog, 'Keep Editing').click();
            await fixture.whenStable();
            expect(await kept).toBe(false);
            expect(document.activeElement).toBe(trigger);
            expect(timeline.draft()?.dirty).toBe(true);
            expect(closeDraft).not.toHaveBeenCalled();

            const navigation = reviewNavigation();
            await fixture.whenStable();
            saveDraft.mockRejectedValueOnce(new Error('Cannot save the clip.'));
            button(dialog, 'Save and Close Timeline').click();
            await fixture.whenStable();
            expect(dialog.open).toBe(true);
            expect(dialog.querySelector('[role="alert"]')?.textContent).toContain('Cannot save the clip.');
            expect(closeDraft).not.toHaveBeenCalled();

            button(dialog, 'Save and Close Timeline').click();
            await fixture.whenStable();
            expect(await navigation).toBe(true);
            expect(saveDraft).toHaveBeenLastCalledWith('draft-clip', 1, 1, 2);
            expect(closeDraft).toHaveBeenCalledExactlyOnceWith('draft-clip', false);
            expect(events).toEqual(['saved', 'closed']);
            expect(dialog.open).toBe(false);
        } finally {
            if (show) Object.defineProperty(prototype, 'showModal', show);
            else Reflect.deleteProperty(prototype, 'showModal');
            if (close) Object.defineProperty(prototype, 'close', close);
            else Reflect.deleteProperty(prototype, 'close');
        }
    });

    it('jumps between exact key and bookmark times while leaving the clip untouched', async () => {
        const {fixture, timeline, edit, sample} = setup(timedDraft);
        await fixture.whenStable();
        const root = fixture.nativeElement as HTMLElement;
        const lane = root.querySelector('.scroll-lane')!;
        timeline.snap.set(true);
        button(root, 'Next Key').click();
        await fixture.whenStable();
        expect(timeline.playhead()).toBe(333);
        expect(button(root, '333 ms').getAttribute('aria-pressed')).toBe('true');
        expect(sample).toHaveBeenLastCalledWith(expect.any(String), 333, null, 1);

        const key = new KeyboardEvent('keydown', {key: 'PageDown', shiftKey: true, bubbles: true, cancelable: true});
        lane.dispatchEvent(key);
        await fixture.whenStable();
        expect(key.defaultPrevented).toBe(true);
        expect(timeline.playhead()).toBe(876);
        expect(button(root, 'Locked · 876 ms').getAttribute('aria-pressed')).toBe('true');
        expect(button(root, 'Next Bookmark').disabled).toBe(true);

        button(root, 'Previous Bookmark').click();
        await fixture.whenStable();
        expect(timeline.playhead()).toBe(79);
        expect(button(root, 'Previous Bookmark').disabled).toBe(true);
        button(root, 'Previous Key').click();
        await fixture.whenStable();
        expect(timeline.playhead()).toBe(0);
        expect(button(root, 'Previous Key').disabled).toBe(true);
        expect(edit).not.toHaveBeenCalled();
    });

    it('places ruler ticks, keys, bookmarks and the playhead in the same time scale at every zoom', async () => {
        const {fixture, timeline, edit} = setup(timedDraft);
        await fixture.whenStable();
        const root = fixture.nativeElement as HTMLElement;
        const ruler = root.querySelector<HTMLElement>('.timing-ruler')!;
        const positions = (selector: string) => [...ruler.querySelectorAll<HTMLElement>(selector)]
            .map(point => Number.parseFloat(point.style.left));
        expect(ruler.textContent).toContain('500 ms');
        expect(positions('.ruler-key')).toEqual([0, expect.closeTo(33.3), 100]);
        expect(positions('.ruler-bookmark')).toEqual([expect.closeTo(7.9), expect.closeTo(87.6)]);
        timeline.zoom.set(4);
        button(root, '333 ms').click();
        await fixture.whenStable();
        expect(root.querySelector<HTMLElement>('.key-lane')?.style.width).toBe('400%');
        expect(positions('.ruler-playhead')).toEqual([expect.closeTo(33.3)]);
        expect(positions('.ruler-key.current')).toEqual([expect.closeTo(33.3)]);
        expect(edit).not.toHaveBeenCalled();
    });

    it('reveals an off-screen jump or selection inside the zoomed lane while keeping focus and outer scroll', async () => {
        const {fixture, timeline, edit} = setup(timedDraft);
        await fixture.whenStable();
        const root = fixture.nativeElement as HTMLElement;
        const lane = root.querySelector<HTMLElement>('.scroll-lane')!;
        const cursor = lane.querySelector<HTMLElement>('.ruler-playhead')!;
        const panel = root.querySelector<HTMLElement>('.timeline')!;
        panel.scrollLeft = 30;
        timeline.zoom.set(8);
        Object.defineProperty(lane, 'clientWidth', {value: 160});
        vi.spyOn(lane, 'getBoundingClientRect').mockReturnValue(new DOMRect(100, 0, 160, 48));
        const marker = vi.spyOn(cursor, 'getBoundingClientRect').mockReturnValue(new DOMRect(510, 0, 2, 16));
        lane.focus();
        lane.dispatchEvent(new KeyboardEvent('keydown', {key: 'PageDown', bubbles: true, cancelable: true}));
        await fixture.whenStable();
        expect(timeline.playhead()).toBe(333);
        expect(lane.scrollLeft).toBe(252);
        expect(document.activeElement).toBe(lane);

        marker.mockReturnValue(new DOMRect(92, 0, 2, 16));
        const bookmark = button(root, 'Half-open · 79 ms');
        bookmark.focus();
        bookmark.click();
        await fixture.whenStable();
        expect(timeline.playhead()).toBe(79);
        expect(lane.scrollLeft).toBe(244);
        expect(document.activeElement).toBe(bookmark);
        expect(panel.scrollLeft).toBe(30);

        marker.mockClear();
        timeline.seek(876);
        await fixture.whenStable();
        expect(marker).not.toHaveBeenCalled();
        expect(lane.scrollLeft).toBe(244);
        expect(edit).not.toHaveBeenCalled();
    });

    it('plays at the chosen editor speed without writing to the clip or its undo history', async () => {
        const {fixture, timeline, edit, saveDraft} = setup();
        await fixture.whenStable();
        const root = fixture.nativeElement as HTMLElement;
        const speed = [...root.querySelectorAll('label')]
            .find(label => label.firstChild?.textContent?.trim() === 'Playback speed')!.querySelector('select')!;
        expect(speed.value).toBe('1');
        speed.value = '0.5';
        speed.dispatchEvent(new Event('change'));
        await fixture.whenStable();
        let tick: FrameRequestCallback | undefined;
        const request = vi.spyOn(globalThis, 'requestAnimationFrame').mockImplementation(callback => {
            tick = callback;
            return 1;
        });
        try {
            button(root, 'Play Clip').click();
            await fixture.whenStable();
            expect(speed.disabled).toBe(true);
            tick?.(100);
            tick?.(500);
            expect(timeline.playhead()).toBe(200);
            timeline.setPlaybackRate(2);
            tick?.(2100);
            expect(timeline.playhead()).toBe(1000);
            expect(timeline.playing()).toBe(false);
            expect(timeline.playbackRate()).toBe(0.5);
            expect(edit).not.toHaveBeenCalled();
            expect(saveDraft).not.toHaveBeenCalled();
        } finally {
            request.mockRestore();
        }
    });

    it('loops the editor transport inside the chosen range and releases it on destruction', async () => {
        const {fixture, timeline, close} = setup();
        await fixture.whenStable();
        let queued: FrameRequestCallback | undefined;
        const request = vi.spyOn(globalThis, 'requestAnimationFrame').mockImplementation(callback => {
            queued = callback;
            return 1;
        });
        try {
            timeline.looping.set(true);
            timeline.setLoopStart(200);
            timeline.setLoopEnd(500);
            timeline.seek(200);
            timeline.play();
            queued?.(0);
            queued?.(420);
            expect(timeline.playhead()).toBe(320);
            expect(timeline.playing()).toBe(true);
            fixture.destroy();
            expect(timeline.playing()).toBe(false);
            expect(close).toHaveBeenCalled();
        } finally {
            request.mockRestore();
        }
    });
});
