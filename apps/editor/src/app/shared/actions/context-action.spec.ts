import {ContextActionBuilder} from './context-action';

describe('ContextActionBuilder', () => {
    it('requires an executable intent and keeps earlier actions independent of builder reuse', async () => {
        const firstRun = vi.fn(async (context: { key: string }) => context.key);
        const secondRun = vi.fn();
        const builder = ContextActionBuilder.create<{ key: string }>('entity.delete', 'Delete')
            .icon('minus').shortcut('Delete').destructive();

        expect(() => builder.build()).toThrow('requires a run callback');
        const first = builder.run(firstRun).build();
        const second = builder.destructive(false).icon('object').run(secondRun).build();
        const clicked = {key: 'clicked-entity'};

        expect(first).toMatchObject({
            id: 'entity.delete', label: 'Delete', icon: 'minus', shortcut: 'Delete', destructive: true,
        });
        expect(second).toMatchObject({icon: 'object', destructive: false});
        await expect(first.execute(clicked)).resolves.toBe('clicked-entity');
        expect(firstRun).toHaveBeenCalledWith(clicked);
        expect(secondRun).not.toHaveBeenCalled();
    });

    it('retains context predicates for the menu to evaluate when it opens', () => {
        const action = ContextActionBuilder.create<{ visible: boolean; editable: boolean }>('rename', 'Rename')
            .when(context => context.visible)
            .enabledWhen(context => context.editable)
            .run(() => undefined)
            .build();

        expect(action.visible?.({visible: false, editable: true})).toBe(false);
        expect(action.enabled?.({visible: true, editable: false})).toBe(false);
    });
});
