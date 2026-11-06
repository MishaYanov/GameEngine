import {computed, DestroyRef, DOCUMENT, effect, inject, Injectable, signal} from '@angular/core';
import {ViewportEditorService} from '../../../core/editor/viewport-editor.service';
import {EditorService} from '../../../core/editor/editor.service';
import {SceneEditingService} from './scene-editing.service';

interface AssetDrag {
    readonly path: string;
    readonly pointerId: number;
    readonly element: HTMLElement;
    readonly start: readonly [number, number];
    point: readonly [number, number];
    beginning: Promise<boolean> | null;
    ready: boolean;
    ending: boolean;
    cancelled: boolean;
}

/** Capturing the WebView pointer keeps one internal drag alive over the HWND child. */
@Injectable()
export class ViewportAssetDragService {
    private readonly viewport = inject(ViewportEditorService);
    private readonly editing = inject(SceneEditingService);
    private readonly editor = inject(EditorService);
    private readonly document = inject(DOCUMENT);
    private readonly view = this.document.defaultView;
    private active: AssetDrag | null = null;
    private readonly draggingState = signal(false);
    readonly dragging = this.draggingState.asReadonly();
    private readonly messageState = signal<string | null>(null);
    private readonly awaitingPlacement = signal<{generation: number | null} | null>(null);
    readonly message = this.messageState.asReadonly();
    readonly canBegin = computed(() => this.viewport.nativeAvailable && this.editing.ready() &&
        !this.editor.playing() && this.viewport.settings().view === 'scene' && !this.dragging());

    constructor() {
        effect(() => {
            const pending = this.awaitingPlacement();
            const state = this.viewport.state();
            if (!pending || !state) return;
            if (pending.generation !== state.workspace_generation) {
                this.awaitingPlacement.set(null);
                this.messageState.set('Placement ended because the open project changed.');
            } else if (state.gesture === 'asset_loading') {
                this.messageState.set('Loading and placing model…');
            } else if (state.error) {
                this.awaitingPlacement.set(null);
                this.messageState.set(state.error);
            } else if (!state.placement) {
                this.awaitingPlacement.set(null);
                this.messageState.set('Model placement finished.');
            }
        });
        this.document.addEventListener('pointermove', this.moved, true);
        this.document.addEventListener('pointerup', this.released, true);
        this.document.addEventListener('pointercancel', this.cancelled, true);
        this.document.addEventListener('lostpointercapture', this.lostCapture, true);
        this.document.addEventListener('keydown', this.keydown, true);
        this.view?.addEventListener('blur', this.blurred);
        inject(DestroyRef).onDestroy(() => {
            void this.cancel();
            this.document.removeEventListener('pointermove', this.moved, true);
            this.document.removeEventListener('pointerup', this.released, true);
            this.document.removeEventListener('pointercancel', this.cancelled, true);
            this.document.removeEventListener('lostpointercapture', this.lostCapture, true);
            this.document.removeEventListener('keydown', this.keydown, true);
            this.view?.removeEventListener('blur', this.blurred);
        });
    }

    candidate(path: string, event: PointerEvent): void {
        if (event.button !== 0 || !this.canBegin() || !/\.(?:gltf|glb)$/i.test(path) ||
            path.startsWith('/') || /[:\\\p{Cc}]/u.test(path) || path.split('/').some(part => !part || part === '.' || part === '..')) return;
        const element = event.currentTarget;
        if (!(element instanceof HTMLElement)) return;
        void this.cancel();
        const drag: AssetDrag = {
            path, pointerId: event.pointerId, element,
            start: [event.clientX, event.clientY], point: [event.clientX, event.clientY],
            beginning: null, ready: false, ending: false, cancelled: false,
        };
        this.active = drag;
        this.messageState.set(null);
        try {element.setPointerCapture(event.pointerId);} catch {
            this.active = null;
            this.messageState.set('Pointer capture is unavailable. Use Add to Scene instead.');
        }
    }

    async cancel(): Promise<void> {
        const drag = this.active;
        if (!drag) {
            if (this.awaitingPlacement()) {
                this.awaitingPlacement.set(null);
                try {
                    await this.viewport.action({type: 'cancel_placement'});
                    this.messageState.set('Placement cancellation requested.');
                } catch (error) {this.failed(error);}
            }
            return;
        }
        drag.cancelled = true;
        await this.finish(drag, true);
    }

    private readonly moved = (event: PointerEvent): void => {
        const drag = this.active;
        if (!drag || drag.pointerId !== event.pointerId || drag.ending) return;
        drag.point = [event.clientX, event.clientY];
        if (!drag.beginning && Math.hypot(event.clientX - drag.start[0], event.clientY - drag.start[1]) < 5) return;
        event.preventDefault();
        if (!drag.beginning) {
            this.draggingState.set(true);
            drag.beginning = this.editing.withCommittedEdit(async () => {
                await this.viewport.action({type: 'begin_placement', path: drag.path});
            });
            void drag.beginning.then(async accepted => {
                if (!accepted || drag.cancelled || drag.ending || this.active !== drag) return;
                drag.ready = true;
                await this.viewport.updatePlacement(this.viewport.normalizedPoint(...drag.point));
                this.messageState.set('Drop the model in Scene. Escape cancels placement.');
            }).catch(error => this.failed(error));
        } else if (drag.ready && !drag.cancelled) {
            void this.viewport.updatePlacement(this.viewport.normalizedPoint(...drag.point)).catch(error => this.failed(error));
        }
    };

    private readonly released = (event: PointerEvent): void => {
        const drag = this.active;
        if (!drag || drag.pointerId !== event.pointerId) return;
        drag.point = [event.clientX, event.clientY];
        if (drag.beginning) event.preventDefault();
        void this.finish(drag, false);
    };

    private readonly cancelled = (): void => {void this.cancel();};
    // The native child taking focus also blurs the WebView. A submitted import
    // remains valid while navigating there; only an unfinished pointer drag ends.
    private readonly blurred = (): void => {if (this.active && !this.active.ending) void this.cancel();};
    private readonly lostCapture = (event: PointerEvent): void => {
        if (this.active?.pointerId === event.pointerId && !this.active.ending) void this.cancel();
    };
    private readonly keydown = (event: KeyboardEvent): void => {
        if (event.key !== 'Escape' || (!this.active && !this.awaitingPlacement())) return;
        event.preventDefault();
        event.stopPropagation();
        void this.cancel();
    };

    private async finish(drag: AssetDrag, cancel: boolean): Promise<void> {
        if (drag.ending) {
            if (cancel) drag.cancelled = true;
            return;
        }
        drag.ending = true;
        drag.cancelled ||= cancel;
        try {
            if (!drag.beginning) return;
            const accepted = await drag.beginning;
            if (!accepted) {
                this.messageState.set(this.editing.error() ?? 'Finish the current edit before dragging an asset.');
                return;
            }
            const normalized = this.viewport.normalizedPoint(...drag.point);
            if (drag.cancelled || normalized === null) {
                await this.viewport.action({type: 'cancel_placement'});
                this.messageState.set('Model placement cancelled.');
            } else {
                // Flush the final coordinate before the non-replaceable drop boundary.
                await this.viewport.updatePlacement(normalized);
                const result = await this.viewport.action({type: drag.cancelled ? 'cancel_placement' : 'commit_placement'});
                if (drag.cancelled) this.messageState.set('Model placement cancelled.');
                else {
                    this.awaitingPlacement.set({generation: result.workspace_generation ?? null});
                    this.messageState.set('Loading and placing model…');
                }
            }
        } catch (error) {
            this.failed(error);
            try {await this.viewport.action({type: 'cancel_placement'});} catch {/* Retain the original error. */}
        } finally {
            if (this.active === drag) {
                this.active = null;
                this.draggingState.set(false);
            }
            try {if (drag.element.hasPointerCapture(drag.pointerId)) drag.element.releasePointerCapture(drag.pointerId);} catch {/* Capture already ended. */}
        }
    }

    private failed(error: unknown): void {
        this.messageState.set(error instanceof Error ? error.message : String(error));
    }
}
