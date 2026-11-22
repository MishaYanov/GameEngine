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

function setup() {
    let revision = 0, cursor = 0;
    let history = [initial.clip];
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
    TestBed.configureTestingModule({providers: [
        {provide: AnimationWorkspaceService, useValue: {authoredReady: showPreview}},
        {provide: EditorService, useValue: {
            nativeAvailable: true, snapshot: () => ({mode: 'edit', revision: 2, transaction: null}),
            workspaceGeneration: () => 1,
            openClipTimelineDraft: async () => structuredClone(initial),
            editClipTimelineDraft: edit,
            openClipTimelinePreview: open,
            sampleClipTimelinePreview: sample,
            closeAuthoredClipPreview: close,
            closeClipTimelineDraft: async () => undefined,
        }},
    ]});
    const fixture = TestBed.createComponent(ClipTimeline);
    fixture.componentRef.setInput('asset', initial);
    return {fixture, edit, open, sample, close, showPreview, timeline: fixture.debugElement.injector.get(TimelineEditService)};
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
