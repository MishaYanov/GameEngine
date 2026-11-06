import {afterRenderEffect, Component, computed, DestroyRef, effect, ElementRef, inject, input, output, signal, untracked, viewChild} from '@angular/core';
import type {EntityKey, SceneEntity} from '../../../../core/editor/editor.models';
import {EditorPane} from '../../../../shared/components/pane/pane';
import {Drawer} from '../../../../shared/components/drawer/drawer';
import {EditorIcon} from '../../../../shared/icons/editor-icon';
import type {Config} from '../../../../shared/models/config.model';
import type {DrawerSettings} from '../../../../shared/components/drawer/drawer-settings';
import type {SceneEntitiesModel} from './scene-entities.model';

export interface HierarchyReparentRequest { readonly key: EntityKey; readonly parent: EntityKey | null }
interface HierarchyRow {
    readonly key: EntityKey;
    readonly entity: SceneEntity;
    readonly parent: EntityKey | null;
    readonly level: number;
    readonly position: number;
    readonly siblings: number;
    readonly children: boolean;
    readonly expanded: boolean;
    readonly index: number;
    readonly indent: number;
}
const ENTITY_DRAG_TYPE = 'application/x-game-engine-hierarchy-entity';
let nextHierarchyId = 0;

@Component({
    selector: 'scene-entities-panel',
    imports: [EditorPane, Drawer, EditorIcon],
    templateUrl: './scene-entities-panel.html',
    styleUrl: './scene-entities-panel.scss',
})
export class SceneEntitiesPanel {
    readonly scene = input.required<SceneEntitiesModel>();
    readonly selectedKeys = input<readonly EntityKey[]>([]);
    readonly disabled = input(false);
    readonly sceneLabel = input('Scene');
    readonly contextKey = input('');
    readonly actionError = input<string | null>(null);
    readonly selectionRequested = output<EntityKey>();
    readonly reparentRequested = output<HierarchyReparentRequest>();

    protected readonly rowHeight = 28;
    protected readonly treeId = `editor-hierarchy-${nextHierarchyId++}`;
    protected readonly selection = computed(() => new Set(this.selectedKeys()));
    protected readonly collapsed = signal<ReadonlySet<EntityKey>>(new Set());
    protected readonly focusKey = signal<EntityKey | null>(null);
    protected readonly parentingChild = signal<EntityKey | null>(null);
    protected readonly dragKey = signal<EntityKey | null>(null);
    protected readonly dropTarget = signal<{parent: EntityKey | null; valid: boolean} | null>(null);
    private readonly localError = signal<string | null>(null);
    protected readonly error = computed(() => this.localError() ?? this.actionError());
    protected readonly status = signal<string | null>(null);
    private readonly scrollTop = signal(0);
    private readonly viewportHeight = signal(280);
    private readonly tree = viewChild<ElementRef<HTMLElement>>('tree');
    private destroyed = false;
    private previousContext: string | null = null;
    private previousSelectionIdentity = '';
    private typeahead = '';
    private typeaheadTime = 0;

    private readonly forest = computed(() => {
        const entities = this.scene().entities;
        const byKey = new Map(entities.map(entity => [entity.key, entity]));
        const children = new Map<EntityKey | null, SceneEntity[]>();
        const parents = new Map<EntityKey, EntityKey | null>();
        for (const entity of entities) {
            const parent = entity.parent && byKey.has(entity.parent) && entity.parent !== entity.key ? entity.parent : null;
            parents.set(entity.key, parent);
            const siblings = children.get(parent);
            if (siblings) siblings.push(entity); else children.set(parent, [entity]);
        }
        return {byKey, children, parents};
    });
    protected readonly rows = computed<readonly HierarchyRow[]>(() => {
        const {children, parents} = this.forest();
        const collapsed = this.collapsed();
        const roots = children.get(null) ?? [];
        const stack = roots.map((entity, index) => ({entity, level: 1, position: index + 1, siblings: roots.length})).reverse();
        const rows: HierarchyRow[] = [];
        const visited = new Set<EntityKey>();
        // An iterative traversal also handles deeply nested valid scenes.
        while (stack.length) {
            const entry = stack.pop()!;
            const key = entry.entity.key;
            if (visited.has(key)) continue;
            visited.add(key);
            const descendants = children.get(key) ?? [];
            const expanded = descendants.length > 0 && !collapsed.has(key);
            rows.push({...entry, key, parent: parents.get(key) ?? null, children: descendants.length > 0,
                expanded, index: rows.length, indent: 6 + Math.min(entry.level - 1, 8) * 12});
            if (expanded) for (let index = descendants.length - 1; index >= 0; index--) {
                stack.push({entity: descendants[index], level: entry.level + 1, position: index + 1, siblings: descendants.length});
            }
        }
        return rows;
    });
    private readonly rowIndices = computed(() => new Map(this.rows().map(row => [row.key, row.index])));
    protected readonly activeKey = computed(() => {
        const indices = this.rowIndices();
        const focused = this.focusKey();
        if (focused && indices.has(focused)) return focused;
        return this.selectedKeys().find(key => indices.has(key)) ?? this.rows()[0]?.key ?? null;
    });
    protected readonly activeId = computed(() => this.activeKey() ? this.rowId(this.activeKey()!) : null);
    protected readonly primary = computed(() => this.forest().byKey.get(this.selectedKeys()[0]) ?? null);
    protected readonly totalHeight = computed(() => this.rows().length * this.rowHeight);
    protected readonly renderedRows = computed(() => {
        const rows = this.rows();
        const start = Math.max(0, Math.floor(this.scrollTop() / this.rowHeight) - 6);
        const end = Math.min(rows.length, Math.ceil((this.scrollTop() + this.viewportHeight()) / this.rowHeight) + 6);
        const window = rows.slice(start, end);
        const active = this.activeKey();
        const activeIndex = active ? this.rowIndices().get(active) : undefined;
        // Preserve the active-descendant reference even when manually scrolling.
        if (activeIndex !== undefined && (activeIndex < start || activeIndex >= end)) window.push(rows[activeIndex]);
        return window.sort((a, b) => a.index - b.index);
    });
    private readonly forbiddenParents = computed(() => {
        const source = this.dragKey() ?? this.parentingChild();
        const forbidden = new Set<EntityKey>();
        if (!source) return forbidden;
        const stack = [source];
        while (stack.length) {
            const key = stack.pop()!;
            if (forbidden.has(key)) continue;
            forbidden.add(key);
            for (const child of this.forest().children.get(key) ?? []) stack.push(child.key);
        }
        return forbidden;
    });

    protected readonly drawerConfig: Config<DrawerSettings> = {width: '14rem', minWidth: '8rem'};

    constructor() {
        inject(DestroyRef).onDestroy(() => this.destroyed = true);
        effect(() => {
            const context = this.contextKey();
            const forest = this.forest();
            const selected = this.selectedKeys()[0] ?? null;
            const disabled = this.disabled();
            untracked(() => {
                if (this.previousContext !== context) {
                    this.previousContext = context; this.previousSelectionIdentity = '';
                    this.collapsed.set(new Set()); this.focusKey.set(null); this.scrollTop.set(0);
                    if (this.tree()) this.tree()!.nativeElement.scrollTop = 0;
                    this.cancelParenting();
                }
                const ancestors: EntityKey[] = [];
                const seen = new Set<EntityKey>();
                let parent = selected ? forest.parents.get(selected) : null;
                while (parent && !seen.has(parent)) {seen.add(parent); ancestors.push(parent); parent = forest.parents.get(parent);}
                const identity = JSON.stringify([context, selected, ancestors]);
                if (selected && forest.byKey.has(selected) && identity !== this.previousSelectionIdentity) {
                    const next = new Set(this.collapsed());
                    for (const ancestor of ancestors) next.delete(ancestor);
                    this.collapsed.set(next); this.focusKey.set(selected);
                    queueMicrotask(() => {if (!this.destroyed) this.reveal(selected);});
                }
                this.previousSelectionIdentity = identity;
                if (this.focusKey() && !forest.byKey.has(this.focusKey()!)) this.focusKey.set(null);
                if (disabled) {this.dragKey.set(null); this.dropTarget.set(null); this.parentingChild.set(null); this.status.set(null);}
            });
        });
        afterRenderEffect(onCleanup => {
            const element = this.tree()?.nativeElement;
            if (!element) return;
            const updateHeight = (height: number) => this.viewportHeight.set(Math.max(this.rowHeight, height || 280));
            updateHeight(element.clientHeight);
            const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(entries => updateHeight(entries[0]?.contentRect.height ?? element.clientHeight));
            observer?.observe(element);
            untracked(() => {const key = this.activeKey(); if (key) this.reveal(key);});
            onCleanup(() => observer?.disconnect());
        });
    }

    protected rowId(key: EntityKey): string {return `${this.treeId}-${encodeURIComponent(key)}`;}
    protected scrolled(event: Event): void {this.scrollTop.set((event.currentTarget as HTMLElement).scrollTop);}
    protected choose(row: HierarchyRow): void {
        if (this.disabled()) return;
        this.focus(row.key);
        const child = this.parentingChild();
        if (child) this.requestReparent(child, row.key); else this.selectionRequested.emit(row.key);
    }
    protected toggle(event: Event, row: HierarchyRow): void {
        event.preventDefault(); event.stopPropagation();
        if (this.disabled() || !row.children) return;
        this.setExpanded(row, !row.expanded); this.focus(row.key);
    }
    private setExpanded(row: HierarchyRow, expanded: boolean): void {
        const next = new Set(this.collapsed());
        if (expanded) next.delete(row.key); else next.add(row.key);
        this.collapsed.set(next);
    }
    private focus(key: EntityKey, select = false): void {
        this.focusKey.set(key); this.reveal(key);
        this.tree()?.nativeElement.focus({preventScroll: true});
        if (select && !this.parentingChild()) this.selectionRequested.emit(key);
    }
    private reveal(key: EntityKey): void {
        const index = this.rowIndices().get(key);
        if (index === undefined) return;
        const top = index * this.rowHeight;
        const current = this.scrollTop();
        const next = top < current ? top : top + this.rowHeight > current + this.viewportHeight()
            ? top + this.rowHeight - this.viewportHeight() : current;
        if (next !== current) {this.scrollTop.set(Math.max(0, next)); if (this.tree()) this.tree()!.nativeElement.scrollTop = Math.max(0, next);}
    }

    protected keydown(event: KeyboardEvent): void {
        if (this.disabled() || event.isComposing) return;
        const rows = this.rows();
        const index = this.activeKey() ? this.rowIndices().get(this.activeKey()!) ?? 0 : 0;
        const row = rows[index];
        if (!row) return;
        const move = (next: number) => this.focus(rows[Math.max(0, Math.min(rows.length - 1, next))].key, true);
        switch (event.key) {
            case 'ArrowDown': event.preventDefault(); move(index + 1); break;
            case 'ArrowUp': event.preventDefault(); move(index - 1); break;
            case 'Home': event.preventDefault(); move(0); break;
            case 'End': event.preventDefault(); move(rows.length - 1); break;
            case 'ArrowRight':
                event.preventDefault();
                if (row.children && !row.expanded) this.setExpanded(row, true);
                else if (row.children && rows[index + 1]?.parent === row.key) move(index + 1);
                break;
            case 'ArrowLeft':
                event.preventDefault();
                if (row.children && row.expanded) this.setExpanded(row, false);
                else if (row.parent) this.focus(row.parent, true);
                break;
            case 'Enter': case ' ': event.preventDefault(); this.choose(row); break;
            case 'Escape': event.preventDefault(); this.cancelParenting(); this.status.set('Parenting cancelled.'); break;
            default:
                if (event.key.length !== 1 || event.ctrlKey || event.metaKey || event.altKey || !event.key.trim()) return;
                event.preventDefault();
                this.typeahead = event.timeStamp - this.typeaheadTime > 700 ? event.key : this.typeahead + event.key;
                this.typeaheadTime = event.timeStamp;
                const query = /^([\s\S])\1+$/.test(this.typeahead) ? event.key : this.typeahead;
                for (let offset = 1; offset <= rows.length; offset++) {
                    const candidate = rows[(index + offset) % rows.length];
                    if (candidate.entity.name.toLocaleLowerCase().startsWith(query.toLocaleLowerCase())) {this.focus(candidate.key, true); break;}
                }
        }
    }

    protected beginParenting(): void {
        const child = this.primary();
        if (!child || this.disabled()) return;
        this.parentingChild.set(child.key); this.dragKey.set(null); this.localError.set(null);
        this.status.set(`Choose a parent for ${child.name}, then press Enter. Escape cancels.`); this.focus(child.key);
    }
    protected moveToRoot(): void {const key = this.parentingChild() ?? this.primary()?.key; if (key) this.requestReparent(key, null);}
    protected cancelParenting(): void {this.parentingChild.set(null); this.dragKey.set(null); this.dropTarget.set(null); this.localError.set(null); this.status.set(null);}
    protected dragStarted(event: DragEvent, key: EntityKey): void {
        if (this.disabled() || !event.dataTransfer || !this.forest().byKey.has(key)) {event.preventDefault(); return;}
        this.parentingChild.set(null); this.localError.set(null); this.dragKey.set(key);
        event.dataTransfer.effectAllowed = 'move'; event.dataTransfer.setData(ENTITY_DRAG_TYPE, key);
        this.status.set('Drop onto an entity to parent it, or onto Scene root to remove its parent.');
    }
    protected dragEnded(): void {this.dragKey.set(null); this.dropTarget.set(null);}
    protected dragOver(event: DragEvent, parent: EntityKey | null): void {
        const key = this.dragKey(); if (!key || this.disabled()) return;
        const valid = this.canReparent(key, parent);
        if (valid) {event.preventDefault(); if (event.dataTransfer) event.dataTransfer.dropEffect = 'move';}
        const previous = this.dropTarget();
        if (previous?.parent !== parent || previous?.valid !== valid) this.dropTarget.set({parent, valid});
    }
    protected dragLeft(event: DragEvent): void {
        if (event.currentTarget instanceof Element && event.relatedTarget instanceof Node && event.currentTarget.contains(event.relatedTarget)) return;
        this.dropTarget.set(null);
    }
    protected dropped(event: DragEvent, parent: EntityKey | null): void {
        const key = this.dragKey(); if (!key) return;
        event.preventDefault(); event.stopPropagation();
        const sameSession = event.dataTransfer?.getData(ENTITY_DRAG_TYPE) === key;
        this.dropTarget.set(null);
        if (sameSession) this.requestReparent(key, parent);
        this.dragKey.set(null);
    }
    protected isDropTarget(parent: EntityKey | null): boolean {return this.dropTarget()?.parent === parent;}
    private canReparent(key: EntityKey, parent: EntityKey | null): boolean {
        const forest = this.forest();
        if (this.disabled() || !forest.byKey.has(key) || forest.parents.get(key) === parent) return false;
        if (parent === null) return true;
        if (!forest.byKey.has(parent)) return false;
        if ((this.dragKey() === key || this.parentingChild() === key) && this.forbiddenParents().has(parent)) return false;
        const seen = new Set<EntityKey>();
        let ancestor: EntityKey | null | undefined = parent;
        while (ancestor && !seen.has(ancestor)) {if (ancestor === key) return false; seen.add(ancestor); ancestor = forest.parents.get(ancestor);}
        return !ancestor;
    }
    private requestReparent(key: EntityKey, parent: EntityKey | null): void {
        if (this.disabled()) return;
        if (!this.canReparent(key, parent)) {
            this.localError.set(this.forest().parents.get(key) === parent ? 'This entity already has that parent.' : 'An entity cannot be parented beneath itself or its descendants.');
            return;
        }
        this.localError.set(null); this.parentingChild.set(null); this.reparentRequested.emit({key, parent});
        this.status.set(parent ? 'Parent change requested. The world pose will be preserved.' : 'Move to Scene root requested. The world pose will be preserved.');
    }
}
