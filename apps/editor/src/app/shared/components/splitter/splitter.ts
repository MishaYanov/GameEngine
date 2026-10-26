import {Component, DestroyRef, DOCUMENT, inject, input, output, signal} from '@angular/core';

@Component({
    selector: 'app-splitter',
    imports: [],
    templateUrl: './splitter.html',
    styleUrl: './splitter.scss',
    host: {
        role: 'separator',
        tabindex: '0',
        '[attr.aria-label]': 'label()',
        '[attr.aria-orientation]': 'orientation()',
        '[attr.aria-valuenow]': 'value()',
        '[attr.aria-valuemin]': 'minimum()',
        '[attr.aria-valuemax]': 'maximum()',
        '[class.horizontal]': 'orientation() === "horizontal"',
        '[class.dragging]': 'dragging()',
        '(pointerdown)': 'start($event)',
        '(pointermove)': 'move($event)',
        '(pointerup)': 'finish($event)',
        '(pointercancel)': 'finish($event)',
        '(lostpointercapture)': 'finish($event)',
        '(keydown)': 'onKey($event)',
        '(dblclick)': 'reset.emit()',
    },
})
export class Splitter {
    readonly orientation = input<'vertical' | 'horizontal'>('vertical');
    readonly label = input('Resize drawer');
    readonly value = input(0);
    readonly minimum = input(0);
    readonly maximum = input(1000);
    readonly resize = output<number>();
    readonly edge = output<'min' | 'max'>();
    readonly reset = output<void>();
    protected readonly dragging = signal(false);
    private pointer: number | undefined;
    private previous = 0;
    private delta = 0;
    private frame: number | undefined;
    private readonly view = inject(DOCUMENT).defaultView;

    constructor() {
        inject(DestroyRef).onDestroy(() => {
            if (this.frame !== undefined) this.view?.cancelAnimationFrame(this.frame);
        });
    }

    protected start(event: PointerEvent): void {
        if (event.button !== 0 || event.isPrimary === false) return;
        const element = event.currentTarget as HTMLElement;
        element.setPointerCapture(event.pointerId);
        element.focus({preventScroll: true});
        this.pointer = event.pointerId;
        this.previous = this.coordinate(event);
        this.dragging.set(true);
        event.preventDefault();
    }

    protected move(event: PointerEvent): void {
        if (event.pointerId !== this.pointer || !this.dragging()) return;
        const position = this.coordinate(event);
        this.delta += position - this.previous;
        this.previous = position;
        if (this.frame === undefined)
            this.frame = this.view?.requestAnimationFrame(() => {
                this.frame = undefined;
                this.flush();
            });
    }

    protected finish(event: PointerEvent): void {
        if (event.pointerId !== this.pointer) return;
        if (this.frame !== undefined) this.view?.cancelAnimationFrame(this.frame);
        this.frame = undefined;
        this.flush();
        this.pointer = undefined;
        this.dragging.set(false);
        const element = event.currentTarget as HTMLElement;
        if (element.hasPointerCapture(event.pointerId)) element.releasePointerCapture(event.pointerId);
    }

    protected onKey(event: KeyboardEvent): void {
        const horizontal = this.orientation() === 'horizontal';
        const negative = horizontal ? 'ArrowUp' : 'ArrowLeft';
        const positive = horizontal ? 'ArrowDown' : 'ArrowRight';
        if (event.key === negative || event.key === positive) {
            this.resize.emit((event.key === negative ? -1 : 1) * (event.shiftKey ? 40 : 10));
        } else if (event.key === 'Home' || event.key === 'End') {
            this.edge.emit(event.key === 'Home' ? 'min' : 'max');
        } else return;
        event.preventDefault();
    }

    private flush(): void {
        const delta = this.delta;
        this.delta = 0;
        if (delta) this.resize.emit(delta);
    }

    private coordinate(event: PointerEvent): number {
        return this.orientation() === 'vertical' ? event.clientX : event.clientY;
    }
}
