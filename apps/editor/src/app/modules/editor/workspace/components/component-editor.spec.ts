import {TestBed} from '@angular/core/testing';
import {ComponentEditor, type ComponentEditIntent} from './component-editor';
import {
    validComponentRecord,
    type ComponentTypeDescriptor,
} from '../../../../core/editor/editor-components';

const interactable: ComponentTypeDescriptor = {
    type_id: 'engine.interactable', kind: 'interactable', label: 'Interactable',
    description: 'Game action and reach.', max_per_object: 1,
};

describe('Interactable Inspector', () => {
    async function setup() {
        const fixture = TestBed.createComponent(ComponentEditor);
        const host: HTMLElement = fixture.nativeElement;
        const intents: ComponentEditIntent[] = [];
        fixture.componentInstance.actionRequested.subscribe(intent => intents.push(intent));
        fixture.componentRef.setInput('types', [interactable]);
        fixture.componentRef.setInput('editable', true);
        fixture.componentRef.setInput('ownerKey', 'lever');
        fixture.componentRef.setInput('contextKey', 'main:lever');
        fixture.componentRef.setInput('gameActions', ['interact', 'inspect']);
        await fixture.whenStable();
        const button = (name: string) => Array.from(host.querySelectorAll('button'))
            .find(button => button.textContent.trim() === name)!;
        const field = (name: string) => Array.from(host.querySelectorAll('label'))
            .find(label => label.textContent.trim().startsWith(name))!.querySelector('input')!;
        const set = async (name: string, value: string) => {
            const input = field(name);
            input.value = value;
            input.dispatchEvent(new Event('input', {bubbles: true}));
            await fixture.whenStable();
        };
        return {fixture, host, intents, button, field, set};
    }

    it('submits one typed record with the authored action and reach', async () => {
        const panel = await setup();
        panel.button('Add Component').click();
        await panel.fixture.whenStable();
        expect(Array.from(panel.host.querySelectorAll('datalist option'), option => option.getAttribute('value')))
            .toEqual(['interact', 'inspect']);
        await panel.set('Game action ID', 'inspect');
        await panel.set('Reach (metres)', '1.25');
        panel.button('Apply Component').click();
        await panel.fixture.whenStable();
        expect(panel.intents).toEqual([expect.objectContaining({
            kind: 'add', expected: null, component: expect.objectContaining({
                schema_version: 1,
                enabled: true,
                data: {kind: 'interactable', settings: {action: 'inspect', reach: 1.25}},
            }),
        })]);
    });

    it('keeps invalid input in a draft until selection changes', async () => {
        const panel = await setup();
        panel.button('Add Component').click();
        await panel.fixture.whenStable();
        await panel.set('Reach (metres)', '3.5');
        expect(panel.button('Apply Component').disabled).toBe(true);
        expect(panel.host.textContent).toContain('Reach must be positive');
        panel.button('Discard Component Draft').click();
        await panel.fixture.whenStable();
        expect(panel.host.querySelector('.component-form')).toBeNull();
        panel.button('Add Component').click();
        await panel.fixture.whenStable();
        panel.fixture.componentRef.setInput('contextKey', 'main:other');
        await panel.fixture.whenStable();
        expect(panel.host.querySelector('.component-form')).toBeNull();
        expect(panel.intents).toEqual([]);
    });

    it('shows the prefab source value and excludes native instance edit intents', async () => {
        const panel = await setup();
        const component = {
            id: 'interaction', schema_version: 1, enabled: true,
            data: {kind: 'interactable', settings: {action: 'interact', reach: 1.25}}
        };
        expect(validComponentRecord(component)).toBe(true);
        expect(validComponentRecord({
            ...component, data: {
                ...component.data,
                settings: {...component.data.settings, reach: 1e-100}
            }
        })).toBe(false);
        panel.fixture.componentRef.setInput('components', [component]);
        panel.fixture.componentRef.setInput('interactablePinned', true);
        await panel.fixture.whenStable();
        expect(panel.host.textContent).toContain('1.25 m');
        expect(panel.host.textContent).toContain('Edit the prefab source');
        expect(panel.host.querySelector('[aria-label="Edit Interactable"]')).toBeNull();
        expect(panel.button('Add Component').disabled).toBe(true);
    });
});
