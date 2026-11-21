import {CuePreview, decodeCue} from './cue-preview';

const wave = Array.from({length: 46}, () => 0);

function deferred<T>() {
    let resolve!: (value: T) => void;
    const promise = new Promise<T>(complete => {
        resolve = complete;
    });
    return {promise, resolve};
}

function fakeDevice() {
    const create = () => ({
        buffer: null as AudioBuffer | null,
        connect: vi.fn(),
        disconnect: vi.fn(),
        start: vi.fn(),
        stop: vi.fn(),
        onended: null as (() => void) | null,
    });
    const source = create();
    const gain = {gain: {value: 1}, connect: vi.fn(), disconnect: vi.fn()};
    const context = {
        resume: vi.fn(async () => {
        }),
        close: vi.fn(async () => {
        }),
        decodeAudioData: vi.fn(async (_: ArrayBuffer) => ({} as AudioBuffer)),
        createBufferSource: vi.fn(() => source),
        createGain: vi.fn(() => gain),
        destination: {},
    };
    const preview = new CuePreview(() => context as unknown as AudioContext);
    return {context, preview, source, gain, create};
}

describe(CuePreview, () => {
    it('resumes from a user gesture and stops any existing audition before starting another', async () => {
        const {context, preview, source, gain, create} = fakeDevice();
        const second = create();
        const load = vi.fn(async () => wave);
        const ended = vi.fn();
        await preview.play(load, () => true, ended);
        expect(context.resume.mock.invocationCallOrder[0]).toBeLessThan(load.mock.invocationCallOrder[0]);
        expect(gain.gain.value).toBe(0.35);
        expect(source.start).toHaveBeenCalledOnce();
        context.createBufferSource.mockReturnValueOnce(second);
        await preview.play(load, () => true, ended);
        expect(source.stop).toHaveBeenCalledOnce();
        expect(source.disconnect).toHaveBeenCalledOnce();
        expect(second.start).toHaveBeenCalledOnce();
        second.onended?.();
        expect(ended).toHaveBeenCalledOnce();
        expect(second.disconnect).toHaveBeenCalledOnce();
    });

    it('discards a delayed read after close and releases the device', async () => {
        const {context, preview, source} = fakeDevice();
        const response = deferred<unknown>();
        const playing = preview.play(() => response.promise, () => true, vi.fn());
        preview.close();
        response.resolve(wave);
        await playing;
        expect(context.close).toHaveBeenCalledOnce();
        expect(context.decodeAudioData).not.toHaveBeenCalled();
        expect(source.start).not.toHaveBeenCalled();
    });

    it('discards decoded data when the project is no longer current', async () => {
        const {context, preview, source} = fakeDevice();
        const decode = deferred<AudioBuffer>();
        const started = deferred<void>();
        context.decodeAudioData.mockImplementationOnce(() => {
            started.resolve(undefined);
            return decode.promise;
        });
        let active = true;
        const playing = preview.play(async () => wave, () => active, vi.fn());
        await started.promise;
        active = false;
        decode.resolve({} as AudioBuffer);
        await playing;
        expect(context.decodeAudioData).toHaveBeenCalledOnce();
        expect(source.start).not.toHaveBeenCalled();
    });

    it('rejects invalid payloads and an unavailable device without starting a source', async () => {
        const {context, preview, source} = fakeDevice();
        const browser = context as unknown as AudioContext;
        await expect(decodeCue(browser, new Array(16_045))).rejects.toThrow('Invalid audio preview bytes');
        await expect(decodeCue(browser, [...wave, 256])).rejects.toThrow('Invalid audio preview bytes');
        context.resume.mockRejectedValueOnce(new Error('Device unavailable'));
        await expect(preview.play(async () => wave, () => true, vi.fn())).rejects.toThrow('Device unavailable');
        expect(source.start).not.toHaveBeenCalled();
    });
});
