import {
    afterNextRender,
    Component,
    computed,
    DestroyRef,
    DOCUMENT,
    ElementRef,
    inject,
    input,
    output,
    signal,
    viewChild,
} from '@angular/core';
import {EngineViewportDirective} from '../../../../core/editor/engine-viewport.directive';
import {resolveNativeEditorShortcut} from '../../../../core/editor/editor-keymap';
import type {EditorActionId} from '../../../../core/editor/editor-actions';
import {EditorClientError, EditorService} from '../../../../core/editor/editor.service';
import type {ViewportInput, ViewportStatus} from '../../../../core/editor/editor.models';
import {EditorIcon} from '../../../../shared/icons/editor-icon';
import {ViewportEditorService} from '../../../../core/editor/viewport-editor.service';
import {ContextMenuService} from '../../../../shared/components/context-menu/context-menu.service';

/** Owns a native Vulkan viewport through the existing, serialized engine bridge. */
@Component({
    selector: 'scene-viewport',
    imports: [EngineViewportDirective, EditorIcon],
    templateUrl: './scene-viewport.html',
    styleUrl: './scene-viewport.scss',
})
export class SceneViewport {
    readonly label = input('Scene viewport');
    readonly enabled = input(true);
    readonly viewportInput = output<ViewportInput>();
    readonly editorActionRequested = output<EditorActionId>();
    readonly viewportStatus = output<ViewportStatus>();
    readonly viewportError = output<EditorClientError>();
    protected readonly nativeAvailable = inject(EditorService).nativeAvailable;
    protected readonly error = signal<string | null>(null);
    private readonly domOverlayOpen = signal(false);
    private readonly contextMenu = inject(ContextMenuService);
    protected readonly active = computed(() => this.enabled() && !this.domOverlayOpen() && !this.contextMenu.request());
    private readonly viewport = viewChild<EngineViewportDirective>('viewport');
    private readonly surface = viewChild<ElementRef<HTMLElement>>('surface');
    private readonly viewTools = inject(ViewportEditorService, {optional: true});
    private releaseSurface: (() => void) | null = null;
    private readonly document = inject(DOCUMENT);
    private observer: MutationObserver | undefined;
    private readonly updateDomOverlays = (): void => {
        const popoversSupported = this.document.defaultView?.CSS?.supports('selector(:popover-open)');
        this.domOverlayOpen.set(!!this.document.querySelector('dialog:modal') ||
            (popoversSupported === true && !!this.document.querySelector(':popover-open')));
    };

    constructor() {
        afterNextRender(() => {
            if (!this.nativeAvailable) return;
            const surface = this.surface();
            if (surface) this.releaseSurface = this.viewTools?.registerSurface(surface.nativeElement) ?? null;
            // Native child surfaces sit above WebView dialogs and popovers.
            this.updateDomOverlays();
            this.document.addEventListener('toggle', this.updateDomOverlays, true);
            const overlaySelector = 'dialog, [popover]';
            this.observer = new MutationObserver((records) => {
                if (
                    records.some(
                        (record) =>
                            (record.type === 'attributes' && (record.attributeName === 'popover' || record.target instanceof HTMLDialogElement)) ||
                            [...record.addedNodes, ...record.removedNodes].some(
                                (node) =>
                                    node instanceof Element &&
                                    (node.matches(overlaySelector) || !!node.querySelector(overlaySelector)),
                            ),
                    )
                )
                    this.updateDomOverlays();
            });
            this.observer.observe(this.document.body, {
                subtree: true,
                childList: true,
                attributes: true,
                attributeFilter: ['open', 'popover'],
            });
        });
        inject(DestroyRef).onDestroy(() => {
            this.observer?.disconnect();
            this.document.removeEventListener('toggle', this.updateDomOverlays, true);
            this.releaseSurface?.();
        });
    }

    /** Notify after a dock/drag operation that moves the viewport without resizing it. */
    refreshBounds(): void {
        this.viewport()?.refreshBounds();
    }

    protected inputReceived(input: ViewportInput): void {
        if (!this.active()) return;
        this.viewportInput.emit(input);
        const action = resolveNativeEditorShortcut(input);
        if (action) this.editorActionRequested.emit(action);
    }

    protected statusChanged(status: ViewportStatus): void {
        this.error.set(status.error);
        if (status.attached && !status.error) void this.viewTools?.start().catch(() => undefined);
        this.viewportStatus.emit(status);
    }

    protected failed(error: EditorClientError): void {
        this.error.set(error.message);
        this.viewportError.emit(error);
    }
}
