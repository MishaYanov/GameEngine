import type {
    EntityKey, FolderId, FolderRecord, SceneEntity, SceneOrganization,
} from '../../../../core/editor/editor.models';

export type HierarchyKey = `entity:${EntityKey}` | `folder:${FolderId}`;

export type HierarchyItem =
    | {readonly kind: 'entity'; readonly key: HierarchyKey; readonly name: string; readonly entity: SceneEntity}
    | {readonly kind: 'folder'; readonly key: HierarchyKey; readonly name: string; readonly folder: FolderRecord};

interface HierarchyForest {
    readonly byKey: ReadonlyMap<EntityKey, SceneEntity>;
    readonly children: ReadonlyMap<EntityKey | null, readonly SceneEntity[]>;
    readonly parents: ReadonlyMap<EntityKey, EntityKey | null>;
    readonly items: ReadonlyMap<HierarchyKey, HierarchyItem>;
    readonly folders: ReadonlyMap<FolderId, FolderRecord>;
    readonly membership: ReadonlyMap<EntityKey, FolderId>;
    readonly rowChildren: ReadonlyMap<HierarchyKey | null, readonly HierarchyItem[]>;
    readonly rowParents: ReadonlyMap<HierarchyKey, HierarchyKey | null>;
}

export interface HierarchyRow {
    readonly key: HierarchyKey;
    readonly item: HierarchyItem;
    readonly parent: HierarchyKey | null;
    readonly level: number;
    readonly position: number;
    readonly siblings: number;
    readonly children: boolean;
    readonly expanded: boolean;
    readonly index: number;
    readonly indent: number;
}

export function entityHierarchyKey(key: EntityKey): HierarchyKey {
    return `entity:${key}`;
}

export function folderHierarchyKey(id: FolderId): HierarchyKey {
    return `folder:${id}`;
}

export function buildHierarchy(
    entities: readonly SceneEntity[],
    organization?: SceneOrganization,
): HierarchyForest {
    const byKey = new Map(entities.map(entity => [entity.key, entity]));
    const children = new Map<EntityKey | null, SceneEntity[]>();
    const parents = new Map<EntityKey, EntityKey | null>();
    const folders = new Map(organization?.folders.map(folder => [folder.id, folder]));
    const membership = new Map(Object.entries(organization?.root_membership ?? {}));
    const items = new Map<HierarchyKey, HierarchyItem>();
    const rowParents = new Map<HierarchyKey, HierarchyKey | null>();
    const rowChildren = new Map<HierarchyKey | null, HierarchyItem[]>();
    const append = (item: HierarchyItem, parent: HierarchyKey | null) => {
        items.set(item.key, item);
        rowParents.set(item.key, parent);
        const siblings = rowChildren.get(parent);
        if (siblings) siblings.push(item); else rowChildren.set(parent, [item]);
    };
    for (const folder of folders.values()) {
        const parent = folder.parent && folder.parent !== folder.id && folders.has(folder.parent)
            ? folderHierarchyKey(folder.parent)
            : null;
        append({kind: 'folder', key: folderHierarchyKey(folder.id), name: folder.name, folder}, parent);
    }
    for (const entity of entities) {
        const parent = entity.parent && byKey.has(entity.parent) && entity.parent !== entity.key ? entity.parent : null;
        parents.set(entity.key, parent);
        const siblings = children.get(parent);
        if (siblings) siblings.push(entity); else children.set(parent, [entity]);
        const folder = membership.get(entity.key);
        const presentationParent = parent ? entityHierarchyKey(parent) :
            folder && folders.has(folder) ? folderHierarchyKey(folder) : null;
        append({kind: 'entity', key: entityHierarchyKey(entity.key), name: entity.name, entity}, presentationParent);
    }
    return {byKey, children, parents, items, folders, membership, rowChildren, rowParents};
}

export function visibleHierarchyRows(
    forest: HierarchyForest,
    collapsed: ReadonlySet<HierarchyKey>,
    query = '',
): readonly HierarchyRow[] {
    const {rowChildren, rowParents} = forest;
    const matches = searchMatches(forest, query.trim().toLocaleLowerCase());
    const shown = (parent: HierarchyKey | null) => {
        const children = rowChildren.get(parent) ?? [];
        return matches ? children.filter(child => matches.has(child.key)) : children;
    };
    const roots = shown(null);
    const stack = roots.map((item, index) => ({
        item,
        level: 1,
        position: index + 1,
        siblings: roots.length
    })).reverse();
    const rows: HierarchyRow[] = [];
    const visited = new Set<HierarchyKey>();
    // An iterative traversal also handles deeply nested valid scenes.
    while (stack.length) {
        const entry = stack.pop()!;
        const key = entry.item.key;
        if (visited.has(key)) continue;
        visited.add(key);
        const descendants = shown(key);
        const expanded = descendants.length > 0 && (matches !== null || !collapsed.has(key));
        rows.push({
            ...entry, key, parent: rowParents.get(key) ?? null, children: descendants.length > 0,
            expanded, index: rows.length, indent: 6 + Math.min(entry.level - 1, 8) * 12
        });
        if (expanded) for (let index = descendants.length - 1; index >= 0; index--) {
            stack.push({
                item: descendants[index],
                level: entry.level + 1,
                position: index + 1,
                siblings: descendants.length
            });
        }
    }
    return rows;
}

export function hierarchyAncestors({rowParents}: HierarchyForest, key: HierarchyKey): readonly HierarchyKey[] {
    const ancestors: HierarchyKey[] = [];
    const seen = new Set<HierarchyKey>();
    let parent = rowParents.get(key);
    while (parent && !seen.has(parent)) {
        seen.add(parent);
        ancestors.push(parent);
        parent = rowParents.get(parent);
    }
    return ancestors;
}

function searchMatches({items, rowParents}: HierarchyForest, query: string): ReadonlySet<HierarchyKey> | null {
    if (!query) return null;
    const matches = new Set<HierarchyKey>();
    for (const item of items.values()) {
        if (!item.name.toLocaleLowerCase().includes(query)) continue;
        for (let key: HierarchyKey | null = item.key; key && !matches.has(key); key = rowParents.get(key) ?? null) {
            matches.add(key);
        }
    }
    return matches;
}

export function hierarchySubtreeKeys({children}: HierarchyForest, root: EntityKey): ReadonlySet<EntityKey> {
    const subtree = new Set<EntityKey>();
    const stack = [root];
    while (stack.length) {
        const key = stack.pop()!;
        if (subtree.has(key)) continue;
        subtree.add(key);
        for (const child of children.get(key) ?? []) stack.push(child.key);
    }
    return subtree;
}

export function canReparentEntity(
    {byKey, parents}: HierarchyForest,
    key: EntityKey,
    parent: EntityKey | null,
    isForbiddenParent: (candidate: EntityKey) => boolean
): boolean {
    if (!byKey.has(key) || parents.get(key) === parent) return false;
    if (parent === null) return true;
    if (!byKey.has(parent) || isForbiddenParent(parent)) return false;
    const seen = new Set<EntityKey>();
    let ancestor: EntityKey | null | undefined = parent;
    while (ancestor && !seen.has(ancestor)) {
        if (ancestor === key) return false;
        seen.add(ancestor);
        ancestor = parents.get(ancestor);
    }
    return !ancestor;
}
