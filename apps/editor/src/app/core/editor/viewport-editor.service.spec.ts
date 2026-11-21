import {signal} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import {invoke} from '@tauri-apps/api/core';
import {type Event, listen} from '@tauri-apps/api/event';
import {vi} from 'vitest';
import {EditorService} from './editor.service';
import {ViewportEditorService} from './viewport-editor.service';
import {
    DEFAULT_VIEWPORT_SETTINGS,
    type PlayStatus,
    type ViewportAction,
    type ViewportEditorState
} from './viewport-editor.models';

vi.mock('@tauri-apps/api/core', () => ({invoke: vi.fn(), isTauri: vi.fn(() => true)}));
vi.mock('@tauri-apps/api/event', () => ({listen: vi.fn()}));

function state(revision = 1, generation = 1): ViewportEditorState {
    return {settings: {...DEFAULT_VIEWPORT_SETTINGS}, camera_position: [0, 5, 10], camera_target: [0, 0, 0],
        camera_distance: 10, revision, workspace_generation: generation, gesture: null, placement: null, focused: true, error: null};
}

function gameStatus(playSessionId: string, runId: string, completed = false): PlayStatus {
    return {
        play_session_id: playSessionId, run_id: runId, scene_id: 'courtyard', scene_generation: 0,
        paused: false, pending_scene_id: null,
        gameplay: {objective: completed ? 'Run complete' : 'Find the key', detail: null, score: 0, completed},
    };
}

function deferred<T>() {
    let resolve!: (value: T) => void;
    const promise = new Promise<T>(done => resolve = done);
    return {promise, resolve};
}

describe('ViewportEditorService', () => {
    let service: ViewportEditorService;
    let handlers: Map<string, (event: Event<unknown>) => void>;
    let stops: ReturnType<typeof vi.fn>[];
    let current: ViewportEditorState;
    let snapshot: ReturnType<typeof signal<{ revision: number; mode?: 'edit' | 'play'; play_session_id?: string }>>;
    let workspaceGeneration: ReturnType<typeof signal<number>>;
    let refresh: ReturnType<typeof vi.fn>;

    beforeEach(() => {
        handlers = new Map(); stops = []; current = state(); snapshot = signal({revision: 1}); workspaceGeneration = signal(1);
        refresh = vi.fn(async () => {snapshot.set({revision: 10}); return snapshot();});
        vi.mocked(listen).mockReset().mockImplementation(async (name, handler) => {
            handlers.set(name, handler as (event: Event<unknown>) => void);
            const stop = vi.fn(); stops.push(stop); return stop;
        });
        vi.mocked(invoke).mockReset().mockImplementation(async (operation, args) => {
            if (operation === 'editor_viewport_editor_state') return current;
            if (operation === 'editor_viewport_action') {
                const action = (args as {action: ViewportAction}).action;
                current = {...current, revision: current.revision + 1};
                if (action.type === 'set_tool') current = {...current, settings: {...current.settings, tool: action.tool}};
                return current;
            }
            throw new Error(`Unexpected operation: ${operation}`);
        });
        TestBed.configureTestingModule({providers: [ViewportEditorService,
            {provide: EditorService, useValue: {nativeAvailable: true, snapshot, refresh, workspaceGeneration}}]});
        service = TestBed.inject(ViewportEditorService);
    });

    function emit(name: string, payload: unknown): void {handlers.get(name)!({event: name, id: 1, payload});}
    function actions(): ViewportAction[] {return vi.mocked(invoke).mock.calls.filter(call => call[0] === 'editor_viewport_action').map(call => (call[1] as {action: ViewportAction}).action);}

    it('subscribes once and camera/tool notifications never refresh authored documents', async () => {
        await Promise.all([service.start(), service.start()]);
        expect(listen).toHaveBeenCalledTimes(2);
        for (let revision = 2; revision < 40; revision++) emit('editor://viewport-editor-state', {...state(revision), camera_distance: revision});
        expect(service.state()?.camera_distance).toBe(39);
        expect(refresh).not.toHaveBeenCalled();
        expect(invoke).toHaveBeenCalledExactlyOnceWith('editor_viewport_editor_state');
    });

    it('accepts progress-only game status and hides status from another Play session', async () => {
        await service.start();
        snapshot.set({revision: 2, mode: 'play', play_session_id: 'play-1'});
        const first = gameStatus('play-1', 'run-1');
        emit('editor://viewport-editor-state', {...state(2), game_status: first});
        expect(service.gameStatus()).toEqual(first);
        const completed = gameStatus('play-1', 'run-1', true);
        emit('editor://viewport-editor-state', {...state(3), game_status: completed});
        expect(service.gameStatus()).toEqual(completed);

        snapshot.set({revision: 3, mode: 'edit'});
        expect(service.gameStatus()).toBeNull();
        snapshot.set({revision: 4, mode: 'play', play_session_id: 'play-2'});
        emit('editor://viewport-editor-state', {...state(4), game_status: completed});
        expect(service.gameStatus()).toBeNull();
        const second = gameStatus('play-2', 'run-2');
        emit('editor://viewport-editor-state', {...state(5), game_status: second});
        emit('editor://viewport-editor-state', {...state(4), game_status: completed});
        expect(service.gameStatus()).toEqual(second);
        expect(refresh).not.toHaveBeenCalled();
        expect(invoke).toHaveBeenCalledExactlyOnceWith('editor_viewport_editor_state');
    });

    it('ignores stale restart reply errors after newer state has arrived', async () => {
        await service.start();
        snapshot.set({revision: 2, mode: 'play', play_session_id: 'play-1'});
        const first = gameStatus('play-1', 'run-1', true);
        emit('editor://viewport-editor-state', {...state(2), game_status: first});
        const held = deferred<ViewportEditorState>();
        vi.mocked(invoke).mockImplementationOnce(() => held.promise);
        const restart = service.action({type: 'restart_run', run_id: first.run_id, expected_revision: 2});
        await vi.waitFor(() => expect(actions()).toHaveLength(1));
        const restarted = gameStatus('play-1', 'run-2');
        emit('editor://viewport-editor-state', {...state(4), game_status: restarted});
        held.resolve({...state(3), game_status: first, error: 'Old restart failure'});
        await restart;

        expect(service.gameStatus()).toEqual(restarted);
        expect(service.error()).toBeNull();
        await expect(service.action({type: 'restart_run', run_id: first.run_id, expected_revision: 2}))
            .rejects.toThrow('game run changed');
        expect(actions()).toHaveLength(1);
        expect(service.error()).toBeNull();
    });

    it('coalesces a document-event burst into one authoritative refresh', async () => {
        await service.start();
        for (let revision = 2; revision <= 10; revision++) emit('editor://viewport-document-changed', {workspace_generation: 1, revision});
        await vi.waitFor(() => expect(refresh).toHaveBeenCalledOnce());
        expect(snapshot().revision).toBe(10);
    });

    it('does not repeat a delayed refresh that already includes the latest notification', async () => {
        await service.start();
        const result = deferred<{revision: number}>();
        refresh.mockReturnValueOnce(result.promise);
        emit('editor://viewport-document-changed', {workspace_generation: 1, revision: 2});
        await vi.waitFor(() => expect(refresh).toHaveBeenCalledOnce());
        emit('editor://viewport-document-changed', {workspace_generation: 1, revision: 3});
        emit('editor://viewport-document-changed', {workspace_generation: 1, revision: 4});
        result.resolve({revision: 4});
        await result.promise; await Promise.resolve(); await Promise.resolve();
        expect(refresh).toHaveBeenCalledOnce();
    });

    it('keeps only the newest queued placement point before an ordered commit', async () => {
        await service.start();
        const held = deferred<ViewportEditorState>();
        vi.mocked(invoke).mockImplementationOnce(() => held.promise);
        const beginning = service.action({type: 'begin_placement', path: 'props/robot.glb'});
        await vi.waitFor(() => expect(actions()).toHaveLength(1));
        const first = service.updatePlacement([0.1, 0.2]);
        const latest = service.updatePlacement([0.9, 0.8]);
        expect(first).toBe(latest);
        const commit = service.action({type: 'commit_placement'});
        held.resolve(state(2));
        await Promise.all([beginning, first, commit]);
        expect(actions()).toEqual([
            {type: 'begin_placement', path: 'props/robot.glb'},
            {type: 'update_placement', normalized: [0.9, 0.8]},
            {type: 'commit_placement'},
        ]);
        expect(service.pending()).toBe(0);
    });

    it('caps action backlog and accepts later commands after it drains', async () => {
        const commands = Array.from({length: 33}, () => service.action({type: 'zoom', factor: 0.8}));
        const results = await Promise.allSettled(commands);
        expect(results.filter(result => result.status === 'rejected')).toHaveLength(1);
        expect(actions()).toHaveLength(32);
        await service.action({type: 'set_tool', tool: 'rotate'});
        expect(service.settings().tool).toBe('rotate');
    });

    it('rejects queued actions and late replies after project replacement', async () => {
        await service.start();
        const held = deferred<ViewportEditorState>();
        vi.mocked(invoke).mockImplementationOnce(() => held.promise);
        const first = service.action({type: 'begin_placement', path: 'props/robot.glb'});
        await vi.waitFor(() => expect(actions()).toHaveLength(1));
        const queued = service.action({type: 'commit_placement'});
        const settled = Promise.allSettled([first, queued]);
        workspaceGeneration.set(2);
        held.resolve({...state(5, 1), gesture: 'placement'});
        const results = await settled;
        expect(results.every(result => result.status === 'rejected')).toBe(true);
        expect(actions()).toHaveLength(1);
        expect(service.state()).toBeNull();
        expect(service.pending()).toBe(0);
        current = state(1, 2);
        await service.action({type: 'set_tool', tool: 'rotate'});
        expect(service.state()?.workspace_generation).toBe(2);
        expect(service.settings().tool).toBe('rotate');
    });

    it('rejects stale native view states across revision and workspace boundaries', async () => {
        await service.start();
        emit('editor://viewport-editor-state', state(20));
        emit('editor://viewport-editor-state', state(19));
        expect(service.state()?.revision).toBe(20);
        workspaceGeneration.set(2);
        emit('editor://viewport-editor-state', state(1, 2));
        emit('editor://viewport-editor-state', state(50, 1));
        expect(service.state()?.workspace_generation).toBe(2);
        expect(service.state()?.revision).toBe(1);
    });

    it('clears old previews and keeps native generations separate from local context counters', async () => {
        current = {...state(7, 41), gesture: 'transform', transform_preview: {
            key: 'robot', transform: {position: [1, 2, 3], rotation: [0, 0, 0], scale: [1, 1, 1]},
        }};
        await service.start();
        expect(service.authoringGesture()).toBe(true);
        workspaceGeneration.set(2);
        expect(service.state()).toBeNull();
        expect(service.authoringGesture()).toBe(false);
        emit('editor://viewport-editor-state', {...state(8, 41), gesture: 'transform'});
        expect(service.state()).toBeNull();
        emit('editor://viewport-editor-state', state(1, 99));
        expect(service.state()?.workspace_generation).toBe(99);
        expect(service.state()?.transform_preview).toBeUndefined();
    });

    it('retains a future native state arriving before the frontend open acknowledgement', async () => {
        current = state(7, 41);
        await service.start();
        emit('editor://viewport-editor-state', {...state(1, 99), gesture: 'placement'});
        expect(service.state()?.workspace_generation).toBe(41);
        workspaceGeneration.set(2);
        TestBed.tick();
        expect(service.state()?.workspace_generation).toBe(99);
        expect(service.authoringGesture()).toBe(true);
        emit('editor://viewport-editor-state', {...state(50, 41), gesture: 'transform'});
        expect(service.state()?.workspace_generation).toBe(99);
    });

    it('keeps camera navigation separate from authoring gesture locks', async () => {
        await service.start();
        emit('editor://viewport-editor-state', {...state(2), gesture: 'navigation'});
        expect(service.authoringGesture()).toBe(false);
        for (const [index, gesture] of ['transform', 'placement', 'asset_loading'].entries()) {
            emit('editor://viewport-editor-state', {...state(index + 3), gesture});
            expect(service.authoringGesture()).toBe(true);
        }
        emit('editor://viewport-editor-state', {...state(6), gesture: null});
        expect(service.authoringGesture()).toBe(false);
        expect(refresh).not.toHaveBeenCalled();
    });

    it('uses normalized CSS aperture coordinates without applying DPI twice', () => {
        const element = document.createElement('div');
        vi.spyOn(element, 'getBoundingClientRect').mockReturnValue({left: 100, top: 50, width: 400, height: 200} as DOMRect);
        const release = service.registerSurface(element);
        expect(service.normalizedPoint(300, 150)).toEqual([0.5, 0.5]);
        expect(service.normalizedPoint(99, 150)).toBeNull();
        release();
        expect(service.normalizedPoint(300, 150)).toBeNull();
    });

    it('cleans subscriptions that resolve after workspace destruction', async () => {
        const subscription = deferred<() => void>();
        vi.mocked(listen).mockReturnValueOnce(subscription.promise);
        const pending = service.start();
        TestBed.resetTestingModule();
        const stop = vi.fn(); subscription.resolve(stop);
        await pending;
        expect(stop).toHaveBeenCalledOnce();
        expect(invoke).not.toHaveBeenCalled();
    });

    it('surfaces command errors and keeps its queue usable', async () => {
        await service.start();
        vi.mocked(invoke).mockRejectedValueOnce(new Error('Native view rejected the setting.'));
        await expect(service.action({type: 'set_tool', tool: 'rotate'})).rejects.toThrow('rejected');
        expect(service.error()).toContain('rejected');
        await service.action({type: 'set_tool', tool: 'scale'});
        expect(service.error()).toBeNull();
        expect(service.settings().tool).toBe('scale');
    });
});
