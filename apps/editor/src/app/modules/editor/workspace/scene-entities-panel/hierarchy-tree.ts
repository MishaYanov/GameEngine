import type {EntityKey, SceneEntity} from '../../../../core/editor/editor.models';

interface HierarchyForest {
    readonly byKey: ReadonlyMap<EntityKey, SceneEntity>;
    readonly children: ReadonlyMap<EntityKey | null, readonly SceneEntity[]>;
    readonly parents: ReadonlyMap<EntityKey, EntityKey | null>;
}

export interface HierarchyRow {
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

export function buildHierarchy(entities: readonly SceneEntity[]): HierarchyForest {
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
}

export function visibleHierarchyRows(
    {children, parents}: HierarchyForest,
    collapsed: ReadonlySet<EntityKey>
): readonly HierarchyRow[] {
    const roots = children.get(null) ?? [];
    const stack = roots.map((entity, index) => ({
        entity,
        level: 1,
        position: index + 1,
        siblings: roots.length
    })).reverse();
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
        rows.push({
            ...entry, key, parent: parents.get(key) ?? null, children: descendants.length > 0,
            expanded, index: rows.length, indent: 6 + Math.min(entry.level - 1, 8) * 12
        });
        if (expanded) for (let index = descendants.length - 1; index >= 0; index--) {
            stack.push({
                entity: descendants[index],
                level: entry.level + 1,
                position: index + 1,
                siblings: descendants.length
            });
        }
    }
    return rows;
}

export function hierarchyAncestors({parents}: HierarchyForest, key: EntityKey): readonly EntityKey[] {
    const ancestors: EntityKey[] = [];
    const seen = new Set<EntityKey>();
    let parent = parents.get(key);
    while (parent && !seen.has(parent)) {
        seen.add(parent);
        ancestors.push(parent);
        parent = parents.get(parent);
    }
    return ancestors;
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
