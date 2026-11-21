import {
    afterNextRender,
    afterRenderEffect,
    Component,
    computed,
    DestroyRef,
    effect,
    ElementRef,
    inject,
    Injector,
    input,
    output,
    signal,
    untracked,
    viewChild
} from '@angular/core';
import type {EntityKey} from '../../../../core/editor/editor.models';
import {EditorPane} from '../../../../shared/components/pane/pane';
import {Drawer} from '../../../../shared/components/drawer/drawer';
import {EditorIcon} from '../../../../shared/icons/editor-icon';
import type {Config} from '../../../../shared/models/config.model';
import type {DrawerSettings} from '../../../../shared/components/drawer/drawer-settings';
import type {SceneEntitiesModel} from './scene-entities.model';
import {
    buildHierarchy,
    canReparentEntity,
    hierarchyAncestors,
    type HierarchyRow,
    hierarchySubtreeKeys,
    visibleHierarchyRows
} from './hierarchy-tree';
import {hierarchyScrollTop, renderHierarchyWindow} from './hierarchy-viewport';
import {ContextActionBuilder} from '../../../../shared/actions/context-action';
import {ContextMenuBuilder, type ContextMenuItem} from '../../../../shared/builder/context-menu-builder';
import {EditorContextMenuTrigger} from '../../../../shared/components/context-menu/context-menu-trigger';

export interface HierarchyReparentRequest {
    readonly key: EntityKey;
    readonly parent: EntityKey | null
}

interface EntityMenuContext {
    readonly key: EntityKey;
    readonly contextKey: string;
}

const ENTITY_DRAG_TYPE = 'application/x-game-engine-hierarchy-entity';
let nextHierarchyId = 0;

@Component({
    selector: 'scene-entities-panel',
    imports: [EditorPane, Drawer, EditorIcon, EditorContextMenuTrigger],
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
    readonly usagesRequested = output<EntityKey>();
    readonly reparentRequested = output<HierarchyReparentRequest>();

    protected readonly rowHeight = 28;
    protected readonly treeId = `editor-hierarchy-${nextHierarchyId++}`;
    protected readonly selection = computed(() => new Set(this.selectedKeys()));
    protected readonly collapsed = signal<ReadonlySet<EntityKey>>(new Set());
    protected readonly focusKey = signal<EntityKey | null>(null);
    protected readonly parentingChild = signal<EntityKey | null>(null);
    protected readonly dragKey = signal<EntityKey | null>(null);
    protected readonly dropTarget = signal<{ parent: EntityKey | null; valid: boolean } | null>(null);
    private readonly localError = signal<string | null>(null);
    protected readonly error = computed(() => this.localError() ?? this.actionError());
    protected readonly status = signal<string | null>(null);
    private readonly scrollTop = signal(0);
    private readonly viewportHeight = signal(280);
    private readonly drawer = viewChild(Drawer);
    private readonly tree = viewChild<ElementRef<HTMLElement>>('tree');
    private readonly injector = inject(Injector);
    private destroyed = false;
    private previousContext: string | null = null;
    private previousSelectionIdentity = '';
    private typeahead = '';
    private typeaheadTime = 0;

    protected readonly renderedRows = computed(() => {
        const active = this.activeKey();
        const activeIndex = active ? this.rowIndices().get(active) : undefined;
        return renderHierarchyWindow(this.rows(), this.rowHeight, this.scrollTop(), this.viewportHeight(), activeIndex);
    });
    private readonly forest = computed(() => buildHierarchy(this.scene().entities));
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
    protected readonly rows = computed(() => visibleHierarchyRows(this.forest(), this.collapsed()));
    private readonly forbiddenParents = computed(() => {
        const source = this.dragKey() ?? this.parentingChild();
        return source ? hierarchySubtreeKeys(this.forest(), source) : new Set<EntityKey>();
    });

    protected readonly drawerConfig: Config<DrawerSettings> = {width: '14rem', minWidth: '8rem'};
    private readonly entityMenuBuilder = ContextMenuBuilder.create<EntityMenuContext>()
        .action(ContextActionBuilder.create<EntityMenuContext>('entity.select', 'Select')
            .icon('object')
            .enabledWhen(context => this.canUseEntityMenu(context))
            .run(context => {
                this.cancelParenting();
                this.focus(context.key);
                this.selectionRequested.emit(context.key);
            }).build())
        .action(ContextActionBuilder.create<EntityMenuContext>('entity.find-usages', 'Find Usages…')
            .icon('graph')
            .enabledWhen(context => this.canUseEntityMenu(context))
            .run(context => this.usagesRequested.emit(context.key)).build())
        .separator()
        .submenu('entity.parenting', 'Parenting', menu => menu
            .action(ContextActionBuilder.create<EntityMenuContext>('entity.set-parent', 'Set Parent…')
                .enabledWhen(context => this.canUseEntityMenu(context))
                .run(context => this.beginParenting(context.key)).build())
            .action(ContextActionBuilder.create<EntityMenuContext>('entity.move-to-root', 'Move to Root')
                .enabledWhen(context => this.canUseEntityMenu(context) && this.canReparent(context.key, null))
                .run(context => this.requestReparent(context.key, null)).build()));
    protected readonly activeEntityMenu = (): readonly ContextMenuItem[] => {
        const key = this.activeKey();
        return key ? this.entityMenu(key)() : [];
    };

    protected entityMenu(key: EntityKey): () => readonly ContextMenuItem[] {
        const context: EntityMenuContext = {key, contextKey: this.contextKey()};
        return () => this.entityMenuBuilder.build(context);
    }

    private canUseEntityMenu(context: EntityMenuContext): boolean {
        return context.contextKey === this.contextKey() && this.scene().projectOpen &&
            !this.disabled() && this.forest().byKey.has(context.key);
    }

    constructor() {
        inject(DestroyRef).onDestroy(() => this.destroyed = true);
        effect(() => {
            const context = this.contextKey();
            const forest = this.forest();
            const selected = this.selectedKeys()[0] ?? null;
            const disabled = this.disabled();
            untracked(() => {
                if (this.previousContext !== context) {
                    this.previousContext = context;
                    this.previousSelectionIdentity = '';
                    this.collapsed.set(new Set());
                    this.focusKey.set(null);
                    this.scrollTop.set(0);
                    if (this.tree()) this.tree()!.nativeElement.scrollTop = 0;
                    this.cancelParenting();
                }
                const ancestors = selected ? hierarchyAncestors(forest, selected) : [];
                const identity = JSON.stringify([context, selected, ancestors]);
                if (selected && forest.byKey.has(selected) && identity !== this.previousSelectionIdentity) {
                    const next = new Set(this.collapsed());
                    for (const ancestor of ancestors) next.delete(ancestor);
                    this.collapsed.set(next);
                    this.focusKey.set(selected);
                    queueMicrotask(() => {
                        if (!this.destroyed) this.reveal(selected);
                    });
                }
                this.previousSelectionIdentity = identity;
                if (this.focusKey() && !forest.byKey.has(this.focusKey()!)) this.focusKey.set(null);
                if (disabled) {
                    this.dragKey.set(null);
                    this.dropTarget.set(null);
                    this.parentingChild.set(null);
                    this.status.set(null);
                }
            });
        });
        afterRenderEffect(onCleanup => {
            const element = this.tree()?.nativeElement;
            if (!element) return;
            const updateHeight = (height: number) => this.viewportHeight.set(Math.max(this.rowHeight, height || 280));
            updateHeight(element.clientHeight);
            const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(entries => updateHeight(entries[0]?.contentRect.height ?? element.clientHeight));
            observer?.observe(element);
            untracked(() => {
                const key = this.activeKey();
                if (key) this.reveal(key);
            });
            onCleanup(() => observer?.disconnect());
        });
    }

    protected rowId(key: EntityKey): string {
        return `${this.treeId}-${encodeURIComponent(key)}`;
    }

    revealEntity(key: EntityKey): void {
        const forest = this.forest();
        if (!forest.byKey.has(key)) return;
        const context = this.contextKey();
        this.drawer()?.config.update(config => ({...config, collapsed: false, expanded: true}));
        const collapsed = new Set(this.collapsed());
        for (const ancestor of hierarchyAncestors(forest, key)) collapsed.delete(ancestor);
        this.collapsed.set(collapsed);
        afterNextRender(() => {
            if (context === this.contextKey() && this.forest().byKey.has(key)) this.focus(key);
        }, {injector: this.injector});
    }

    protected scrolled(event: Event): void {
        this.scrollTop.set((event.currentTarget as HTMLElement).scrollTop);
    }

    protected choose(row: HierarchyRow): void {
        if (this.disabled()) return;
        this.focus(row.key);
        const child = this.parentingChild();
        if (child) this.requestReparent(child, row.key); else this.selectionRequested.emit(row.key);
    }

    protected toggle(event: Event, row: HierarchyRow): void {
        event.preventDefault();
        event.stopPropagation();
        if (this.disabled() || !row.children) return;
        this.setExpanded(row, !row.expanded);
        this.focus(row.key);
    }

    private setExpanded(row: HierarchyRow, expanded: boolean): void {
        const next = new Set(this.collapsed());
        if (expanded) next.delete(row.key); else next.add(row.key);
        this.collapsed.set(next);
    }

    protected keydown(event: KeyboardEvent): void {
        if (this.disabled() || event.isComposing) return;
        if (this.parentingChild() || this.dragKey()) event.stopPropagation();
        const rows = this.rows();
        const index = this.activeKey() ? this.rowIndices().get(this.activeKey()!) ?? 0 : 0;
        const row = rows[index];
        if (!row) return;
        const move = (next: number) => this.focus(rows[Math.max(0, Math.min(rows.length - 1, next))].key, true);
        switch (event.key) {
            case 'ArrowDown':
                event.preventDefault();
                move(index + 1);
                break;
            case 'ArrowUp':
                event.preventDefault();
                move(index - 1);
                break;
            case 'Home':
                event.preventDefault();
                move(0);
                break;
            case 'End':
                event.preventDefault();
                move(rows.length - 1);
                break;
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
            case 'Enter':
            case ' ':
                event.preventDefault();
                this.choose(row);
                break;
            case 'Escape':
                event.preventDefault();
                this.cancelParenting();
                this.status.set('Parenting cancelled.');
                break;
            default:
                if (event.key.length !== 1 || event.ctrlKey || event.metaKey || event.altKey || !event.key.trim()) return;
                event.preventDefault();
                this.typeahead = event.timeStamp - this.typeaheadTime > 700 ? event.key : this.typeahead + event.key;
                this.typeaheadTime = event.timeStamp;
                const query = /^([\s\S])\1+$/.test(this.typeahead) ? event.key : this.typeahead;
                for (let offset = 1; offset <= rows.length; offset++) {
                    const candidate = rows[(index + offset) % rows.length];
                    if (candidate.entity.name.toLocaleLowerCase().startsWith(query.toLocaleLowerCase())) {
                        this.focus(candidate.key, true);
                        break;
                    }
                }
        }
    }

    protected beginParenting(key?: EntityKey): void {
        const child = key === undefined ? this.primary() : this.forest().byKey.get(key);
        if (!child || this.disabled()) return;
        this.parentingChild.set(child.key);
        this.dragKey.set(null);
        this.localError.set(null);
        this.status.set(`Choose a parent for ${child.name}, then press Enter. Escape cancels.`);
        this.focus(child.key);
    }

    protected moveToRoot(): void {
        const key = this.parentingChild() ?? this.primary()?.key;
        if (key) this.requestReparent(key, null);
    }

    protected cancelParenting(): void {
        this.parentingChild.set(null);
        this.dragKey.set(null);
        this.dropTarget.set(null);
        this.localError.set(null);
        this.status.set(null);
    }

    protected dragStarted(event: DragEvent, key: EntityKey): void {
        if (this.disabled() || !event.dataTransfer || !this.forest().byKey.has(key)) {
            event.preventDefault();
            return;
        }
        this.parentingChild.set(null);
        this.localError.set(null);
        this.dragKey.set(key);
        event.dataTransfer.effectAllowed = 'move';
        event.dataTransfer.setData(ENTITY_DRAG_TYPE, key);
        this.status.set('Drop onto an entity to parent it, or onto Scene root to remove its parent.');
    }

    protected dragEnded(): void {
        this.dragKey.set(null);
        this.dropTarget.set(null);
    }

    protected dragOver(event: DragEvent, parent: EntityKey | null): void {
        const key = this.dragKey();
        if (!key || this.disabled()) return;
        const valid = this.canReparent(key, parent);
        if (valid) {
            event.preventDefault();
            if (event.dataTransfer) event.dataTransfer.dropEffect = 'move';
        }
        const previous = this.dropTarget();
        if (previous?.parent !== parent || previous?.valid !== valid) this.dropTarget.set({parent, valid});
    }

    protected dropped(event: DragEvent, parent: EntityKey | null): void {
        const key = this.dragKey();
        if (!key) return;
        event.preventDefault();
        event.stopPropagation();
        const sameSession = event.dataTransfer?.getData(ENTITY_DRAG_TYPE) === key;
        this.dropTarget.set(null);
        if (sameSession) this.requestReparent(key, parent);
        this.dragKey.set(null);
    }

    protected isDropTarget(parent: EntityKey | null): boolean {
        return this.dropTarget()?.parent === parent;
    }

    protected dragLeft(event: DragEvent): void {
        if (event.currentTarget instanceof Element && event.relatedTarget instanceof Node && event.currentTarget.contains(event.relatedTarget)) return;
        this.dropTarget.set(null);
    }

    private focus(key: EntityKey, select = false): void {
        this.focusKey.set(key);
        this.reveal(key);
        this.tree()?.nativeElement.focus({preventScroll: true});
        if (select && !this.parentingChild()) this.selectionRequested.emit(key);
    }

    private reveal(key: EntityKey): void {
        const index = this.rowIndices().get(key);
        if (index === undefined) return;
        const current = this.scrollTop();
        const next = hierarchyScrollTop(index, this.rowHeight, current, this.viewportHeight());
        if (next !== current) {
            const scrollTop = Math.max(0, next);
            this.scrollTop.set(scrollTop);
            if (this.tree()) this.tree()!.nativeElement.scrollTop = scrollTop;
        }
    }

    private canReparent(key: EntityKey, parent: EntityKey | null): boolean {
        if (this.disabled()) return false;
        return canReparentEntity(this.forest(), key, parent, candidate =>
            (this.dragKey() === key || this.parentingChild() === key) && this.forbiddenParents().has(candidate));
    }

    private requestReparent(key: EntityKey, parent: EntityKey | null): void {
        if (this.disabled()) return;
        if (!this.canReparent(key, parent)) {
            this.localError.set(this.forest().parents.get(key) === parent ? 'This entity already has that parent.' : 'An entity cannot be parented beneath itself or its descendants.');
            return;
        }
        this.localError.set(null);
        this.parentingChild.set(null);
        this.reparentRequested.emit({key, parent});
        this.status.set(parent ? 'Parent change requested. The world pose will be preserved.' : 'Move to Scene root requested. The world pose will be preserved.');
    }
}
