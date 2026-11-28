import {
    afterRenderEffect, Component, computed, DestroyRef, DOCUMENT, ElementRef, inject, linkedSignal, viewChild,
} from '@angular/core';
import type {ContextMenuItem} from '../../builder/context-menu-builder';
import {EditorIcon} from '../../icons/editor-icon';
import {ContextMenuService} from './context-menu.service';

interface MenuLevel {
    readonly items: readonly ContextMenuItem[];
    readonly label: string;
}

let nextMenuId = 0;

@Component({
    selector: 'editor-context-menu',
    imports: [EditorIcon],
    templateUrl: './context-menu.html',
    styleUrl: './context-menu.scss',
})
export class EditorContextMenu {
    protected readonly menu = inject(ContextMenuService);
    private readonly document = inject(DOCUMENT);
    private readonly surface = viewChild<ElementRef<HTMLElement>>('surface');
    private readonly id = `editor-context-menu-${nextMenuId++}`;
    private readonly path = linkedSignal({source: this.menu.request, computation: (): readonly number[] => []});
    protected readonly active = linkedSignal({source: this.menu.request, computation: (): readonly number[] => [0]});
    protected readonly levels = computed(() => {
        const request = this.menu.request();
        if (!request) return [];
        const levels: MenuLevel[] = [{items: request.items, label: request.label}];
        for (const index of this.path()) {
            const parent = levels.at(-1)!.items[index];
            if (parent?.kind !== 'submenu') break;
            levels.push({items: parent.children, label: parent.label});
        }
        return levels;
    });
    private query = '';
    private queryTime = 0;

    constructor() {
        inject(DestroyRef).onDestroy(() => this.menu.close(false));
        afterRenderEffect(onCleanup => {
            const request = this.menu.request();
            const surface = this.surface()?.nativeElement;
            if (!request || !surface) return;
            this.query = '';
            this.queryTime = 0;
            // A manual popover keeps the menu above clipped editor panes and drawers.
            surface.showPopover?.();
            const outside = (event: Event) => {
                if (event.target instanceof Node && !surface.contains(event.target)) this.menu.close(false);
            };
            const scroll = (event: Event) => {
                const scroller = event.target;
                if (scroller instanceof Node && !surface.contains(scroller) && scroller.contains(request.owner)) {
                    this.menu.close();
                }
            };
            const dismiss = () => this.menu.close(false);
            this.document.addEventListener('pointerdown', outside, true);
            this.document.addEventListener('contextmenu', outside, true);
            this.document.addEventListener('focusin', outside);
            this.document.addEventListener('scroll', scroll, true);
            this.document.defaultView?.addEventListener('resize', dismiss);
            this.document.defaultView?.addEventListener('blur', dismiss);
            onCleanup(() => {
                this.document.removeEventListener('pointerdown', outside, true);
                this.document.removeEventListener('contextmenu', outside, true);
                this.document.removeEventListener('focusin', outside);
                this.document.removeEventListener('scroll', scroll, true);
                this.document.defaultView?.removeEventListener('resize', dismiss);
                this.document.defaultView?.removeEventListener('blur', dismiss);
                surface.hidePopover?.();
            });
        });
        afterRenderEffect(() => {
            const request = this.menu.request();
            const surface = this.surface()?.nativeElement;
            const path = this.path();
            const active = this.active();
            if (!request || !surface) return;
            const panels = [...surface.querySelectorAll<HTMLElement>('[role="menu"]')];
            const width = this.document.documentElement.clientWidth || this.document.defaultView?.innerWidth || 0;
            const height = this.document.documentElement.clientHeight || this.document.defaultView?.innerHeight || 0;
            panels.forEach((panel, level) => {
                const bounds = panel.getBoundingClientRect();
                const parent = panels[level - 1];
                const parentBounds = parent?.getBoundingClientRect();
                const opener = parent?.querySelector<HTMLElement>(`[data-index="${path[level - 1]}"]`);
                let x = parentBounds?.right ?? request.x;
                if (parentBounds && x + bounds.width > width - 4) x = parentBounds.left - bounds.width;
                const y = opener?.getBoundingClientRect().top ?? request.y;
                panel.style.left = `${Math.max(4, Math.min(x, width - bounds.width - 4))}px`;
                panel.style.top = `${Math.max(4, Math.min(y, height - bounds.height - 4))}px`;
            });
            const last = panels.at(-1);
            const index = active[panels.length - 1] ?? 0;
            const focused = last?.querySelector<HTMLElement>(`[data-index="${index}"]`);
            if (last && focused) {
                focused.focus({preventScroll: true});
                const viewport = last.getBoundingClientRect();
                const row = focused.getBoundingClientRect();
                if (row.top < viewport.top) last.scrollTop += row.top - viewport.top;
                else if (row.bottom > viewport.bottom) last.scrollTop += row.bottom - viewport.bottom;
            }
        });
    }

    protected itemId(level: number, index: number): string {
        return `${this.id}-${level}-${index}`;
    }

    protected expanded(level: number, index: number): boolean {
        return this.path()[level] === index;
    }

    protected hover(level: number, index: number): void {
        const item = this.levels()[level].items[index];
        if (item.kind === 'submenu') {
            this.openSubmenu(level, index);
        } else {
            this.active.set([...this.active().slice(0, level), index]);
            this.path.set(this.path().slice(0, level));
        }
    }

    protected activate(level: number, index: number): void {
        const item = this.levels()[level].items[index];
        if (item.kind === 'submenu') this.openSubmenu(level, index);
        else this.menu.execute(item);
    }

    protected navigate(event: KeyboardEvent, level: number): void {
        event.stopPropagation();
        if (event.key === 'Tab') {
            this.menu.close();
            return;
        }
        if (event.key === 'Escape') {
            event.preventDefault();
            this.menu.close();
            return;
        }
        const entries = this.levels()[level].items;
        const indices = entries.flatMap((item, index) => item.kind === 'separator' ? [] : [index]);
        const index = this.active()[level] ?? indices[0];
        const position = indices.indexOf(index);
        let next: number | undefined;
        switch (event.key) {
            case 'ArrowDown':
                next = indices[(position + 1) % indices.length];
                break;
            case 'ArrowUp':
                next = indices[(position + indices.length - 1) % indices.length];
                break;
            case 'Home':
                next = indices[0];
                break;
            case 'End':
                next = indices.at(-1);
                break;
            case 'ArrowRight':
                if (entries[index]?.kind === 'submenu') this.openSubmenu(level, index);
                break;
            case 'ArrowLeft':
                if (level > 0) {
                    this.path.set(this.path().slice(0, level - 1));
                    this.active.set(this.active().slice(0, level));
                }
                break;
            case 'Enter':
            case ' ':
                this.activate(level, index);
                break;
            default: {
                if (event.key.length !== 1 || event.ctrlKey || event.metaKey || event.altKey || event.isComposing) return;
                this.query = event.timeStamp - this.queryTime > 700 ? event.key : this.query + event.key;
                this.queryTime = event.timeStamp;
                const query = /^([\s\S])\1+$/.test(this.query) ? event.key : this.query;
                for (let offset = 1; offset <= indices.length; offset++) {
                    const candidate = indices[(position + offset) % indices.length];
                    const item = entries[candidate];
                    if (item.kind !== 'separator' && item.label.toLocaleLowerCase().startsWith(query.toLocaleLowerCase())) {
                        next = candidate;
                        break;
                    }
                }
            }
        }
        event.preventDefault();
        if (next !== undefined) {
            this.active.set([...this.active().slice(0, level), next]);
            this.path.set(this.path().slice(0, level));
        }
    }

    private openSubmenu(level: number, index: number): void {
        this.query = '';
        this.queryTime = 0;
        this.path.set([...this.path().slice(0, level), index]);
        this.active.set([...this.active().slice(0, level), index, 0]);
    }
}
