import {
    afterRenderEffect,
    Component,
    DestroyRef,
    DOCUMENT,
    effect,
    ElementRef,
    inject,
    input,
    signal,
    viewChild
} from '@angular/core';
import type {ParticleEmitterSettings} from '../../../../core/editor/editor.models';
import {ParticlePreview} from './particle-preview';

@Component({
    selector: 'editor-particle-preview',
    template: '<canvas #canvas role="img" aria-label="Animated particle preview"></canvas>',
    styles: `
        :host, canvas {
            display: block;
            inline-size: 100%;
        }
        canvas {
            block-size: min(28rem, 48dvh);
            min-block-size: 12rem;
            border: var(--border-width) solid var(--color-border);
            border-radius: .5rem;
            background: radial-gradient(ellipse at center, #21314a, #101724);
        }
    `,
})
export class ParticlePreviewCanvas {
    readonly settings = input<ParticleEmitterSettings | null>(null);
    readonly sprite = input<Blob | null>(null);
    readonly running = input(true);
    readonly restart = input(0);
    private readonly canvas = viewChild.required<ElementRef<HTMLCanvasElement>>('canvas');
    private readonly document = inject(DOCUMENT);
    private readonly window = this.document.defaultView;
    private readonly spriteBitmap = signal<ImageBitmap | null>(null);
    private spritePreview: HTMLCanvasElement | null = null;
    private spriteTint = '';
    private preparedBitmap: ImageBitmap | null = null;
    private context: CanvasRenderingContext2D | null = null;
    private preview: ParticlePreview | null = null;
    private configuration: ParticleEmitterSettings | null = null;
    private restartToken = 0;
    private resizeObserver: ResizeObserver | null = null;
    private observing = false;
    private frame = 0;
    private previousTime: number | null = null;

    constructor() {
        effect(onCleanup => {
            const sprite = this.sprite();
            this.spriteBitmap.set(null);
            if (!sprite || typeof createImageBitmap !== 'function') return;
            let current = true;
            let bitmap: ImageBitmap | null = null;
            onCleanup(() => {
                current = false;
                bitmap?.close();
            });
            void createImageBitmap(sprite).then(decoded => {
                if (current) {
                    bitmap = decoded;
                    this.spriteBitmap.set(decoded);
                } else decoded.close();
            }, () => undefined);
        });
        afterRenderEffect(() => {
            const canvas = this.canvas().nativeElement;
            const settings = this.settings();
            const restart = this.restart();
            const running = this.running();
            const bitmap = this.spriteBitmap();
            this.stop();
            if (!this.window) return;
            if (!this.observing) {
                this.context = canvas.getContext('2d');
                if (!this.context) return;
                this.observing = true;
                if (typeof ResizeObserver !== 'undefined') {
                    this.resizeObserver = new ResizeObserver(this.resize);
                    this.resizeObserver.observe(canvas);
                } else this.window.addEventListener('resize', this.resize);
            }
            if (settings !== this.configuration || restart !== this.restartToken) {
                this.configuration = settings;
                this.restartToken = restart;
                this.preview = settings ? new ParticlePreview(settings) : null;
            }
            this.prepareSprite(bitmap, settings?.color ?? [1, 1, 1]);
            this.resize();
            if (running && this.preview && this.preview.settings.rate_per_second > 0) {
                this.frame = this.window.requestAnimationFrame(this.animate);
            }
        });
        inject(DestroyRef).onDestroy(() => {
            this.stop();
            this.resizeObserver?.disconnect();
            this.window?.removeEventListener('resize', this.resize);
        });
    }

    private prepareSprite(bitmap: ImageBitmap | null, color: ParticleEmitterSettings['color']): void {
        const tint = color.join(',');
        if (bitmap === this.preparedBitmap && tint === this.spriteTint) return;
        this.preparedBitmap = bitmap;
        this.spriteTint = tint;
        if (!bitmap) {
            this.spritePreview = null;
            return;
        }
        const canvas = this.spritePreview ?? this.document.createElement('canvas');
        canvas.width = bitmap.width;
        canvas.height = bitmap.height;
        const context = canvas.getContext('2d', {willReadFrequently: true});
        if (!context) return;
        context.drawImage(bitmap, 0, 0);
        const image = context.getImageData(0, 0, canvas.width, canvas.height);
        const [red, green, blue] = color.map(tintLookup);
        const pixels = image.data;
        for (let index = 0; index < pixels.length; index += 4) {
            pixels[index] = red[pixels[index]];
            pixels[index + 1] = green[pixels[index + 1]];
            pixels[index + 2] = blue[pixels[index + 2]];
            pixels[index + 3] = pixels[index + 3] < 128 ? 0 : 255;
        }
        context.putImageData(image, 0, 0);
        this.spritePreview = canvas;
    }

    private readonly resize = (): void => {
        if (!this.context) return;
        const canvas = this.context.canvas;
        const width = canvas.clientWidth;
        const height = canvas.clientHeight;
        if (!width || !height) return;
        const ratio = Math.min(this.window?.devicePixelRatio ?? 1, 2, 2048 / Math.max(width, height));
        const physicalWidth = Math.round(width * ratio);
        const physicalHeight = Math.round(height * ratio);
        if (canvas.width !== physicalWidth || canvas.height !== physicalHeight) {
            canvas.width = physicalWidth;
            canvas.height = physicalHeight;
        }
        this.draw();
    };

    private readonly animate = (time: number): void => {
        if (this.previousTime !== null) this.preview?.advance((time - this.previousTime) / 1000);
        this.previousTime = time;
        this.draw();
        this.frame = this.window?.requestAnimationFrame(this.animate) ?? 0;
    };

    private stop(): void {
        if (this.frame) this.window?.cancelAnimationFrame(this.frame);
        this.frame = 0;
        this.previousTime = null;
    }

    private draw(): void {
        const context = this.context;
        if (!context) return;
        const {width, height} = context.canvas;
        context.clearRect(0, 0, width, height);
        const preview = this.preview;
        if (!preview) return;
        const {color, gravity, lifetime_seconds: life, size, speed, spread_degrees: spread} = preview.settings;
        const launch = speed * Math.cos(spread * Math.PI / 180);
        const apex = gravity < 0 ? Math.min(life, -speed / gravity) : life;
        const top = Math.max(size * 2, speed * apex + gravity * apex * apex / 2);
        const bottom = Math.min(0, launch * life + gravity * life * life / 2);
        const radius = size + speed * life * Math.sin(spread * Math.PI / 180) * 1.12;
        const scale = Math.min(width * .4 / radius, height * .76 / (top - bottom));
        const center = width / 2;
        const origin = (height + scale * (top + bottom)) / 2;
        context.strokeStyle = '#68809c';
        context.lineWidth = Math.max(1, width / 490);
        context.beginPath();
        context.moveTo(width * .12, origin);
        context.lineTo(width * .88, origin);
        context.moveTo(center, origin - 5 * context.lineWidth);
        context.lineTo(center, origin + 5 * context.lineWidth);
        context.stroke();

        context.fillStyle = `rgb(${color.map(channel => Math.round(channel * 255)).join(' ')})`;
        for (const particle of preview.positions()) {
            const y = origin - particle.y * scale;
            const x = center + (particle.x + particle.z * .48) * scale;
            const side = Math.max(1, particle.size * scale);
            const radius = side / 2;
            if (this.spritePreview) {
                context.drawImage(this.spritePreview, x - radius, y - radius, side, side);
            } else {
                context.beginPath();
                context.moveTo(x, y - radius);
                context.lineTo(x + radius, y);
                context.lineTo(x, y + radius);
                context.lineTo(x - radius, y);
                context.fill();
            }
        }
    }
}

function tintLookup(tint: number): Uint8ClampedArray {
    return Uint8ClampedArray.from({length: 256}, (_, byte) => {
        const srgb = byte / 255;
        const linear = (srgb <= 0.04045 ? srgb / 12.92 : ((srgb + 0.055) / 1.055) ** 2.4) * tint;
        return 255 * (linear <= 0.0031308 ? linear * 12.92 : 1.055 * linear ** (1 / 2.4) - 0.055);
    });
}
