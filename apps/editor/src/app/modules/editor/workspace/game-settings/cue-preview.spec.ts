import {auditionGain, CuePreview, decodeCue} from './cue-preview';

const wave = Array.from({length: 46}, () => 0);
const fullVolume = {levelPercent: 100, muted: false};

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
        await preview.play(load, fullVolume, () => true, ended);
        expect(context.resume.mock.invocationCallOrder[0]).toBeLessThan(load.mock.invocationCallOrder[0]);
        expect(gain.gain.value).toBe(0.35);
        expect(source.start).toHaveBeenCalledOnce();
        context.createBufferSource.mockReturnValueOnce(second);
        await preview.play(load, {...fullVolume, levelPercent: 50}, () => true, ended);
        expect(gain.gain.value).toBeCloseTo(0.175);
        expect(source.stop).toHaveBeenCalledOnce();
        expect(source.disconnect).toHaveBeenCalledOnce();
        expect(second.start).toHaveBeenCalledOnce();
        second.onended?.();
        expect(ended).toHaveBeenCalledOnce();
        expect(second.disconnect).toHaveBeenCalledOnce();
    });

    it('combines local, category and scene levels before the audition ceiling', async () => {
        const {preview, gain} = fakeDevice();
        const mix = {levelPercent: 75, categoryPercent: 80, scenePercent: 50, muted: false};
        await preview.play(async () => wave, mix, () => true, vi.fn());
        expect(gain.gain.value).toBeCloseTo(0.105);
        expect(auditionGain({...mix, muted: true})).toBe(0);
        expect(auditionGain({...mix, scenePercent: 0})).toBe(0);
    });

    it('discards a delayed read after close and releases the device', async () => {
        const {context, preview, source} = fakeDevice();
        const response = deferred<unknown>();
        const playing = preview.play(() => response.promise, fullVolume, () => true, vi.fn());
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
        const playing = preview.play(async () => wave, fullVolume, () => active, vi.fn());
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
        await expect(preview.play(async () => wave, fullVolume, () => true, vi.fn())).rejects.toThrow('Device unavailable');
        expect(source.start).not.toHaveBeenCalled();
    });

    it('finishes zero-volume and muted previews without opening or reading audio', async () => {
        const {context, preview, source} = fakeDevice();
        const load = vi.fn(async () => wave);
        const ended = vi.fn();
        await preview.play(load, {levelPercent: 0, muted: false}, () => true, ended);
        await preview.play(load, {levelPercent: 50, muted: true}, () => true, ended);
        expect(ended).toHaveBeenCalledTimes(2);
        expect(load).not.toHaveBeenCalled();
        expect(context.resume).not.toHaveBeenCalled();
        expect(source.start).not.toHaveBeenCalled();
    });

    it('clamps audition gain to the fixed preview ceiling', () => {
        expect(auditionGain({levelPercent: NaN, muted: false})).toBe(0);
        expect(auditionGain({levelPercent: -25, muted: false})).toBe(0);
        expect(auditionGain({levelPercent: 125, muted: false})).toBe(0.35);
    });
});
