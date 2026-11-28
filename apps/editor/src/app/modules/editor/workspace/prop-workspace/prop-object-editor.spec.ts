import {TestBed} from '@angular/core/testing';
import {PropObjectEditor, type PropObjectEdit} from './prop-object-editor';
import type {PropWorkspaceObject} from './prop-workspace.service';

function object(key: string, name: string, parent: string | null = null): PropWorkspaceObject {
    return {
        key, name, parent, components: [], level: parent === null ? 0 : 1,
        transform: {position: [0, 0, 0], rotation: [0, 0, 0], scale: [1, 1, 1]}
    };
}

describe('PropObjectEditor parent destination', () => {
    it('keeps roots and descendants distinct when searching by a long Prop object name', async () => {
        const fixture = TestBed.createComponent(PropObjectEditor);
        const root = fixture.nativeElement as HTMLElement;
        const selected = object('child', 'Drawbridge');
        const anchor = object('parent',
            'Long oak tower support beam finishing the curved upper balcony behind the gate');
        fixture.componentRef.setInput('object', selected);
        fixture.componentRef.setInput('objects', [selected, object('blocked', 'Forbidden descendant', 'child'), anchor]);
        const edits: PropObjectEdit[] = [];
        fixture.componentInstance.edited.subscribe(edit => edits.push(edit));
        await fixture.whenStable();

        const input = root.querySelector<HTMLInputElement>('input[role="combobox"]')!;
        const search = async (query: string) => {
            input.value = query;
            input.dispatchEvent(new Event('input', {bubbles: true}));
            await fixture.whenStable();
        };
        const choose = async () => {
            input.dispatchEvent(new KeyboardEvent('keydown', {key: 'Enter', bubbles: true, cancelable: true}));
            await fixture.whenStable();
        };

        expect(input.value).toBe('Prop root');
        await search('forbidden');
        expect(root.querySelectorAll('[role="option"]')).toHaveLength(0);
        await choose();
        expect(edits).toEqual([]);

        await search('balcony behind the gate');
        await choose();
        expect(edits).toEqual([{key: 'child', parent: 'parent'}]);

        fixture.componentRef.setInput('object', {...selected, parent: 'parent'});
        fixture.componentRef.setInput('requireParent', true);
        await fixture.whenStable();
        expect(input.title).toBe(anchor.name);
        await search('Prop root');
        expect(root.querySelectorAll('[role="option"]')).toHaveLength(0);
        await choose();
        expect(edits).toHaveLength(1);
    });
});
