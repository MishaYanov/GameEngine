import {signal} from '@angular/core';
import {invoke} from '@tauri-apps/api/core';
import {listen, type UnlistenFn} from '@tauri-apps/api/event';
import {EditorClientError} from '../editor-client-error';
import type {EditorCapabilities, ViewportBounds, ViewportInput, ViewportStatus} from '../editor.models';

interface ViewportAttachmentOptions {
    readonly nativeAvailable: boolean;
    readonly loadCapabilities: () => Promise<EditorCapabilities>;
    readonly assertNative: (operation: string) => void;
    readonly invoke: <T>(operation: string, args?: Record<string, unknown>) => Promise<T>;
    readonly recordError: (operation: string, cause: unknown) => EditorClientError;
}

interface PendingBounds {
    readonly update: { bounds: ViewportBounds };
    readonly promise: Promise<ViewportStatus>;
}

/** Owns the native surface, independently of document commands and workspace viewport tools. */
export class ViewportAttachment {
    private readonly statusState = signal<ViewportStatus | null>(null);
    readonly status = this.statusState.asReadonly();
    private readonly listeners = new Set<UnlistenFn>();
    private queue: Promise<void> = Promise.resolve();
    private statusListener: UnlistenFn | null = null;
    private pendingBounds: PendingBounds | null = null;
    private pendingDetach: Promise<ViewportStatus> | null = null;
    private pendingStatus: Promise<ViewportStatus> | null = null;
    private pending = 0;
    private reserved = false;
    private attachmentRequested = false;
    private destroyed = false;

    constructor(private readonly options: ViewportAttachmentOptions) {
    }

    destroy(): void {
        this.destroyed = true;
        for (const unlisten of this.listeners) unlisten();
        this.listeners.clear();
        if (this.options.nativeAvailable && this.attachmentRequested) {
            // Resource cleanup can still run after guarded editor calls are closed.
            void this.queue.then(() => invoke('editor_viewport_detach')).catch(() => undefined);
        }
    }

    /** One Angular host can own the single embedded viewport at a time. */
    reserve(): () => void {
        if (this.reserved) {
            throw this.options.recordError(
                'editor_viewport_attach',
                new EditorClientError(
                    'busy',
                    'editor_viewport_attach',
                    'Another Angular element already owns the embedded viewport.',
                ),
            );
        }
        this.reserved = true;
        let released = false;
        return () => {
            if (!released) {
                released = true;
                this.reserved = false;
            }
        };
    }

    attach(bounds: ViewportBounds): Promise<ViewportStatus> {
        if (this.pending >= 32) return this.queueFull('editor_viewport_attach');
        // Lifecycle boundaries retain their order: bounds submitted after a new
        // attach must never replace bounds belonging to the preceding owner.
        this.pendingBounds = null;
        this.pendingDetach = null;
        this.pendingStatus = null;
        return this.enqueue('editor_viewport_attach', async () => {
            const capabilities = await this.options.loadCapabilities();
            if (!capabilities.embedded_viewport_supported) {
                throw new EditorClientError(
                    'unavailable',
                    'editor_viewport_attach',
                    'This platform does not support the embedded Vulkan viewport.',
                );
            }
            if (!this.statusListener) {
                this.statusListener = await this.listenNative<ViewportStatus>(
                    'editor://viewport-status',
                    (status) => this.statusState.set(status),
                );
            }
            this.attachmentRequested = true;
            const result = await this.options.invoke<ViewportStatus>('editor_viewport_attach', {bounds});
            this.statusState.set(result);
            return result;
        });
    }

    updateBounds(bounds: ViewportBounds): Promise<ViewportStatus> {
        if (this.pendingBounds) {
            this.pendingBounds.update.bounds = bounds;
            return this.pendingBounds.promise;
        }
        if (this.pending >= 32) return this.queueFull('editor_viewport_bounds');
        this.pendingStatus = null;
        const update = {bounds};
        const promise = this.enqueue('editor_viewport_bounds', async () => {
            if (this.pendingBounds?.update === update) this.pendingBounds = null;
            const result = await this.options.invoke<ViewportStatus>('editor_viewport_bounds', {
                bounds: update.bounds,
            });
            this.statusState.set(result);
            return result;
        });
        this.pendingBounds = {update, promise};
        return promise;
    }

    detach(): Promise<ViewportStatus> {
        if (this.pendingDetach) return this.pendingDetach;
        this.pendingBounds = null;
        this.pendingStatus = null;
        // One coalesced detach may exceed the regular queue cap so releasing an
        // Angular owner can always finish. New attaches still obey the cap.
        const promise = this.enqueue('editor_viewport_detach', async () => {
            const result = await this.options.invoke<ViewportStatus>('editor_viewport_detach');
            this.statusListener?.();
            this.statusListener = null;
            this.attachmentRequested = false;
            this.statusState.set(result);
            return result;
        });
        this.pendingDetach = promise;
        void promise
            .finally(() => {
                if (this.pendingDetach === promise) this.pendingDetach = null;
            })
            .catch(() => undefined);
        return promise;
    }

    refreshStatus(): Promise<ViewportStatus> {
        if (this.pendingStatus) return this.pendingStatus;
        if (this.pending >= 32) return this.queueFull('editor_viewport_status');
        const promise = this.enqueue('editor_viewport_status', async () => {
            if (this.pendingStatus === promise) this.pendingStatus = null;
            const result = await this.options.invoke<ViewportStatus>('editor_viewport_status');
            this.statusState.set(result);
            return result;
        });
        this.pendingStatus = promise;
        return promise;
    }

    /** Subscribe explicitly; raw movement never sends document commands by itself. */
    listenInput(handler: (input: ViewportInput) => void): Promise<UnlistenFn> {
        return this.listenNative<ViewportInput>('editor://viewport-input', handler);
    }

    private enqueue<T>(operation: string, action: () => Promise<T>): Promise<T> {
        this.pending += 1;
        const result = this.queue.then(async () => {
            try {
                this.options.assertNative(operation);
                return await action();
            } catch (cause: unknown) {
                throw this.options.recordError(operation, cause);
            }
        });
        this.queue = result.then(
            () => undefined,
            () => undefined,
        );
        return result.finally(() => {
            this.pending -= 1;
        });
    }

    private queueFull(operation: string): Promise<never> {
        return Promise.reject(
            this.options.recordError(
                operation,
                new EditorClientError(
                    'busy',
                    operation,
                    'Viewport lifecycle queue is full; coalesce repeated attachment requests.',
                ),
            ),
        );
    }

    private async listenNative<T>(event: string, handler: (payload: T) => void): Promise<UnlistenFn> {
        if (!this.options.nativeAvailable || this.destroyed) return () => undefined;
        let active = true;
        const nativeUnlisten = await listen<T>(event, (message) => {
            if (active && !this.destroyed) handler(message.payload);
        });
        const unlisten = () => {
            if (active) {
                active = false;
                this.listeners.delete(unlisten);
                try {
                    // Tauri currently returns an async cleanup despite UnlistenFn's void
                    // signature. Window shutdown may reject that IPC; local callbacks
                    // must still remain disabled and teardown must continue.
                    void Promise.resolve(nativeUnlisten()).catch(() => undefined);
                } catch {
                    /* The owning WebView may already have closed. */
                }
            }
        };
        if (this.destroyed) unlisten();
        else this.listeners.add(unlisten);
        return unlisten;
    }
}
