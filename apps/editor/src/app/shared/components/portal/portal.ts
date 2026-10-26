import {
    afterNextRender,
    Component,
    computed,
    DestroyRef,
    DOCUMENT,
    inject,
    input,
    output,
    signal,
    viewChild,
} from '@angular/core';
import {EngineViewportDirective} from '../../../core/editor/engine-viewport.directive';
import {EditorClientError, EditorService} from '../../../core/editor/editor.service';
import type {ViewportInput, ViewportStatus} from '../../../core/editor/editor.models';
import {EditorIcon} from '../../icons/editor-icon';

/** Owns a native Vulkan viewport through the existing, serialized engine bridge. */
@Component({
    selector: 'engine-portal',
    imports: [EngineViewportDirective, EditorIcon],
    templateUrl: './portal.html',
    styleUrl: './portal.scss',
})
export class EnginePortal {
    readonly label = input('Scene viewport');
    readonly enabled = input(true);
    readonly viewportInput = output<ViewportInput>();
    readonly viewportStatus = output<ViewportStatus>();
    readonly viewportError = output<EditorClientError>();
    protected readonly nativeAvailable = inject(EditorService).nativeAvailable;
    protected readonly error = signal<string | null>(null);
    private readonly modalOpen = signal(false);
    protected readonly active = computed(() => this.enabled() && !this.modalOpen());
    private readonly viewport = viewChild<EngineViewportDirective>('viewport');
    private readonly document = inject(DOCUMENT);
    private observer: MutationObserver | undefined;

    constructor() {
        afterNextRender(() => {
            if (!this.nativeAvailable) return;
            // Native child surfaces sit above WebView content. Hide them while a modal is open.
            const update = () => this.modalOpen.set(!!this.document.querySelector('dialog:modal'));
            update();
            this.observer = new MutationObserver((records) => {
                if (
                    records.some(
                        (record) =>
                            (record.type === 'attributes' && record.target instanceof HTMLDialogElement) ||
                            [...record.addedNodes, ...record.removedNodes].some(
                                (node) =>
                                    node instanceof Element &&
                                    (node.matches('dialog') || !!node.querySelector('dialog')),
                            ),
                    )
                )
                    update();
            });
            this.observer.observe(this.document.body, {
                subtree: true,
                childList: true,
                attributes: true,
                attributeFilter: ['open'],
            });
        });
        inject(DestroyRef).onDestroy(() => this.observer?.disconnect());
    }

    /** Notify after a dock/drag operation that moves the viewport without resizing it. */
    refreshBounds(): void {
        this.viewport()?.refreshBounds();
    }

    protected statusChanged(status: ViewportStatus): void {
        this.error.set(status.error);
        this.viewportStatus.emit(status);
    }

    protected failed(error: EditorClientError): void {
        this.error.set(error.message);
        this.viewportError.emit(error);
    }
}
