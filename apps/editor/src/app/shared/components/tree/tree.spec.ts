import {TestBed} from '@angular/core/testing';
import axe from 'axe-core';
import {EditorTree, type EditorTreeNode} from './tree';

const settings: readonly EditorTreeNode[] = [
    {
        id: 'editor', label: 'Editor', children: [
            {id: 'keymap', label: 'Keymap'},
            {id: 'display', label: 'Display'},
        ]
    },
    {id: 'shortcuts', label: 'Shortcuts'},
];

function renderTree() {
    const fixture = TestBed.createComponent(EditorTree);
    fixture.componentRef.setInput('nodes', settings);
    fixture.componentRef.setInput('label', 'Settings');
    fixture.componentRef.setInput('selectedId', 'keymap');
    fixture.detectChanges();
    const host: HTMLElement = fixture.nativeElement;
    const tree = host.querySelector<HTMLElement>('[role="tree"]')!;
    const row = (label: string) => tree.querySelector<HTMLElement>(`[role="treeitem"][aria-label="${label}"]`)!;
    const active = () => host.ownerDocument.getElementById(tree.getAttribute('aria-activedescendant') ?? '');
    const key = (value: string) => {
        const event = new KeyboardEvent('keydown', {key: value, cancelable: true, bubbles: true});
        tree.dispatchEvent(event);
        fixture.detectChanges();
        return event;
    };
    return {fixture, host, tree, row, active, key};
}

describe('EditorTree', () => {
    it('navigates visible rows and selects only when activated', () => {
        const {fixture, tree, row, active, key} = renderTree();
        const selections: (string | null)[] = [];
        fixture.componentInstance.selectedId.subscribe(id => selections.push(id));
        expect(tree.getAttribute('aria-label')).toBe('Settings');
        expect(active()).toBe(row('Keymap'));
        expect(row('Keymap').getAttribute('aria-level')).toBe('2');

        expect(key('ArrowLeft').defaultPrevented).toBe(true);
        expect(active()).toBe(row('Editor'));
        key('ArrowLeft');
        expect(row('Editor').getAttribute('aria-expanded')).toBe('false');
        expect(row('Keymap')).toBeNull();
        key('ArrowDown');
        expect(active()).toBe(row('Shortcuts'));
        expect(selections).toEqual([]);
        key('Enter');
        expect(row('Shortcuts').getAttribute('aria-selected')).toBe('true');
        expect(selections).toEqual(['shortcuts']);

        key('Home');
        key('ArrowRight');
        expect(row('Editor').getAttribute('aria-expanded')).toBe('true');
        key('ArrowRight');
        expect(active()).toBe(row('Keymap'));
        key(' ');
        expect(selections).toEqual(['shortcuts', 'keymap']);
        key('End');
        expect(active()).toBe(row('Shortcuts'));
        expect(key('Escape').defaultPrevented).toBe(false);
    });

    it('keeps disclosure and selection independent and has accessible tree markup', async () => {
        const {fixture, host, tree, row, active} = renderTree();
        row('Editor').querySelector<HTMLElement>('.disclosure')!.click();
        fixture.detectChanges();
        expect(active()).toBe(row('Editor'));
        expect(row('Editor').getAttribute('aria-expanded')).toBe('false');
        expect(fixture.componentInstance.selectedId()).toBe('keymap');

        row('Shortcuts').click();
        fixture.detectChanges();
        expect(host.ownerDocument.activeElement).toBe(tree);
        expect(row('Shortcuts').getAttribute('aria-selected')).toBe('true');
        const report = await axe.run(host, {rules: {'color-contrast': {enabled: false}}});
        expect(report.violations.map(violation => violation.id)).toEqual([]);
    });
});
