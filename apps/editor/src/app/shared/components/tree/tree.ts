import {Component, computed, ElementRef, input, model, signal, viewChild} from '@angular/core';
import {EditorIcon, type EditorIconName} from '../../icons/editor-icon';

export interface EditorTreeNode {
    readonly id: string;
    readonly label: string;
    readonly icon?: EditorIconName;
    readonly children?: readonly EditorTreeNode[];
}

interface TreeEntry {
    readonly node: EditorTreeNode;
    readonly parentId: string | null;
    readonly level: number;
    readonly position: number;
    readonly siblingCount: number;
}

interface TreeRow extends TreeEntry {
    readonly expanded: boolean | null;
}

let nextTreeId = 0;

@Component({
    selector: 'editor-tree',
    imports: [EditorIcon],
    templateUrl: './tree.html',
    styleUrl: './tree.scss',
})
export class EditorTree {
    readonly label = input.required<string>();
    readonly nodes = input.required<readonly EditorTreeNode[]>();
    readonly selectedId = model<string | null>(null);
    private readonly tree = viewChild<ElementRef<HTMLElement>>('tree');
    private readonly treeId = `editor-tree-${nextTreeId++}`;
    private readonly collapsed = signal<ReadonlySet<string>>(new Set());
    protected readonly rows = computed(() => {
        const rows: TreeRow[] = [];
        const collapsed = this.collapsed();
        const stack: TreeEntry[] = [];
        const pushSiblings = (nodes: readonly EditorTreeNode[], parentId: string | null, level: number) => {
            for (let index = nodes.length - 1; index >= 0; index--) {
                stack.push({node: nodes[index], parentId, level, position: index + 1, siblingCount: nodes.length});
            }
        };
        pushSiblings(this.nodes(), null, 1);
        while (stack.length) {
            const entry = stack.pop()!;
            const children = entry.node.children ?? [];
            const expanded = children.length ? !collapsed.has(entry.node.id) : null;
            rows.push({...entry, expanded});
            if (expanded) pushSiblings(children, entry.node.id, entry.level + 1);
        }
        return rows;
    });
    private readonly focusedId = signal<string | null>(null);
    private readonly indices = computed(() => new Map(this.rows().map(({node}, index) => [node.id, index])));
    protected readonly activeId = computed(() => {
        const indices = this.indices();
        const focused = this.focusedId();
        const selected = this.selectedId();
        if (focused !== null && indices.has(focused)) return focused;
        if (selected !== null && indices.has(selected)) return selected;
        return this.rows()[0]?.node.id ?? null;
    });
    protected readonly activeDescendant = computed(() => {
        const id = this.activeId();
        return id === null ? null : this.rowId(id);
    });

    protected rowId(id: string): string {
        return `${this.treeId}-${encodeURIComponent(id)}`;
    }

    protected choose(id: string): void {
        this.focus(id);
        this.tree()?.nativeElement.focus({preventScroll: true});
        this.selectedId.set(id);
    }

    protected toggle(event: MouseEvent, row: TreeRow): void {
        if (row.expanded === null) return;
        event.stopPropagation();
        this.focus(row.node.id);
        this.tree()?.nativeElement.focus({preventScroll: true});
        this.setExpanded(row.node.id, !row.expanded);
    }

    protected navigate(event: KeyboardEvent): void {
        if (event.defaultPrevented || event.isComposing || event.altKey || event.ctrlKey || event.metaKey) return;
        const id = this.activeId();
        const index = id === null ? undefined : this.indices().get(id);
        if (index === undefined) return;
        const rows = this.rows();
        const row = rows[index];
        switch (event.key) {
            case 'ArrowDown':
                this.focus(rows[index + 1]?.node.id);
                break;
            case 'ArrowUp':
                this.focus(rows[index - 1]?.node.id);
                break;
            case 'Home':
                this.focus(rows[0].node.id);
                break;
            case 'End':
                this.focus(rows[rows.length - 1].node.id);
                break;
            case 'ArrowRight':
                if (row.expanded === false) this.setExpanded(row.node.id, true);
                else if (row.expanded === true) this.focus(rows[index + 1].node.id);
                break;
            case 'ArrowLeft':
                if (row.expanded) this.setExpanded(row.node.id, false);
                else this.focus(row.parentId ?? undefined);
                break;
            case 'Enter':
            case ' ':
                this.selectedId.set(row.node.id);
                break;
            default:
                return;
        }
        event.preventDefault();
    }

    private setExpanded(id: string, expanded: boolean): void {
        this.collapsed.update(previous => {
            const next = new Set(previous);
            if (expanded) next.delete(id);
            else next.add(id);
            return next;
        });
    }

    private focus(id: string | undefined): void {
        if (id === undefined) return;
        this.focusedId.set(id);
        this.tree()?.nativeElement.ownerDocument.getElementById(this.rowId(id))
            ?.scrollIntoView?.({block: 'nearest', inline: 'nearest'});
    }
}
