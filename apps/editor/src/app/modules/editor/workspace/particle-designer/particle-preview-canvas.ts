import {afterRenderEffect, Component, DestroyRef, DOCUMENT, ElementRef, inject, input, viewChild} from '@angular/core';
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
    readonly running = input(true);
    readonly restart = input(0);
    private readonly canvas = viewChild.required<ElementRef<HTMLCanvasElement>>('canvas');
    private readonly window = inject(DOCUMENT).defaultView;
    private context: CanvasRenderingContext2D | null = null;
    private preview: ParticlePreview | null = null;
    private configuration: ParticleEmitterSettings | null = null;
    private restartToken = 0;
    private resizeObserver: ResizeObserver | null = null;
    private observing = false;
    private frame = 0;
    private previousTime: number | null = null;

    constructor() {
        afterRenderEffect(() => {
            const canvas = this.canvas().nativeElement;
            const settings = this.settings();
            const restart = this.restart();
            const running = this.running();
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
            context.fillRect(x - side / 2, y - side / 2, side, side);
        }
    }
}
