import {ContextActionBuilder} from '../actions/context-action';
import {ContextMenuBuilder, type ContextMenuItem} from './context-menu-builder';

interface ClickedItem {
    readonly key: string;
    visible: boolean;
    editable: boolean;
}

function asAction(item: ContextMenuItem): Extract<ContextMenuItem, { kind: 'action' }> {
    if (item.kind !== 'action') throw new Error('Expected an action.');
    return item;
}

describe('ContextMenuBuilder', () => {
    it('resolves a contextual tree and cleans separators after pruning unavailable branches', () => {
        const visibleAction = ContextActionBuilder.create<ClickedItem>('select', 'Select')
            .icon('object').shortcut('Enter').run(() => undefined).build();
        const hiddenAction = ContextActionBuilder.create<ClickedItem>('rename', 'Rename')
            .when(context => context.visible).run(() => undefined).build();
        const deleteAction = ContextActionBuilder.create<ClickedItem>('delete', 'Delete')
            .enabledWhen(context => context.editable).destructive().run(() => undefined).build();
        const builder = ContextMenuBuilder.create<ClickedItem>()
            .separator().separator().action(visibleAction).separator()
            .submenu('hidden', 'Hidden', branch => branch.separator().action(hiddenAction).separator())
            .separator()
            .submenu('edit', 'Edit', branch => branch.separator().action(deleteAction).separator(), 'folder')
            .separator();

        const menu = builder.build({key: 'locked', visible: false, editable: false});

        expect(menu.map(item => item.kind)).toEqual(['action', 'separator', 'submenu']);
        expect(menu[0]).toMatchObject({
            id: 'select',
            icon: 'object',
            shortcut: 'Enter',
            disabled: false,
            destructive: false
        });
        expect(menu[2]).toMatchObject({
            id: 'edit', label: 'Edit', icon: 'folder', children: [{id: 'delete', disabled: true, destructive: true}],
        });
    });

    it('retains the clicked item and rechecks visibility and availability before dispatch', async () => {
        const execute = vi.fn(async (context: ClickedItem) => context.key);
        const action = ContextActionBuilder.create<ClickedItem>('delete', 'Delete')
            .when(context => context.visible).enabledWhen(context => context.editable).run(execute).build();
        const clicked = {key: 'entity-a', visible: true, editable: true};
        const builder = ContextMenuBuilder.create<ClickedItem>().action(action);
        const resolved = asAction(builder.build(clicked)[0]);

        clicked.editable = false;
        expect(resolved.execute()).toBeUndefined();
        clicked.editable = true;
        clicked.visible = false;
        expect(resolved.execute()).toBeUndefined();
        expect(execute).not.toHaveBeenCalled();
        clicked.visible = true;
        await expect(resolved.execute()).resolves.toBe('entity-a');
        expect(execute).toHaveBeenCalledExactlyOnceWith(clicked);
    });

    it('isolates built menus across contexts, later builder changes, and retained submenu builders', async () => {
        const execute = vi.fn();
        const actionBuilder = ContextActionBuilder.create<ClickedItem>('open', 'Open')
            .enabledWhen(context => context.editable).run(execute);
        let retainedBranch: ContextMenuBuilder<ClickedItem> | undefined;
        const builder = ContextMenuBuilder.create<ClickedItem>()
            .action(actionBuilder.build())
            .submenu('more', 'More', branch => {
                retainedBranch = branch;
                branch.action(actionBuilder.build());
            });
        const firstContext = {key: 'first', visible: true, editable: true};
        const first = builder.build(firstContext);
        actionBuilder.icon('folder').run(() => undefined);
        retainedBranch!.action(ContextActionBuilder.create<ClickedItem>('later-child', 'Later child').run(() => undefined).build());
        builder.action(ContextActionBuilder.create<ClickedItem>('later', 'Later').run(() => undefined).build());
        const second = builder.build({key: 'second', visible: true, editable: false});

        expect(first.map(item => item.id)).toEqual(['open', 'more']);
        expect(second.map(item => item.id)).toEqual(['open', 'more', 'later']);
        expect(asAction(first[0]).disabled).toBe(false);
        expect(asAction(second[0]).disabled).toBe(true);
        expect(first[1]).toMatchObject({children: [{id: 'open'}]});
        expect(second[1]).toMatchObject({children: [{id: 'open'}]});
        await asAction(first[0]).execute();
        expect(execute).toHaveBeenCalledExactlyOnceWith(firstContext);
    });

    it('rejects ambiguous sibling ids, including hidden actions, while allowing ids in separate branches', () => {
        const hidden = ContextActionBuilder.create<ClickedItem>('open', 'Open').when(() => false).run(() => undefined).build();
        const builder = ContextMenuBuilder.create<ClickedItem>().action(hidden);

        expect(() => builder.action(hidden)).toThrow('Duplicate context menu item id "open"');
        expect(() => builder.submenu('open', 'Open', () => undefined)).toThrow('Duplicate context menu item id "open"');
        expect(() => builder.submenu('more', 'More', branch => branch.action(hidden).action(hidden)))
            .toThrow('Duplicate context menu item id "open"');
        expect(() => builder.submenu('more', 'More', branch => branch.action(hidden))).not.toThrow();
        expect(builder.build({key: 'a', visible: true, editable: true})).toEqual([]);
    });
});
