import {TestBed} from '@angular/core/testing';
import {invoke, isTauri} from '@tauri-apps/api/core';
import {listen} from '@tauri-apps/api/event';
import {vi} from 'vitest';
import {EditorService} from './editor.service';
import type {EditorCapabilities, EditorSnapshot, ViewportBounds, ViewportStatus,} from './editor.models';

vi.mock('@tauri-apps/api/core', () => ({invoke: vi.fn(), isTauri: vi.fn(() => true)}));
vi.mock('@tauri-apps/api/event', () => ({listen: vi.fn()}));

function snapshot(revision: number): EditorSnapshot {
    return {
        revision,
        scene_revision: revision,
        selection: [],
        mode: 'edit',
        paused: false,
        dirty: false,
        can_undo: false,
        can_redo: false,
        transaction: null,
        snap: {enabled: false, translation: 0.25, rotation_radians: Math.PI / 12, scale: 0.1},
        document: {
            schema_version: 3,
            entities: [],
            bindings: {},
            lights: [],
            camera: {
                position: [0, 0, 3],
                yaw: 0,
                pitch: 0,
                vertical_fov: 1,
                near: 0.1,
                far: 100,
            },
        },
    };
}

const capabilities: EditorCapabilities = {
    protocol_version: 1,
    scene_schema_version: 3,
    commands: [],
    max_selection: 4096,
    max_history_entries: 128,
    history_budget_bytes: 16 * 1024 * 1024,
    embedded_viewport_supported: true,
};
const status: ViewportStatus = {
    attached: true,
    visible: true,
    width: 800,
    height: 600,
    draw_calls: 0,
    instances: 0,
    triangles: 0,
    gpu_time_ms: null,
    gpu_timestamps_supported: false,
    error: null,
};
const bounds: ViewportBounds = {x: 0, y: 0, width: 800, height: 600, visible: true};

function deferred<T>() {
    let resolve!: (value: T) => void;
    const promise = new Promise<T>((done) => {
        resolve = done;
    });
    return {promise, resolve};
}

describe('EditorService native command and viewport queues', () => {
    beforeEach(() => {
        vi.mocked(invoke).mockReset();
        vi.mocked(isTauri).mockReturnValue(true);
        vi.mocked(listen).mockReset().mockResolvedValue(vi.fn());
        TestBed.configureTestingModule({providers: [EditorService]});
    });

    it('serializes document commands using the revision returned by the preceding command', async () => {
        vi.mocked(invoke)
            .mockResolvedValueOnce(snapshot(4))
            .mockResolvedValueOnce(snapshot(5))
            .mockResolvedValueOnce(snapshot(6));
        const service = TestBed.inject(EditorService);
        await service.open('assets', 'scene.json');
        const first = service.execute({type: 'select', keys: ['a']});
        const second = service.execute({type: 'rename', key: 'a', name: 'Changed'});
        expect(service.pending()).toBe(2);
        expect(service.busy()).toBe(true);
        await Promise.all([first, second]);
        expect(invoke).toHaveBeenNthCalledWith(2, 'editor_execute', {
            request: {
                expected_revision: 4,
                command: {type: 'select', keys: ['a']},
            },
        });
        expect(invoke).toHaveBeenNthCalledWith(3, 'editor_execute', {
            request: {
                expected_revision: 5,
                command: {type: 'rename', key: 'a', name: 'Changed'},
            },
        });
        expect(service.snapshot()?.revision).toBe(6);
        expect(service.pending()).toBe(0);
        expect(service.busy()).toBe(false);
    });

    it('refreshes authoritative memory after a conflict without retrying the rejected edit', async () => {
        vi.mocked(invoke)
            .mockResolvedValueOnce(snapshot(1))
            .mockRejectedValueOnce('Revision conflict: current 9')
            .mockResolvedValueOnce(snapshot(9))
            .mockResolvedValueOnce(snapshot(10));
        const service = TestBed.inject(EditorService);
        await service.open('assets', 'scene.json');
        await expect(service.execute({type: 'delete_selection'})).rejects.toMatchObject({
            kind: 'conflict',
        });
        expect(service.snapshot()?.revision).toBe(9);
        expect(invoke).toHaveBeenNthCalledWith(3, 'editor_snapshot', undefined);
        expect(
            vi.mocked(invoke).mock.calls.filter(([operation]) => operation === 'editor_execute'),
        ).toHaveLength(1);
        expect(vi.mocked(invoke).mock.calls.some(([operation]) => operation === 'editor_reload')).toBe(
            false,
        );
        await service.execute({type: 'undo'});
        expect(invoke).toHaveBeenLastCalledWith('editor_execute', {
            request: {expected_revision: 9, command: {type: 'undo'}},
        });
        expect(service.error()).toBeNull();
    });

    it('rejects commands before opening a document and browser-only use before invoking native IPC', async () => {
        const service = TestBed.inject(EditorService);
        await expect(service.execute({type: 'undo'})).rejects.toMatchObject({kind: 'not_open'});
        expect(invoke).not.toHaveBeenCalled();
        TestBed.resetTestingModule();
        vi.mocked(isTauri).mockReturnValue(false);
        const browser = TestBed.inject(EditorService);
        await expect(browser.open('assets', 'scene.json')).rejects.toMatchObject({
            kind: 'unavailable',
        });
        expect(invoke).not.toHaveBeenCalled();
    });

    it('deduplicates capabilities and rejects incompatible protocol/schema before caching', async () => {
        const result = deferred<EditorCapabilities>();
        vi.mocked(invoke).mockReturnValueOnce(result.promise);
        const service = TestBed.inject(EditorService);
        const first = service.loadCapabilities();
        const second = service.loadCapabilities();
        expect(second).toBe(first);
        result.resolve({...capabilities, scene_schema_version: 2});
        await expect(first).rejects.toMatchObject({kind: 'backend'});
        expect(service.capabilities()).toBeNull();
        vi.mocked(invoke).mockResolvedValueOnce(capabilities);
        expect(await service.loadCapabilities()).toEqual(capabilities);
        await service.loadCapabilities();
        expect(invoke).toHaveBeenCalledTimes(2);
    });

    it('coalesces queued viewport bounds to the latest geometry', async () => {
        vi.mocked(invoke).mockResolvedValue(status);
        const service = TestBed.inject(EditorService);
        const first = service.updateViewportBounds(bounds);
        const second = service.updateViewportBounds({...bounds, x: 10});
        const third = service.updateViewportBounds({...bounds, x: 20, visible: false});
        expect(second).toBe(first);
        expect(third).toBe(first);
        await third;
        expect(invoke).toHaveBeenCalledExactlyOnceWith('editor_viewport_bounds', {
            bounds: {...bounds, x: 20, visible: false},
        });
    });

    it('orders detach after an in-flight attach and coalesces duplicate detach requests', async () => {
        const attachment = deferred<ViewportStatus>();
        const unlisten = vi.fn();
        vi.mocked(listen).mockResolvedValue(unlisten);
        vi.mocked(invoke)
            .mockResolvedValueOnce(capabilities)
            .mockReturnValueOnce(attachment.promise)
            .mockResolvedValueOnce({...status, attached: false, visible: false});
        const service = TestBed.inject(EditorService);
        const attached = service.attachViewport(bounds);
        await vi.waitFor(() =>
            expect(invoke).toHaveBeenCalledWith('editor_viewport_attach', {bounds}),
        );
        const first = service.detachViewport();
        expect(service.detachViewport()).toBe(first);
        expect(
            vi.mocked(invoke).mock.calls.some(([operation]) => operation === 'editor_viewport_detach'),
        ).toBe(false);
        attachment.resolve(status);
        await Promise.all([attached, first]);
        expect(invoke).toHaveBeenLastCalledWith('editor_viewport_detach', undefined);
        expect(unlisten).toHaveBeenCalledTimes(1);
        expect(service.viewport()?.attached).toBe(false);
    });

    it('keeps old and new owner bounds on their respective sides of lifecycle boundaries', async () => {
        vi.mocked(invoke)
            .mockResolvedValueOnce(status)
            .mockResolvedValueOnce(capabilities)
            .mockResolvedValueOnce(status)
            .mockResolvedValueOnce(status);
        const service = TestBed.inject(EditorService);
        const oldBounds = service.updateViewportBounds({...bounds, x: 1});
        const attached = service.attachViewport(bounds);
        const newBounds = service.updateViewportBounds({...bounds, x: 2});
        expect(oldBounds).not.toBe(newBounds);
        await Promise.all([oldBounds, attached, newBounds]);
        expect(vi.mocked(invoke).mock.calls.map(([operation]) => operation)).toEqual([
            'editor_viewport_bounds',
            'editor_capabilities',
            'editor_viewport_attach',
            'editor_viewport_bounds',
        ]);
        expect(invoke).toHaveBeenLastCalledWith('editor_viewport_bounds', {
            bounds: {...bounds, x: 2},
        });
    });

    it('releases viewport ownership idempotently and allows the next owner to reserve it', () => {
        const service = TestBed.inject(EditorService);
        const release = service.reserveViewport();
        expect(() => service.reserveViewport()).toThrow('already owns');
        release();
        release();
        const next = service.reserveViewport();
        release(); // The first owner's stale release cannot release the new owner.
        expect(() => service.reserveViewport()).toThrow('already owns');
        next();
        expect(() => service.reserveViewport()).not.toThrow();
    });

    it('bounds document work at 256 pending calls without dropping queued requests', async () => {
        vi.mocked(invoke).mockResolvedValue(snapshot(0));
        const service = TestBed.inject(EditorService);
        const queued = Array.from({length: 256}, () => service.refresh());
        expect(service.pending()).toBe(256);
        await expect(service.refresh()).rejects.toMatchObject({kind: 'busy'});
        await Promise.all(queued);
        expect(invoke).toHaveBeenCalledTimes(256);
        expect(service.pending()).toBe(0);
    });

    it('cleans up a native listener that resolves after its Angular owner was destroyed', async () => {
        const subscription = deferred<() => void>();
        vi.mocked(listen).mockReturnValueOnce(subscription.promise);
        const service = TestBed.inject(EditorService);
        const pending = service.listenViewportInput(vi.fn());
        TestBed.resetTestingModule();
        const unlisten = vi.fn();
        subscription.resolve(unlisten);
        const stop = await pending;
        expect(unlisten).toHaveBeenCalledTimes(1);
        stop();
        expect(unlisten).toHaveBeenCalledTimes(1);
    });
});
