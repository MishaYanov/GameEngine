import {computed, DestroyRef, DOCUMENT, effect, inject, Injectable, signal} from '@angular/core';
import type {SceneId} from '../../../core/editor/editor.models';
import {ViewportEditorService} from '../../../core/editor/viewport-editor.service';
import {EditorService} from '../../../core/editor/editor.service';
import {SceneEditingService} from './scene-editing.service';

type DragAsset =
    | { readonly kind: 'model'; readonly path: string }
    | { readonly kind: 'scene'; readonly id: SceneId; readonly open: (id: SceneId) => Promise<boolean> };

interface AssetDrag {
    readonly asset: DragAsset;
    readonly workspace: number;
    readonly scene: SceneId | null;
    readonly pointerId: number;
    readonly element: HTMLElement;
    readonly start: readonly [number, number];
    point: readonly [number, number];
    started: boolean;
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
    private readonly awaitingPlacement = signal<(Pick<AssetDrag, 'workspace' | 'scene'> & {
        generation: number | null
    }) | null>(null);
    readonly message = this.messageState.asReadonly();
    readonly canBegin = computed(() => this.viewport.nativeAvailable && this.editing.ready() &&
        !this.editor.playing() && this.viewport.settings().view === 'scene' && !this.dragging() && !this.awaitingPlacement());

    constructor() {
        effect(() => {
            const pending = this.awaitingPlacement();
            const state = this.viewport.state();
            if (!pending) return;
            if (!this.isCurrent(pending) || (state && pending.generation !== state.workspace_generation)) {
                this.awaitingPlacement.set(null);
                this.messageState.set('Placement ended because the open project changed.');
            } else if (state?.gesture === 'asset_loading') {
                this.messageState.set('Loading and placing model…');
            } else if (state?.error) {
                this.awaitingPlacement.set(null);
                this.messageState.set(state.error);
            } else if (state && !state.placement) {
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
        if (!/\.(?:gltf|glb)$/i.test(path) ||
            path.startsWith('/') || /[:\\\p{Cc}]/u.test(path) || path.split('/').some(part => !part || part === '.' || part === '..')) return;
        this.capture({kind: 'model', path}, event);
    }

    candidateScene(id: SceneId, event: PointerEvent, open: (id: SceneId) => Promise<boolean>): void {
        this.capture({kind: 'scene', id, open}, event);
    }

    async cancel(): Promise<void> {
        const drag = this.active;
        if (!drag) {
            const pending = this.awaitingPlacement();
            if (pending) {
                this.awaitingPlacement.set(null);
                if (!this.isCurrent(pending)) return;
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

    private capture(asset: DragAsset, event: PointerEvent): void {
        if (event.button !== 0 || event.isPrimary === false || !this.canBegin()) return;
        const element = event.currentTarget;
        if (!(element instanceof HTMLElement)) return;
        void this.cancel();
        const drag: AssetDrag = {
            asset, workspace: this.editor.workspaceGeneration(), scene: this.editor.snapshot()?.active_scene_id ?? null,
            pointerId: event.pointerId, element,
            start: [event.clientX, event.clientY], point: [event.clientX, event.clientY],
            started: false, beginning: null, ready: false, ending: false, cancelled: false,
        };
        this.active = drag;
        this.messageState.set(null);
        try {
            element.setPointerCapture(event.pointerId);
        } catch {
            this.active = null;
            const alternative = asset.kind === 'scene' ? 'Open Scene' : 'Add to Scene';
            this.messageState.set(`Pointer capture is unavailable. Use ${alternative} instead.`);
        }
    }

    private readonly moved = (event: PointerEvent): void => {
        const drag = this.active;
        if (!drag || drag.pointerId !== event.pointerId || drag.ending) return;
        if (!this.isCurrent(drag)) {
            void this.cancel();
            return;
        }
        drag.point = [event.clientX, event.clientY];
        if (!drag.started && Math.hypot(event.clientX - drag.start[0], event.clientY - drag.start[1]) < 5) return;
        event.preventDefault();
        if (!drag.started) {
            drag.started = true;
            this.draggingState.set(true);
            const asset = drag.asset;
            if (asset.kind === 'scene') {
                this.messageState.set('Drop the scene file in Scene to open it. Escape cancels.');
                return;
            }
            drag.beginning = this.editing.withCommittedEdit(async () => {
                if (this.isCurrent(drag)) await this.viewport.action({type: 'begin_placement', path: asset.path});
            });
            void drag.beginning.then(async accepted => {
                if (!accepted || drag.cancelled || drag.ending || this.active !== drag || !this.isCurrent(drag)) return;
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
        if (drag.started) event.preventDefault();
        void this.finish(drag, false);
    };

    private readonly cancelled = (event: PointerEvent): void => {
        if (this.active?.pointerId === event.pointerId) void this.cancel();
    };
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
        if (drag.asset.kind === 'scene') {
            const {id, open} = drag.asset;
            const dropped = drag.started && !drag.cancelled && this.isCurrent(drag) &&
                this.viewport.normalizedPoint(...drag.point) !== null;
            this.release(drag);
            this.messageState.set(null);
            // A valid drop can open a modal. Release the captured pointer before showing it.
            if (dropped) {
                try {
                    await open(id);
                } catch (error) {
                    this.failed(error);
                }
            }
            return;
        }
        try {
            if (!drag.beginning || !this.isCurrent(drag)) return;
            const accepted = await drag.beginning;
            if (!this.isCurrent(drag)) return;
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
                if (!this.isCurrent(drag)) return;
                const result = await this.viewport.action({type: drag.cancelled ? 'cancel_placement' : 'commit_placement'});
                if (drag.cancelled) this.messageState.set('Model placement cancelled.');
                else {
                    this.awaitingPlacement.set({
                        workspace: drag.workspace,
                        scene: drag.scene,
                        generation: result.workspace_generation ?? null
                    });
                    this.messageState.set('Loading and placing model…');
                }
            }
        } catch (error) {
            this.failed(error);
            if (this.isCurrent(drag)) {
                try {
                    await this.viewport.action({type: 'cancel_placement'});
                } catch {/* Retain the original error. */
                }
            }
        } finally {
            this.release(drag);
        }
    }

    private isCurrent(context: Pick<AssetDrag, 'workspace' | 'scene'>): boolean {
        return this.editor.workspaceGeneration() === context.workspace &&
            (this.editor.snapshot()?.active_scene_id ?? null) === context.scene;
    }

    private release(drag: AssetDrag): void {
        if (this.active === drag) {
            this.active = null;
            this.draggingState.set(false);
        }
        try {
            if (drag.element.hasPointerCapture(drag.pointerId)) drag.element.releasePointerCapture(drag.pointerId);
        } catch {/* Capture already ended. */
        }
    }

    private failed(error: unknown): void {
        this.messageState.set(error instanceof Error ? error.message : String(error));
    }
}
