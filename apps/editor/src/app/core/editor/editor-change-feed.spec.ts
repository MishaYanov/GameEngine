import nativeChangePage from './fixtures/editor-change-page.json';
import {TestBed} from '@angular/core/testing';
import {EDITOR_TRANSPORT, type EditorTransport} from './editor-transport';
import {EditorService} from './editor.service';
import {readEditorChangePage} from './editor-change-feed';
import type {EditorSnapshot} from './editor.models';

const snapshot = (revision: number) => ({revision, scene_revision: revision} as EditorSnapshot);

describe('Editor transport and change feed', () => {
    function setup(pages: readonly unknown[], onChangeReady?: EditorTransport['onChangeReady']) {
        const calls: Array<{operation: string; arguments_?: Record<string, unknown>}> = [];
        let nextPage = 0;
        let nextSnapshot = 0;
        const transport: EditorTransport = {
            available: true,
            onChangeReady,
            async invoke<T>(operation: string, arguments_?: Record<string, unknown>): Promise<T> {
                calls.push({operation, arguments_});
                if (operation === 'editor_snapshot' || operation === 'editor_open') return snapshot(++nextSnapshot) as T;
                if (operation === 'editor_changes') return pages[nextPage++] as T;
                if (operation === 'editor_capabilities') return {protocol_version: 2, scene_schema_version: 8, resumable_changes: !!onChangeReady} as T;
                if (operation === 'editor_execute') throw {code: 'conflict', message: 'Stale revision'};
                throw new Error('Unexpected operation ' + operation);
            },
        };
        TestBed.configureTestingModule({providers: [{provide: EDITOR_TRANSPORT, useValue: transport}]});
        return {service: TestBed.inject(EditorService), calls};
    }

    it('correlates a rejected command with its requested revision without retrying the edit', async () => {
        const {service, calls} = setup([]);
        await service.refresh();
        await expect(service.execute({type: 'stop_play'}, undefined, 1)).rejects.toThrow('Stale revision');
        expect(service.commandOutcomes()).toEqual([{
            id: 1, command: 'stop_play', workspaceGeneration: 0,
            expectedRevision: 1, acceptedRevision: null, outcome: 'rejected',
        }]);
        expect(calls.filter(call => call.operation === 'editor_execute')).toHaveLength(1);
    });

    it('does not send an old workspace command after a queued Open succeeds', async () => {
        const {service, calls} = setup([]);
        await service.refresh();
        const opening = service.open('assets', 'scenes/main.json');
        const edit = service.execute({type: 'undo'});
        await opening;
        await expect(edit).rejects.toThrow(/workspace changed/i);
        expect(calls.some(call => call.operation === 'editor_execute')).toBe(false);
        expect(service.commandOutcomes().at(-1)?.outcome).toBe('rejected');
    });

    it('checks the journal once after the listener registers', async () => {
        let registered: (() => void) | undefined;
        const {service, calls} = setup([nativeChangePage], () => new Promise(resolve => {
            registered = () => resolve(() => undefined);
        }));
        await service.refresh();
        await service.loadCapabilities();
        expect(calls.some(call => call.operation === 'editor_changes')).toBe(false);
        registered?.();
        await vi.waitFor(() => expect(service.snapshot()?.revision).toBe(2));
        expect(calls.filter(call => call.operation === 'editor_changes')).toHaveLength(1);
    });

    it('shares one catch-up query and resets from an expired journal cursor', async () => {
        const {service, calls} = setup([
            {cursor: 1, resync_required: false, events: [{sequence: 1, revision: 1, event: {kind: 'simulation_advanced'}}]},
            {cursor: 26, resync_required: true, events: []},
        ]);
        await service.refresh();
        const resume = service.resumeChanges();
        expect(service.resumeChanges()).toBe(resume);
        expect((await resume).revision).toBe(1);
        expect((await service.resumeChanges()).revision).toBe(2);
        expect(calls.filter(call => call.operation === 'editor_changes').map(call => call.arguments_)).toEqual([{cursor: 0}, {cursor: 1}]);
    });

    it('reads the Rust change-journal fixture with its native field names', () => {
        expect(readEditorChangePage(nativeChangePage, 0)).toEqual({
            cursor: 2, resyncRequired: false, revisions: [10, 11],
        });
    });

    it('rejects unsafe or noncontiguous cursor stamps before using them', () => {
        expect(() => readEditorChangePage({cursor: Number.MAX_SAFE_INTEGER + 1, resync_required: false, events: []}, 0)).toThrow();
        expect(() => readEditorChangePage({cursor: 2, resync_required: false,
            events: [{sequence: 2, revision: 3, event: {kind: 'state_changed'}}]}, 0)).toThrow();
    });
});
