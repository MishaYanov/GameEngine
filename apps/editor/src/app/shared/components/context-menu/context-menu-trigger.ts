import {DestroyRef, Directive, ElementRef, inject, input} from '@angular/core';
import type {ContextMenuItem} from '../../builder/context-menu-builder';
import {ContextMenuService} from './context-menu.service';

export type ContextMenuSource = readonly ContextMenuItem[] | (() => readonly ContextMenuItem[]);

/** Attach to a component or row; the closest trigger handles a nested right click. */
@Directive({
    selector: '[editorContextMenu]',
    host: {
        '(contextmenu)': 'openPointer($event)',
        '(keydown)': 'openKeyboard($event)',
    },
})
export class EditorContextMenuTrigger {
    readonly editorContextMenu = input.required<ContextMenuSource>();
    readonly editorContextMenuLabel = input('Actions');
    private readonly element = inject<ElementRef<HTMLElement>>(ElementRef).nativeElement;
    private readonly menu = inject(ContextMenuService);

    constructor() {
        inject(DestroyRef).onDestroy(() => this.menu.closeFor(this.element));
    }

    protected openPointer(event: MouseEvent): void {
        if (event.defaultPrevented) return;
        // Keyboard-generated contextmenu events have no pointer coordinates.
        const keyboard = event.button === 0 && event.clientX === 0 && event.clientY === 0;
        this.open(event, keyboard ? null : {x: event.clientX, y: event.clientY});
    }

    protected openKeyboard(event: KeyboardEvent): void {
        if (event.defaultPrevented || event.isComposing || event.altKey || event.ctrlKey || event.metaKey) return;
        if (event.key === 'ContextMenu' || (event.key === 'F10' && event.shiftKey)) this.open(event, null);
    }

    private open(event: Event, pointer: { x: number; y: number } | null): void {
        const source = this.editorContextMenu();
        const items = typeof source === 'function' ? source() : source;
        const document = this.element.ownerDocument;
        const activeId = this.element.getAttribute('aria-activedescendant');
        const anchor = activeId ? document.getElementById(activeId) ?? this.element : this.element;
        const rect = anchor.getBoundingClientRect();
        const returnFocus = this.element.closest<HTMLElement>('[role="tree"]') ??
            (this.element.tabIndex >= 0 ? this.element :
                document.activeElement instanceof HTMLElement ? document.activeElement : this.element);
        if (this.menu.open({
            items, label: this.editorContextMenuLabel(),
            x: pointer?.x ?? rect.left, y: pointer?.y ?? rect.bottom,
            owner: this.element, returnFocus,
        })) {
            event.preventDefault();
            event.stopPropagation();
        }
    }
}
