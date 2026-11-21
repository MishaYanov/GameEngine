import {ErrorHandler, inject, Service, signal} from '@angular/core';
import type {ContextMenuItem} from '../../builder/context-menu-builder';

export interface ContextMenuRequest {
    readonly items: readonly ContextMenuItem[];
    readonly label: string;
    readonly x: number;
    readonly y: number;
    readonly owner: HTMLElement;
    readonly returnFocus: HTMLElement;
}

@Service()
export class ContextMenuService {
    private readonly errorHandler = inject(ErrorHandler);
    private readonly current = signal<ContextMenuRequest | null>(null);
    readonly request = this.current.asReadonly();

    open(request: ContextMenuRequest): boolean {
        if (!request.items.some(item => item.kind !== 'separator')) return false;
        this.current.set(request);
        return true;
    }

    close(restoreFocus = true): void {
        const previous = this.current();
        this.current.set(null);
        if (restoreFocus && previous?.returnFocus.isConnected)
            previous.returnFocus.focus({preventScroll: true});
    }

    closeFor(owner: HTMLElement): void {
        if (this.current()?.owner === owner) this.close(false);
    }

    execute(item: ContextMenuItem): void {
        if (item.kind !== 'action' || item.disabled) return;
        this.close();
        try {
            Promise.resolve(item.execute()).catch(error => this.errorHandler.handleError(error));
        } catch (error: unknown) {
            this.errorHandler.handleError(error);
        }
    }
}
