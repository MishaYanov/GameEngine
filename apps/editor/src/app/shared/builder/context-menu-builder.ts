import type {ContextAction} from '../actions/context-action';
import type {EditorIconName} from '../icons/editor-icon';

export type ContextMenuItem =
    | {
    readonly kind: 'action';
    readonly id: string;
    readonly label: string;
    readonly icon?: EditorIconName;
    readonly shortcut?: string;
    readonly destructive: boolean;
    readonly disabled: boolean;
    readonly execute: () => void | Promise<unknown>;
}
    | {
    readonly kind: 'submenu';
    readonly id: string;
    readonly label: string;
    readonly icon?: EditorIconName;
    readonly children: readonly ContextMenuItem[];
}
    | { readonly kind: 'separator'; readonly id: string };

type MenuDefinition<T> =
    | { readonly kind: 'action'; readonly id: string; readonly action: ContextAction<T> }
    | {
    readonly kind: 'submenu';
    readonly id: string;
    readonly label: string;
    readonly icon?: EditorIconName;
    readonly children: readonly MenuDefinition<T>[];
}
    | { readonly kind: 'separator'; readonly id: string };

/** Builds a menu tree from shared actions, resolving it against one clicked item. */
export class ContextMenuBuilder<T> {
    private readonly definitions: MenuDefinition<T>[] = [];
    private readonly ids = new Set<string>();
    private nextSeparator = 0;

    static create<T>(): ContextMenuBuilder<T> {
        return new ContextMenuBuilder<T>();
    }

    action(action: ContextAction<T>): this {
        this.reserveId(action.id);
        this.definitions.push({kind: 'action', id: action.id, action: Object.freeze({...action})});
        return this;
    }

    submenu(
        id: string,
        label: string,
        configure: (builder: ContextMenuBuilder<T>) => void,
        icon?: EditorIconName,
    ): this {
        if (!label.trim()) throw new Error('A context submenu requires a label.');
        if (!id.trim()) throw new Error('A context menu item requires an id.');
        if (this.ids.has(id)) throw new Error(`Duplicate context menu item id "${id}".`);
        const children = ContextMenuBuilder.create<T>();
        configure(children);
        this.reserveId(id);
        this.definitions.push({kind: 'submenu', id, label, icon, children: [...children.definitions]});
        return this;
    }

    separator(): this {
        let id: string;
        do id = `context-menu-separator-${this.nextSeparator++}`;
        while (this.ids.has(id));
        this.reserveId(id);
        this.definitions.push({kind: 'separator', id});
        return this;
    }

    build(context: T): readonly ContextMenuItem[] {
        return this.resolve(this.definitions, context);
    }

    private reserveId(id: string): void {
        if (!id.trim()) throw new Error('A context menu item requires an id.');
        if (this.ids.has(id)) throw new Error(`Duplicate context menu item id "${id}".`);
        this.ids.add(id);
    }

    private resolve(definitions: readonly MenuDefinition<T>[], context: T): readonly ContextMenuItem[] {
        const items: ContextMenuItem[] = [];
        for (const definition of definitions) {
            switch (definition.kind) {
                case 'action': {
                    const action = definition.action;
                    if (action.visible?.(context) === false) continue;
                    items.push(Object.freeze({
                        kind: 'action', id: action.id, label: action.label,
                        icon: action.icon, shortcut: action.shortcut, destructive: action.destructive ?? false,
                        disabled: action.enabled?.(context) === false,
                        execute: () => {
                            // State may change while a menu is open; retain the clicked context but recheck its guards.
                            if (action.visible?.(context) === false || action.enabled?.(context) === false) return;
                            return action.execute(context);
                        },
                    }));
                    break;
                }
                case 'submenu': {
                    const children = this.resolve(definition.children, context);
                    if (children.length) items.push(Object.freeze({
                        kind: 'submenu', id: definition.id, label: definition.label, icon: definition.icon, children,
                    }));
                    break;
                }
                case 'separator':
                    if (items.length && items.at(-1)?.kind !== 'separator') items.push(Object.freeze({...definition}));
                    break;
            }
        }
        if (items.at(-1)?.kind === 'separator') items.pop();
        return Object.freeze(items);
    }
}
