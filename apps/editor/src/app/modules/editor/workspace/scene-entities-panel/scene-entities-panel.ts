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
import type {EntityKey, FolderId, FolderRecord} from '../../../../core/editor/editor.models';
import {EditorPane} from '../../../../shared/components/pane/pane';
import {Drawer} from '../../../../shared/components/drawer/drawer';
import {EditorIcon} from '../../../../shared/icons/editor-icon';
import type {Config} from '../../../../shared/models/config.model';
import type {DrawerSettings} from '../../../../shared/components/drawer/drawer-settings';
import type {SceneEntitiesModel} from './scene-entities.model';
import {
    buildHierarchy,
    canReparentEntity,
    entityHierarchyKey,
    folderHierarchyKey,
    hierarchyAncestors,
    type HierarchyItem,
    type HierarchyKey,
    type HierarchyRow,
    hierarchySubtreeKeys,
    visibleHierarchyRows
} from './hierarchy-tree';
import {hierarchyScrollTop, renderHierarchyWindow} from './hierarchy-viewport';
import {ContextActionBuilder} from '../../../../shared/actions/context-action';
import {ContextMenuBuilder, type ContextMenuItem} from '../../../../shared/builder/context-menu-builder';
import {EditorContextMenuTrigger} from '../../../../shared/components/context-menu/context-menu-trigger';
import {ContextMenuService} from '../../../../shared/components/context-menu/context-menu.service';
import {
    type FolderDestination, type FolderEdit, type HierarchyFolderCommand, HierarchyFolderEditor,
} from './hierarchy-folder-editor';

export interface HierarchyReparentRequest {
    readonly key: EntityKey;
    readonly parent: EntityKey | null;
    readonly keys?: readonly EntityKey[];
    readonly keepWorld?: boolean;
}

interface EntityMenuContext {
    readonly key: EntityKey;
    readonly contextKey: string;
}

interface FolderMenuContext {
    readonly id: FolderId;
    readonly contextKey: string;
}

type HierarchyDrop = {readonly key: HierarchyKey | null; readonly valid: boolean};

interface HierarchyDragSession {
    readonly context: string;
    readonly roots: readonly EntityKey[];
}

const ENTITY_DRAG_TYPE = 'application/x-game-engine-hierarchy-entity';
let nextHierarchyId = 0;

@Component({
    selector: 'scene-entities-panel',
    imports: [EditorPane, Drawer, EditorIcon, EditorContextMenuTrigger, HierarchyFolderEditor],
    templateUrl: './scene-entities-panel.html',
    styleUrl: './scene-entities-panel.scss',
})
export class SceneEntitiesPanel {
    readonly scene = input.required<SceneEntitiesModel>();
    readonly selectedKeys = input<readonly EntityKey[]>([]);
    readonly disabled = input(false);
    readonly foldersEnabled = input(false);
    readonly batchParentingEnabled = input(false);
    readonly cameraCreationEnabled = input(false);
    readonly sceneLabel = input('Scene');
    readonly contextKey = input('');
    readonly actionError = input<string | null>(null);
    readonly selectionRequested = output<EntityKey>();
    readonly selectionSetRequested = output<readonly EntityKey[]>();
    readonly usagesRequested = output<EntityKey>();
    readonly reparentRequested = output<HierarchyReparentRequest>();
    readonly folderRequested = output<HierarchyFolderCommand>();
    readonly cameraRequested = output<void>();

    protected readonly rowHeight = 28;
    protected readonly treeId = `editor-hierarchy-${nextHierarchyId++}`;
    protected readonly selection = computed(() => new Set(this.selectedKeys()));
    protected readonly collapsed = signal<ReadonlySet<HierarchyKey>>(new Set());
    protected readonly focusKey = signal<HierarchyKey | null>(null);
    protected readonly parentingChild = signal<EntityKey | null>(null);
    protected readonly keepWorld = signal(true);
    protected readonly dragKey = signal<EntityKey | null>(null);
    protected readonly dropTarget = signal<HierarchyDrop | null>(null);
    protected readonly search = signal('');
    protected readonly folderEdit = signal<FolderEdit | null>(null);
    private readonly localError = signal<string | null>(null);
    protected readonly error = computed(() => this.localError() ?? this.actionError());
    protected readonly status = signal<string | null>(null);
    private readonly scrollTop = signal(0);
    private readonly viewportHeight = signal(280);
    private readonly drawer = viewChild(Drawer);
    private readonly tree = viewChild<ElementRef<HTMLElement>>('tree');
    private readonly newFolder = viewChild<ElementRef<HTMLButtonElement>>('newFolder');
    private readonly injector = inject(Injector);
    private destroyed = false;
    private previousContext: string | null = null;
    private previousSelectionIdentity = '';
    private previousRowParents: ReadonlyMap<HierarchyKey, HierarchyKey | null> = new Map();
    private selectionAnchor: EntityKey | null = null;
    private dragSession: HierarchyDragSession | null = null;
    private typeahead = '';
    private typeaheadTime = 0;

    protected readonly renderedRows = computed(() => {
        const active = this.activeKey();
        const activeIndex = active ? this.rowIndices().get(active) : undefined;
        return renderHierarchyWindow(this.rows(), this.rowHeight, this.scrollTop(), this.viewportHeight(), activeIndex);
    });
    private readonly forest = computed(() => {
        const scene = this.scene();
        return buildHierarchy(scene.entities, scene.organization);
    });
    private readonly rowIndices = computed(() => new Map(this.rows().map(row => [row.key, row.index])));
    protected readonly activeKey = computed(() => {
        const indices = this.rowIndices();
        const focused = this.focusKey();
        if (focused && indices.has(focused)) return focused;
        return this.selectedKeys().map(entityHierarchyKey).find(key => indices.has(key)) ?? this.rows()[0]?.key ?? null;
    });
    protected readonly activeId = computed(() => this.activeKey() ? this.rowId(this.activeKey()!) : null);
    protected readonly primary = computed(() => this.forest().byKey.get(this.selectedKeys()[0]) ?? null);
    protected readonly totalHeight = computed(() => this.rows().length * this.rowHeight);
    protected readonly rows = computed(() => visibleHierarchyRows(this.forest(), this.collapsed(), this.search()));
    protected readonly canMoveSelection = computed(() =>
        this.canEditFolders() && this.canOrganize(this.selectedKeys()),
    );
    protected readonly canMoveToRoot = computed(() => {
        const key = this.parentingChild() ?? this.primary()?.key;
        return key !== undefined && this.canDropOnRoot(key);
    });
    protected readonly folderDestinations = computed(() => {
        const edit = this.folderEdit();
        if (edit?.kind !== 'move-folder' && edit?.kind !== 'move-objects') return [];
        const forest = this.forest();
        const destinations: FolderDestination[] = [];
        let canUse: (id: FolderId | null) => boolean;
        if (edit.kind === 'move-folder') {
            canUse = id => this.canMoveFolder(edit.id, id);
        } else {
            if (!this.canOrganize(edit.roots)) return [];
            const current = forest.membership.get(edit.roots[0]) ?? null;
            const uniform = edit.roots.every(key => (forest.membership.get(key) ?? null) === current);
            canUse = id => !uniform || id !== current;
        }
        if (canUse(null)) destinations.push({id: null, label: 'Scene root'});
        for (const folder of forest.folders.values()) {
            if (!canUse(folder.id)) continue;
            const path = hierarchyAncestors(forest, folderHierarchyKey(folder.id)).map(key => forest.items.get(key)?.name);
            destinations.push({id: folder.id, label: [...path.reverse(), folder.name].join(' / ')});
        }
        return destinations;
    });
    private readonly forbiddenParents = computed(() => {
        const source = this.dragKey() ?? this.parentingChild();
        return source ? hierarchySubtreeKeys(this.forest(), this.parentingKeys(source)) : new Set<EntityKey>();
    });

    protected readonly drawerConfig: Config<DrawerSettings> = {width: '14rem', minWidth: '8rem'};
    protected readonly contextMenu = inject(ContextMenuService);
    protected readonly canCreateCamera = computed(() => this.cameraCreationEnabled() && this.scene().projectOpen && !this.disabled());
    private readonly createCameraAction = ContextActionBuilder.create<{readonly contextKey: string}>('hierarchy.create.camera', 'Camera')
        .when(() => this.cameraCreationEnabled())
        .enabledWhen(context => this.canCreateCamera() && context.contextKey === this.contextKey())
        .run(context => {
            if (this.canCreateCamera() && context.contextKey === this.contextKey()) this.cameraRequested.emit();
        }).build();
    private readonly createMenuBuilder = ContextMenuBuilder.create<{readonly contextKey: string}>().action(this.createCameraAction);
    protected readonly createMenu = () => this.createMenuBuilder.build({contextKey: this.contextKey()});
    private readonly entityMenuBuilder = ContextMenuBuilder.create<EntityMenuContext>()
        .action(ContextActionBuilder.create<EntityMenuContext>('entity.select', 'Select')
            .icon('object')
            .enabledWhen(context => this.canUseEntityMenu(context))
            .run(context => {
                this.cancelParenting();
                this.focus(entityHierarchyKey(context.key));
                this.selectionRequested.emit(context.key);
            }).build())
        .action(ContextActionBuilder.create<EntityMenuContext>('entity.find-usages', 'Find Usages…')
            .icon('graph')
            .enabledWhen(context => this.canUseEntityMenu(context))
            .run(context => this.usagesRequested.emit(context.key)).build())
        .separator()
        .submenu('entity.parenting', 'Parenting', menu => menu
            .action(ContextActionBuilder.create<EntityMenuContext>('entity.set-parent', 'Make Child Of…')
                .enabledWhen(context => this.canUseEntityMenu(context))
                .run(context => this.beginParenting(context.key)).build())
            .action(ContextActionBuilder.create<EntityMenuContext>('entity.move-to-root', 'Move to Root')
                .enabledWhen(context => this.canUseEntityMenu(context) && this.canDropOnRoot(context.key))
                .run(context => this.requestDropOnRoot(context.key)).build()))
        .action(ContextActionBuilder.create<EntityMenuContext>('entity.move-to-folder', 'Move to Folder…')
            .icon('folder')
            .when(() => this.foldersEnabled())
            .enabledWhen(context => this.canUseEntityMenu(context) && this.canOrganize(this.dragSelection(context.key)))
            .run(context => this.beginMoveObjects(this.dragSelection(context.key))).build())
        .submenu('entity.create', 'Create', items => items.action(this.createCameraAction));
    private readonly folderMenuBuilder = ContextMenuBuilder.create<FolderMenuContext>()
        .action(ContextActionBuilder.create<FolderMenuContext>('folder.create', 'New Subfolder…')
            .icon('folder')
            .enabledWhen(context => this.canUseFolderMenu(context))
            .run(context => this.beginFolderEdit({kind: 'create', parent: context.id})).build())
        .action(ContextActionBuilder.create<FolderMenuContext>('folder.rename', 'Rename Folder…')
            .enabledWhen(context => this.canUseFolderMenu(context))
            .run(context => this.editFolder(context.id, 'rename')).build())
        .action(ContextActionBuilder.create<FolderMenuContext>('folder.move', 'Move Folder…')
            .enabledWhen(context => this.canUseFolderMenu(context))
            .run(context => this.editFolder(context.id, 'move-folder')).build())
        .action(ContextActionBuilder.create<FolderMenuContext>('folder.remove', 'Remove Folder')
            .enabledWhen(context => this.canUseFolderMenu(context))
            .run(context => this.submitFolderEdit({type: 'remove_folder', id: context.id})).build())
        .submenu('folder.create-object', 'Create', items => items.action(this.createCameraAction));
    protected readonly activeItemMenu = (): readonly ContextMenuItem[] => {
        const key = this.activeKey();
        const item = key ? this.forest().items.get(key) : undefined;
        return item ? this.itemMenu(item)() : [];
    };

    protected itemMenu(item: HierarchyItem): () => readonly ContextMenuItem[] {
        const contextKey = this.contextKey();
        return item.kind === 'entity'
            ? () => this.entityMenuBuilder.build({key: item.entity.key, contextKey})
            : () => this.folderMenuBuilder.build({id: item.folder.id, contextKey});
    }

    protected openCreateMenu(button: HTMLButtonElement): void {
        if (!this.canCreateCamera()) return;
        if (this.contextMenu.request()?.owner === button) {
            this.contextMenu.close();
            return;
        }
        const bounds = button.getBoundingClientRect();
        this.contextMenu.open({
            items: this.createMenu(), label: 'Create scene object', x: bounds.left, y: bounds.bottom,
            owner: button, returnFocus: button,
        });
    }

    private canUseEntityMenu(context: EntityMenuContext): boolean {
        return context.contextKey === this.contextKey() && this.scene().projectOpen &&
            !this.disabled() && this.forest().byKey.has(context.key);
    }

    private canUseFolderMenu(context: FolderMenuContext): boolean {
        return context.contextKey === this.contextKey() &&
            this.canEditFolders() && this.forest().folders.has(context.id);
    }

    constructor() {
        inject(DestroyRef).onDestroy(() => this.destroyed = true);
        effect(() => {
            const context = this.contextKey();
            const forest = this.forest();
            const selection = this.selectedKeys();
            const selected = selection[0] ?? null;
            const disabled = this.disabled();
            untracked(() => {
                if (this.previousContext !== context) {
                    this.previousContext = context;
                    this.previousSelectionIdentity = '';
                    this.collapsed.set(new Set());
                    this.focusKey.set(null);
                    this.scrollTop.set(0);
                    this.search.set('');
                    this.folderEdit.set(null);
                    this.selectionAnchor = null;
                    this.previousRowParents = new Map();
                    if (this.tree()) this.tree()!.nativeElement.scrollTop = 0;
                    this.cancelParenting();
                }
                const selectedRow = selected ? entityHierarchyKey(selected) : null;
                const ancestors = selectedRow ? hierarchyAncestors(forest, selectedRow) : [];
                const identity = JSON.stringify([context, selected, ancestors]);
                if (selectedRow && forest.items.has(selectedRow) && identity !== this.previousSelectionIdentity) {
                    const next = new Set(this.collapsed());
                    for (const ancestor of ancestors) next.delete(ancestor);
                    this.collapsed.set(next);
                    const focused = this.focusKey();
                    const acceptsFocused = selection.some(key => entityHierarchyKey(key) === focused);
                    if (!acceptsFocused) this.focusKey.set(selectedRow);
                    queueMicrotask(() => {
                        const key = this.activeKey();
                        if (!this.destroyed && key) this.reveal(key);
                    });
                }
                this.previousSelectionIdentity = identity;
                this.restoreRemovedFocus(forest.items);
                const collapsed = this.collapsed();
                if ([...collapsed].some(key => !forest.items.has(key))) {
                    this.collapsed.set(new Set([...collapsed].filter(key => forest.items.has(key))));
                }
                this.previousRowParents = forest.rowParents;
                if (this.selectionAnchor && !forest.byKey.has(this.selectionAnchor)) this.selectionAnchor = null;
                if (disabled) {
                    this.dragEnded();
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

    protected rowId(key: HierarchyKey): string {
        return `${this.treeId}-${encodeURIComponent(key)}`;
    }

    revealEntity(key: EntityKey): void {
        const forest = this.forest();
        if (!forest.byKey.has(key)) return;
        const context = this.contextKey();
        this.drawer()?.config.update(config => ({...config, collapsed: false, expanded: true}));
        const collapsed = new Set(this.collapsed());
        const rowKey = entityHierarchyKey(key);
        for (const ancestor of hierarchyAncestors(forest, rowKey)) collapsed.delete(ancestor);
        this.collapsed.set(collapsed);
        this.search.set('');
        afterNextRender(() => {
            if (context === this.contextKey() && this.forest().byKey.has(key)) this.focus(rowKey);
        }, {injector: this.injector});
    }

    protected scrolled(event: Event): void {
        this.scrollTop.set((event.currentTarget as HTMLElement).scrollTop);
    }

    protected choose(row: HierarchyRow, event?: MouseEvent | KeyboardEvent): void {
        if (this.disabled()) return;
        this.focus(row.key);
        if (row.item.kind === 'folder') {
            if (!this.parentingChild()) this.setExpanded(row, !row.expanded);
            return;
        }
        const key = row.item.entity.key;
        const child = this.parentingChild();
        if (child) this.requestReparent(child, key);
        else if (event?.shiftKey) this.selectRange(key);
        else if (event?.ctrlKey || event?.metaKey ||
            event instanceof KeyboardEvent && event.key === ' ' && this.foldersEnabled()) this.toggleSelection(key);
        else this.selectSingle(key);
    }

    protected toggle(event: Event, row: HierarchyRow): void {
        event.preventDefault();
        event.stopPropagation();
        if (this.disabled() || !row.children) return;
        this.setExpanded(row, !row.expanded);
        this.focus(row.key);
    }

    private setExpanded(row: HierarchyRow, expanded: boolean): void {
        if (this.search().trim()) return;
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
        const move = (next: number) => this.moveKeyboardFocus(
            rows[Math.max(0, Math.min(rows.length - 1, next))].key, event, row,
        );
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
                else if (row.parent) this.moveKeyboardFocus(row.parent, event, row);
                break;
            case 'Enter':
            case ' ':
                event.preventDefault();
                this.choose(row, event);
                break;
            case 'Escape':
                event.preventDefault();
                if (this.parentingChild() || this.dragKey()) {
                    this.cancelParenting();
                    this.status.set('Parenting cancelled.');
                } else if (this.search()) this.search.set('');
                break;
            case 'F2':
            case 'Delete':
                if (row.item.kind === 'folder' && this.canEditFolders()) {
                    event.preventDefault();
                    event.stopPropagation();
                    if (event.key === 'F2') this.editFolder(row.item.folder.id, 'rename');
                    else this.submitFolderEdit({type: 'remove_folder', id: row.item.folder.id});
                }
                break;
            default:
                if (event.key.length !== 1 || event.ctrlKey || event.metaKey || event.altKey || !event.key.trim()) return;
                event.preventDefault();
                this.typeahead = event.timeStamp - this.typeaheadTime > 700 ? event.key : this.typeahead + event.key;
                this.typeaheadTime = event.timeStamp;
                const query = /^([\s\S])\1+$/.test(this.typeahead) ? event.key : this.typeahead;
                for (let offset = 1; offset <= rows.length; offset++) {
                    const candidate = rows[(index + offset) % rows.length];
                    if (candidate.item.name.toLocaleLowerCase().startsWith(query.toLocaleLowerCase())) {
                        this.focus(candidate.key, true);
                        break;
                    }
                }
        }
    }

    protected beginParenting(key?: EntityKey): void {
        const child = key === undefined ? this.primary() : this.forest().byKey.get(key);
        if (!child || this.disabled()) return;
        this.folderEdit.set(null);
        this.parentingChild.set(child.key);
        this.dragKey.set(null);
        this.localError.set(null);
        this.keepWorld.set(true);
        const count = this.parentingKeys(child.key).length;
        const description = count > 1 ? `${count} selected objects` : child.name;
        this.status.set(`Choose a parent for ${description}, then press Enter. Escape cancels.`);
        this.focus(entityHierarchyKey(child.key));
    }

    protected moveToRoot(): void {
        const key = this.parentingChild() ?? this.primary()?.key;
        if (key) this.requestDropOnRoot(key);
    }

    protected setKeepWorld(event: Event): void {
        if (event.target instanceof HTMLInputElement) this.keepWorld.set(event.target.checked);
    }

    protected cancelParenting(): void {
        this.parentingChild.set(null);
        this.dragEnded();
        this.localError.set(null);
        this.status.set(null);
    }

    protected dragStarted(event: DragEvent, key: EntityKey): void {
        if (this.disabled() || !event.dataTransfer || !this.forest().byKey.has(key)) {
            event.preventDefault();
            return;
        }
        this.folderEdit.set(null);
        this.parentingChild.set(null);
        this.keepWorld.set(true);
        this.localError.set(null);
        this.dragSession = {context: this.contextKey(), roots: [...this.dragSelection(key)]};
        this.dragKey.set(key);
        event.dataTransfer.effectAllowed = 'move';
        event.dataTransfer.setData(ENTITY_DRAG_TYPE, key);
        this.status.set(this.foldersEnabled()
            ? 'Drop scene root objects onto a folder, or drop an object onto another object to parent it.'
            : 'Drop onto an entity to parent it, or onto Scene root to remove its parent.');
    }

    protected dragEnded(): void {
        if (this.dragKey()) this.status.set(null);
        this.dragSession = null;
        this.dragKey.set(null);
        this.dropTarget.set(null);
    }

    protected dragOver(event: DragEvent, item: HierarchyItem | null): void {
        const key = this.dragKey();
        const drag = this.dragSession;
        if (!key || !drag || drag.context !== this.contextKey() || this.disabled()) return;
        const target = item?.key ?? null;
        const valid = item === null ? this.canDropOnRoot(key, drag.roots) :
            item.kind === 'folder'
                ? this.canEditFolders() && this.canOrganize(drag.roots, item.folder.id)
                : this.canReparent(key, item.entity.key);
        if (valid) {
            event.preventDefault();
            event.stopPropagation();
            if (event.dataTransfer) event.dataTransfer.dropEffect = 'move';
        }
        const previous = this.dropTarget();
        if (previous?.key !== target || previous?.valid !== valid) this.dropTarget.set({key: target, valid});
    }

    protected dropped(event: DragEvent, item: HierarchyItem | null): void {
        const key = this.dragKey();
        const drag = this.dragSession;
        if (!key || !drag || drag.context !== this.contextKey()) return;
        event.preventDefault();
        event.stopPropagation();
        const sameSession = event.dataTransfer?.getData(ENTITY_DRAG_TYPE) === key;
        this.dragEnded();
        if (sameSession) {
            if (item === null) this.requestDropOnRoot(key, drag.roots);
            else if (item.kind === 'entity') this.requestReparent(key, item.entity.key);
            else this.submitFolderEdit({type: 'move_roots_to_folder', roots: drag.roots, folder: item.folder.id});
        }
    }

    protected isDropTarget(key: HierarchyKey | null): boolean {
        return this.dropTarget()?.key === key;
    }

    protected dragLeft(event: DragEvent): void {
        if (event.currentTarget instanceof Element && event.relatedTarget instanceof Node && event.currentTarget.contains(event.relatedTarget)) return;
        this.dropTarget.set(null);
    }

    protected createFolder(): void {
        const key = this.activeKey();
        const item = key ? this.forest().items.get(key) : undefined;
        this.beginFolderEdit({kind: 'create', parent: item?.kind === 'folder' ? item.folder.id : null});
    }

    protected moveSelectionToFolder(): void {
        this.beginMoveObjects(this.selectedKeys());
    }

    protected closeFolderEdit(): void {
        this.folderEdit.set(null);
        const active = this.activeKey();
        if (active) this.focus(active);
        else this.newFolder()?.nativeElement.focus();
    }

    protected submitFolderEdit(command: HierarchyFolderCommand): void {
        if (!this.canEditFolders()) return;
        const folders = this.forest().folders;
        switch (command.type) {
            case 'create_folder':
                if (command.parent !== null && !folders.has(command.parent)) return;
                break;
            case 'rename_folder':
            case 'remove_folder':
                if (!folders.has(command.id)) return;
                break;
            case 'move_folder':
                if (!this.canMoveFolder(command.id, command.parent)) return;
                break;
            case 'move_roots_to_folder':
                if (!this.canOrganize(command.roots, command.folder)) return;
                break;
        }
        if (this.folderEdit()) this.closeFolderEdit();
        this.localError.set(null);
        this.status.set(null);
        this.folderRequested.emit(command);
    }

    private canEditFolders(): boolean {
        return this.foldersEnabled() && this.scene().projectOpen && !this.disabled();
    }

    private beginFolderEdit(edit: FolderEdit): void {
        if (!this.canEditFolders()) return;
        this.cancelParenting();
        this.folderEdit.set(edit);
    }

    private beginMoveObjects(roots: readonly EntityKey[]): void {
        if (this.canOrganize(roots)) this.beginFolderEdit({kind: 'move-objects', roots: [...roots]});
    }

    private editFolder(id: FolderId, kind: 'move-folder' | 'rename'): void {
        const folder = this.forest().folders.get(id);
        if (folder) {
            this.focus(folderHierarchyKey(id));
            this.beginFolderEdit({kind, id, name: folder.name});
        }
    }

    private canMoveFolder(id: FolderId, target: FolderId | null): boolean {
        const forest = this.forest();
        const source = forest.folders.get(id);
        if (!source || (source.parent ?? null) === target || id === target) return false;
        return target === null || forest.folders.has(target) &&
            !hierarchyAncestors(forest, folderHierarchyKey(target)).includes(folderHierarchyKey(id));
    }

    private dragSelection(key: EntityKey): readonly EntityKey[] {
        return this.selection().has(key) ? this.selectedKeys() : [key];
    }

    private canOrganize(roots: readonly EntityKey[], folder?: FolderId | null): boolean {
        const {byKey, folders, membership} = this.forest();
        if (!roots.length || !roots.every(key => {
            const entity = byKey.get(key);
            return entity !== undefined && (entity.parent ?? null) === null;
        })) return false;
        if (folder === undefined) return folders.size > 0;
        if (folder !== null && !folders.has(folder)) return false;
        return roots.some(key => (membership.get(key) ?? null) !== folder);
    }

    private canDropOnRoot(key: EntityKey, roots = this.dragSelection(key)): boolean {
        return this.canReparent(key, null) || this.canEditFolders() && this.canOrganize(roots, null);
    }

    private requestDropOnRoot(key: EntityKey, roots = this.dragSelection(key)): void {
        if (this.canEditFolders() && this.canOrganize(roots, null)) {
            this.submitFolderEdit({type: 'move_roots_to_folder', roots, folder: null});
        } else {
            this.requestReparent(key, null);
        }
    }

    private restoreRemovedFocus(items: ReadonlyMap<HierarchyKey, HierarchyItem>): void {
        let focus = this.focusKey();
        const seen = new Set<HierarchyKey>();
        while (focus && !items.has(focus) && !seen.has(focus)) {
            seen.add(focus);
            focus = this.previousRowParents.get(focus) ?? null;
        }
        this.focusKey.set(focus && items.has(focus) ? focus : null);
    }

    private moveKeyboardFocus(key: HierarchyKey, event: KeyboardEvent, previous: HierarchyRow): void {
        const item = this.forest().items.get(key);
        if (event.shiftKey && item?.kind === 'entity' && !this.parentingChild()) {
            if (!this.selectionAnchor && previous.item.kind === 'entity') {
                this.selectionAnchor = previous.item.entity.key;
            }
            this.focus(key);
            this.selectRange(item.entity.key);
        } else {
            this.focus(key, !event.ctrlKey && !event.metaKey && !event.shiftKey);
        }
    }

    private selectSingle(key: EntityKey): void {
        this.selectionAnchor = key;
        this.selectionRequested.emit(key);
    }

    private toggleSelection(key: EntityKey): void {
        this.selectionAnchor = key;
        const selected = this.selectedKeys();
        this.selectionSetRequested.emit(
            this.selection().has(key) ? selected.filter(candidate => candidate !== key) : [...selected, key],
        );
    }

    private selectRange(key: EntityKey): void {
        const anchor = this.selectionAnchor ? this.rowIndices().get(entityHierarchyKey(this.selectionAnchor)) : undefined;
        const destination = this.rowIndices().get(entityHierarchyKey(key));
        if (anchor === undefined || destination === undefined) {
            this.selectSingle(key);
            return;
        }
        const selected = this.rows().slice(Math.min(anchor, destination), Math.max(anchor, destination) + 1);
        if (anchor > destination) selected.reverse();
        this.selectionSetRequested.emit(selected.flatMap(row => row.item.kind === 'entity' ? [row.item.entity.key] : []));
    }

    private focus(key: HierarchyKey, select = false): void {
        this.focusKey.set(key);
        this.reveal(key);
        this.tree()?.nativeElement.focus({preventScroll: true});
        const item = this.forest().items.get(key);
        if (select && !this.parentingChild() && item?.kind === 'entity') this.selectSingle(item.entity.key);
    }

    private reveal(key: HierarchyKey): void {
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
        const forest = this.forest();
        const keys = this.parentingKeys(key);
        if ((this.dragKey() === key || this.parentingChild() === key) &&
            parent !== null && this.forbiddenParents().has(parent)) return false;
        return keys.every(candidate => forest.byKey.has(candidate) &&
            (forest.parents.get(candidate) === parent ||
                canReparentEntity(forest, candidate, parent, () => false))) &&
            keys.some(candidate => forest.parents.get(candidate) !== parent);
    }

    private parentingKeys(key: EntityKey): readonly EntityKey[] {
        return this.batchParentingEnabled() ? this.dragSelection(key) : [key];
    }

    private requestReparent(key: EntityKey, parent: EntityKey | null): void {
        if (this.disabled()) return;
        if (!this.canReparent(key, parent)) {
            this.localError.set(this.forest().parents.get(key) === parent ? 'This entity already has that parent.' : 'An entity cannot be parented beneath itself or its descendants.');
            return;
        }
        this.localError.set(null);
        const keepWorld = !this.parentingChild() || this.keepWorld();
        this.parentingChild.set(null);
        this.reparentRequested.emit(this.batchParentingEnabled()
            ? {key, parent, keys: this.parentingKeys(key), keepWorld}
            : {key, parent});
        this.status.set(keepWorld ? 'Parent change requested. The world pose will be preserved.'
            : 'Parent change requested. The local transform will be preserved.');
    }
}
