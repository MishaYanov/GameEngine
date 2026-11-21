const minimumWaveSize = 46;
const maximumWaveSize = 16_044;
const previewGain = 0.35;

export function decodeCue(context: BaseAudioContext, bytes: unknown): Promise<AudioBuffer> {
    if (!Array.isArray(bytes) || bytes.length < minimumWaveSize || bytes.length > maximumWaveSize ||
        !bytes.every(value => Number.isInteger(value) && value >= 0 && value <= 255)) {
        return Promise.reject(new Error('Invalid audio preview bytes.'));
    }
    const buffer = new ArrayBuffer(bytes.length);
    new Uint8Array(buffer).set(bytes);
    return context.decodeAudioData(buffer);
}

export class CuePreview {
    private context: AudioContext | null = null;
    private source: AudioBufferSourceNode | null = null;
    private gain: GainNode | null = null;
    private generation = 0;

    constructor(private readonly createContext: () => AudioContext = () => new AudioContext()) {
    }

    async play(load: () => Promise<unknown>, active: () => boolean, ended: () => void): Promise<void> {
        this.stop();
        const generation = this.generation;
        const current = () => this.generation === generation && active();
        const context = this.context ??= this.createContext();
        // Start resuming from the button gesture before awaiting native I/O or decoding.
        const resumed = context.resume();
        const [bytes] = await Promise.all([Promise.resolve().then(load), resumed]);
        if (!current()) return;
        const buffer = await decodeCue(context, bytes);
        if (!current()) return;

        const source = context.createBufferSource();
        const gain = context.createGain();
        this.source = source;
        this.gain = gain;
        try {
            source.buffer = buffer;
            gain.gain.value = previewGain;
            source.connect(gain);
            gain.connect(context.destination);
            source.onended = () => {
                if (this.source !== source) return;
                this.stop();
                ended();
            };
            source.start();
        } catch (error) {
            this.stop();
            throw error;
        }
    }

    stop(): void {
        this.generation++;
        const source = this.source;
        this.source = null;
        if (source) {
            source.onended = null;
            try {
                source.stop();
            } catch {
                // Construction can fail before start; disconnect either way.
            }
            source.disconnect();
        }
        this.gain?.disconnect();
        this.gain = null;
    }

    close(): void {
        this.stop();
        const context = this.context;
        this.context = null;
        if (context) void context.close().catch(() => {
        });
    }
}
