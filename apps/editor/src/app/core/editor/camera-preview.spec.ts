import {vi} from 'vitest';
import {
    CameraPreviewController,
    type CameraPreviewReply,
    type PreviewDefinition,
    type PreviewIntent,
    readCameraPreview
} from './camera-preview';

const definition: PreviewDefinition = {
    camera: {
        position: [0, 0, 0], yaw: 0, pitch: 0, vertical_fov: 1,
        near: 0.1, far: 100
    }, expectedGeneration: 31, expectedRevision: 7, sceneId: 'main'
};
const first: CameraPreviewReply = {
    token: 'first', workspace_generation: 31, revision: 7, sample: 0, scene_id: 'main',
    effective_distance: 3, required_probe_radius: 0.25, target_lost: false, blocker: null
};
const turn = {yaw_delta: Math.PI, pitch_delta: -Math.PI, log_distance_delta: Math.log(4)};

function setup() {
    const api = {
        open: vi.fn(async (_d: PreviewDefinition) => first),
        close: vi.fn(async (_token: string) => undefined),
        sample: vi.fn(async (token: string, sample: number, _intent: PreviewIntent) => ({...first, token, sample})),
    };
    return {api, owner: new CameraPreviewController(api)};
}

describe('native camera preview ownership', () => {
    it('closes an old pending open before starting the latest requested definition', async () => {
        const {api, owner} = setup();
        let finish!: (r: CameraPreviewReply) => void;
        api.open.mockImplementationOnce(() => new Promise(done => {
            finish = done;
        }));
        const replacement = {...definition, expectedRevision: 9};
        api.open.mockResolvedValueOnce({...first, token: 'latest', revision: 9});
        owner.open(definition);
        owner.open({...definition, expectedRevision: 8});
        owner.open(replacement);
        owner.close(definition); // Late destruction of A must not close B.
        expect(api.open).toHaveBeenCalledTimes(1);
        finish(first);
        await vi.waitFor(() => expect(owner.state()?.token).toBe('latest'));
        expect(api.open.mock.calls.map(([d]) => d.expectedRevision)).toEqual([7, 9]);
        expect(api.close.mock.calls).toEqual([['first']]);
        expect(api.close.mock.invocationCallOrder[0]).toBeLessThan(api.open.mock.invocationCallOrder[1]);
        owner.close(replacement);
        await vi.waitFor(() => expect(api.close.mock.calls).toEqual([['first'], ['latest']]));
    });

    it('coalesces bounded input, retries a rejected sample number and serializes Close', async () => {
        const {api, owner} = setup();
        owner.open(definition);
        await vi.waitFor(() => expect(owner.state()?.sample).toBe(0));
        api.sample.mockRejectedValueOnce({kind: 'validation', message: 'Penetrating pivot'});
        owner.sample(turn);
        await vi.waitFor(() => expect(owner.error()).toContain('Penetrating'));
        expect(owner.state()?.sample).toBe(0);
        let finish!: (r: CameraPreviewReply) => void;
        api.sample.mockImplementationOnce(() => new Promise(done => {
            finish = done;
        }));
        owner.sample(turn);
        for (let i = 0; i < 10; i++) owner.sample(turn);
        expect(api.sample.mock.calls.map(([, n]) => n)).toEqual([1, 1]);
        finish({...first, sample: 1});
        await vi.waitFor(() => expect(owner.state()?.sample).toBe(2));
        expect(api.sample.mock.lastCall).toEqual(['first', 2, turn]);
        api.sample.mockImplementationOnce(() => new Promise(done => {
            finish = done;
        }));
        owner.sample(turn);
        owner.close();
        expect(owner.state()).toBeNull();
        expect(api.close).not.toHaveBeenCalled();
        finish({...first, sample: 3});
        await vi.waitFor(() => expect(api.close).toHaveBeenCalledWith('first'));
        expect(owner.state()).toBeNull();
    });

    it('blocks another open in the same context after losing the token, then accepts a fresh revision', async () => {
        const {api, owner} = setup();
        api.open.mockRejectedValueOnce('Worker stopped before reporting its result');
        owner.open(definition);
        await vi.waitFor(() => expect(owner.error()).toContain('could not be confirmed'));
        owner.close();
        owner.open({...definition});
        expect(api.open).toHaveBeenCalledTimes(1);
        const fresh = {...definition, expectedRevision: 8};
        api.open.mockResolvedValueOnce({...first, revision: 8});
        owner.open(fresh);
        await vi.waitFor(() => expect(owner.state()?.revision).toBe(8));
    });

    it('closes the known token after losing a sample acknowledgement', async () => {
        const {api, owner} = setup();
        owner.open(definition);
        await vi.waitFor(() => expect(owner.state()?.sample).toBe(0));
        api.sample.mockRejectedValueOnce(new Error('Transport closed'));
        owner.sample(turn);
        await vi.waitFor(() => expect(api.close).toHaveBeenCalledWith('first'));
        expect(owner.state()).toBeNull();
        expect(owner.error()).toContain('could not be confirmed');
        owner.sample(turn);
        expect(api.sample).toHaveBeenCalledTimes(1);
    });

    it('rejects invalid diagnostics and out-of-range counters from native code', () => {
        expect(readCameraPreview(first)).toEqual(first);
        for (const fields of [{token: ''}, {token: 'a'.repeat(129)}, {revision: -1}, {sample: 2 ** 53},
            {effective_distance: NaN}, {required_probe_radius: Infinity}, {blocker: 7}, {scene_id: ''}]) {
            expect(() => readCameraPreview({...first, ...fields})).toThrow('Invalid');
        }
    });

    it('releases a mismatched selected-camera token and samples only Orbit previews', async () => {
        const {api, owner} = setup();
        const scoped: PreviewDefinition = {cameraKey: 'camera', expectedGeneration: 31, expectedRevision: 7, sceneId: 'main'};
        const still = {...first, mode: 'fixed' as const, camera_key: 'camera', required_probe_radius: 0, effective_distance: 0};
        expect(readCameraPreview(still)).toEqual(still);
        expect(() => readCameraPreview({...still, sample: 1})).toThrow('Invalid');
        api.open.mockResolvedValueOnce({...still, camera_key: 'another'});
        owner.open(scoped);
        await vi.waitFor(() => expect(api.close).toHaveBeenCalledWith(first.token));
        expect(owner.state()).toBeNull();
        api.open.mockResolvedValueOnce(still);
        owner.open(scoped);
        await vi.waitFor(() => expect(owner.state()?.camera_key).toBe('camera'));
        owner.sample(turn);
        expect(api.sample).not.toHaveBeenCalled();
        owner.close(scoped);
    });
});
