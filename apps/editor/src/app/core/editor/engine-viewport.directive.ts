import {
    afterNextRender,
    booleanAttribute,
    DestroyRef,
    Directive,
    DOCUMENT,
    effect,
    ElementRef,
    inject,
    input,
    output,
} from '@angular/core';
import type {UnlistenFn} from '@tauri-apps/api/event';
import type {ViewportBounds, ViewportInput, ViewportStatus} from './editor.models';
import {EditorClientError, EditorService} from './editor.service';

/** Attach the native child to an existing Angular pane.scss; supplies no layout or shortcuts. */
@Directive({selector: '[engineViewport]', exportAs: 'engineViewport'})
export class EngineViewportDirective {
    readonly engineViewport = input(true, {transform: booleanAttribute});
    readonly engineViewportInput = output<ViewportInput>();
    readonly engineViewportStatus = output<ViewportStatus>();
    readonly engineViewportError = output<EditorClientError>();
    private readonly editor = inject(EditorService);
    private readonly destroyRef = inject(DestroyRef);
    private readonly document = inject(DOCUMENT);
    private readonly element = inject<ElementRef<HTMLElement>>(ElementRef).nativeElement;
    private readonly view = this.document.defaultView;
    private resizeObserver: ResizeObserver | null = null;
    private intersectionObserver: IntersectionObserver | null = null;
    private stopInput: UnlistenFn | null = null;
    private releaseViewport: (() => void) | null = null;
    private frame: number | null = null;
    private latestBounds: ViewportBounds | null = null;
    private lastBounds: ViewportBounds | null = null;
    private flushing = false;
    private attached = false;
    private intersecting = false;
    private ready = false;
    private destroyed = false;
    private failed = false;

    constructor() {
        effect(() => {
            this.engineViewport();
            this.schedule();
        });
        effect(() => {
            const status = this.editor.viewport();
            if (status && this.releaseViewport && !this.destroyed) this.engineViewportStatus.emit(status);
        });
        afterNextRender(() => this.start());
        this.destroyRef.onDestroy(() => this.stop());
    }

    /** Call after docking/repositioning that changes position without a resize. */
    refreshBounds(): void {
        this.failed = false;
        this.schedule();
    }

    private start(): void {
        if (this.destroyed || !this.view) return;
        if (!this.editor.nativeAvailable) {
            this.engineViewportError.emit(
                new EditorClientError(
                    'unavailable',
                    'editor_viewport_attach',
                    'The embedded Vulkan viewport is available in the Tauri desktop editor.',
                ),
            );
            return;
        }
        try {
            this.releaseViewport = this.editor.reserveViewport();
            this.resizeObserver = new ResizeObserver(() => this.schedule());
            this.resizeObserver.observe(this.element);
            this.intersectionObserver = new IntersectionObserver((entries) => {
                this.intersecting = entries.some(
                    (entry) => entry.target === this.element && entry.isIntersecting,
                );
                this.schedule();
            });
            this.intersectionObserver.observe(this.element);
            this.view.addEventListener('resize', this.onGeometryChange, {passive: true});
            this.view.addEventListener('scroll', this.onGeometryChange, {capture: true, passive: true});
            this.document.addEventListener('visibilitychange', this.onGeometryChange);
            this.ready = true;
            void this.editor
                .listenViewportInput((event) => {
                    if (!this.destroyed && this.attached) this.engineViewportInput.emit(event);
                })
                .then((unlisten) => {
                    if (this.destroyed) unlisten();
                    else this.stopInput = unlisten;
                })
                .catch((cause: unknown) => this.report(cause));
            this.schedule();
        } catch (cause: unknown) {
            this.report(cause);
            this.stop();
        }
    }

    private readonly onGeometryChange = (): void => this.schedule();

    private schedule(): void {
        if (!this.ready || this.destroyed || this.failed || !this.view) return;
        // Hidden documents throttle animation frames. Hide the native child
        // immediately on visibility changes so it can stop allocating frame work.
        if (this.document.visibilityState === 'hidden') {
            if (this.frame !== null) this.view.cancelAnimationFrame(this.frame);
            this.frame = null;
            this.latestBounds = {...(this.lastBounds ?? this.measure()), visible: false};
            void this.flush();
            return;
        }
        if (this.frame !== null) return;
        this.frame = this.view.requestAnimationFrame(() => {
            this.frame = null;
            if (this.destroyed) return;
            this.latestBounds = this.measure();
            void this.flush();
        });
    }

    private measure(): ViewportBounds {
        const rect = this.element.getBoundingClientRect();
        const style = this.view?.getComputedStyle(this.element);
        const number = (value: string | undefined): number => Number.parseFloat(value ?? '') || 0;
        // Native pixels are positioned by the Rust host using its DPI scale. Send
        // CSS logical client coordinates here; do not multiply by devicePixelRatio.
        const scaleX = this.element.offsetWidth > 0 ? rect.width / this.element.offsetWidth : 1;
        const scaleY = this.element.offsetHeight > 0 ? rect.height / this.element.offsetHeight : 1;
        const paddingLeft = number(style?.paddingLeft);
        const paddingTop = number(style?.paddingTop);
        let left = rect.left + (this.element.clientLeft + paddingLeft) * scaleX;
        let top = rect.top + (this.element.clientTop + paddingTop) * scaleY;
        let right =
            left +
            Math.max(0, this.element.clientWidth - paddingLeft - number(style?.paddingRight)) * scaleX;
        let bottom =
            top +
            Math.max(0, this.element.clientHeight - paddingTop - number(style?.paddingBottom)) * scaleY;
        let visible =
            style?.visibility !== 'hidden' &&
            style?.display !== 'none' &&
            number(style?.opacity ?? '1') > 0;
        // Native children do not inherit CSS overflow clipping. Restrict their
        // rectangular aperture to every clipping ancestor, including scroll panes.
        let ancestor = this.element.parentElement;
        let depth = 0;
        while (ancestor && depth < 128) {
            const ancestorStyle = this.view?.getComputedStyle(ancestor);
            const ancestorRect = ancestor.getBoundingClientRect();
            const sx = ancestor.offsetWidth > 0 ? ancestorRect.width / ancestor.offsetWidth : 1;
            const sy = ancestor.offsetHeight > 0 ? ancestorRect.height / ancestor.offsetHeight : 1;
            if (clips(ancestorStyle?.overflowX)) {
                const clipLeft = ancestorRect.left + ancestor.clientLeft * sx;
                left = Math.max(left, clipLeft);
                right = Math.min(right, clipLeft + ancestor.clientWidth * sx);
            }
            if (clips(ancestorStyle?.overflowY)) {
                const clipTop = ancestorRect.top + ancestor.clientTop * sy;
                top = Math.max(top, clipTop);
                bottom = Math.min(bottom, clipTop + ancestor.clientHeight * sy);
            }
            visible =
                visible &&
                ancestorStyle?.visibility !== 'hidden' &&
                ancestorStyle?.display !== 'none' &&
                number(ancestorStyle?.opacity ?? '1') > 0;
            ancestor = ancestor.parentElement;
            depth += 1;
        }
        // Bound DOM work, and hide rather than paint across an uninspected ancestor.
        if (ancestor) visible = false;
        const x = Math.max(0, Math.min(left, this.document.documentElement.clientWidth));
        const y = Math.max(0, Math.min(top, this.document.documentElement.clientHeight));
        const width = Math.max(0, Math.min(right, this.document.documentElement.clientWidth) - x);
        const height = Math.max(0, Math.min(bottom, this.document.documentElement.clientHeight) - y);
        return {
            x,
            y,
            width,
            height,
            visible:
                this.engineViewport() &&
                this.intersecting &&
                this.document.visibilityState !== 'hidden' &&
                visible &&
                width > 0 &&
                height > 0,
        };
    }

    private async flush(): Promise<void> {
        if (this.flushing || this.destroyed || this.failed) return;
        this.flushing = true;
        try {
            while (this.latestBounds && !this.destroyed) {
                const bounds = this.latestBounds;
                this.latestBounds = null;
                if (this.lastBounds && sameBounds(this.lastBounds, bounds)) continue;
                if (!this.attached) {
                    await this.editor.attachViewport(bounds);
                    this.attached = true;
                } else {
                    await this.editor.updateViewportBounds(bounds);
                }
                this.lastBounds = bounds;
            }
        } catch (cause: unknown) {
            this.failed = true;
            this.latestBounds = null;
            this.report(cause);
            // A rejected resize must not leave an old visible game surface running
            // over a pane.scss that Angular has moved or hidden.
            await this.editor.detachViewport().catch(() => undefined);
            this.attached = false;
            this.lastBounds = null;
        } finally {
            this.flushing = false;
            if (this.latestBounds && !this.failed && !this.destroyed) void this.flush();
        }
    }

    private report(cause: unknown): void {
        if (this.destroyed) return;
        const error =
            cause instanceof EditorClientError
                ? cause
                : new EditorClientError(
                    'backend',
                    'editor_viewport_attach',
                    cause instanceof Error ? cause.message : 'The embedded viewport failed.',
                    cause,
                );
        this.engineViewportError.emit(error);
    }

    private stop(): void {
        if (this.destroyed) return;
        this.destroyed = true;
        this.ready = false;
        if (this.frame !== null && this.view) this.view.cancelAnimationFrame(this.frame);
        this.frame = null;
        this.latestBounds = null;
        this.resizeObserver?.disconnect();
        this.intersectionObserver?.disconnect();
        this.view?.removeEventListener('resize', this.onGeometryChange);
        this.view?.removeEventListener('scroll', this.onGeometryChange, true);
        this.document.removeEventListener('visibilitychange', this.onGeometryChange);
        this.stopInput?.();
        this.stopInput = null;
        if (this.releaseViewport) {
            // Lifecycle calls have their own serialized queue. This also follows an
            // attach still in flight, so destruction cannot strand a native child.
            void this.editor.detachViewport().catch(() => undefined);
            this.releaseViewport();
            this.releaseViewport = null;
        }
    }
}

function sameBounds(a: ViewportBounds, b: ViewportBounds): boolean {
    return (
        a.x === b.x &&
        a.y === b.y &&
        a.width === b.width &&
        a.height === b.height &&
        a.visible === b.visible
    );
}

function clips(overflow: string | undefined): boolean {
    return (
        overflow === 'hidden' || overflow === 'clip' || overflow === 'scroll' || overflow === 'auto'
    );
}
