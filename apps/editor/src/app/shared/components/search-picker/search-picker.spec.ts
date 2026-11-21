import {TestBed} from '@angular/core/testing';
import axe from 'axe-core';
import {EditorSearchPicker, type SearchPickerOption} from './search-picker';

const options: readonly SearchPickerOption[] = [
    {id: 'upper', label: 'Gate', description: 'Upstairs'},
    {id: 'lower', label: 'Gate', description: 'Basement'},
    {id: 'blocked', label: 'Gate', description: 'Courtyard', disabled: true},
];

describe('EditorSearchPicker', () => {
    async function setup(value: string | null) {
        const fixture = TestBed.createComponent(EditorSearchPicker);
        fixture.componentRef.setInput('label', 'Choose target');
        fixture.componentRef.setInput('options', options);
        fixture.componentRef.setInput('value', value);
        fixture.componentRef.setInput('clearable', true);
        const picked: string[] = [];
        fixture.componentInstance.picked.subscribe(id => {
            picked.push(id);
            fixture.componentRef.setInput('value', id);
        });
        await fixture.whenStable();
        const host: HTMLElement = fixture.nativeElement;
        const input = host.querySelector<HTMLInputElement>('[role="combobox"]')!;

        async function type(text: string) {
            input.value = text;
            input.dispatchEvent(new Event('input'));
            await fixture.whenStable();
        }

        async function key(name: string) {
            const event = new KeyboardEvent('keydown', {key: name, bubbles: true, cancelable: true});
            input.dispatchEvent(event);
            await fixture.whenStable();
            return event;
        }

        return {fixture, host, input, picked, type, key};
    }

    it('resolves duplicate labels by searching hierarchy details and selects the stable ID', async () => {
        const {host, input, picked, type, key} = await setup('upper');
        await type('basement gate');
        expect(host.querySelectorAll('[role="option"]')).toHaveLength(1);
        expect(host.querySelector('[role="option"]')?.textContent).toContain('Basement');
        const report = await axe.run(host, {rules: {'color-contrast': {enabled: false}}});
        expect(report.violations.map(violation => violation.id)).toEqual([]);
        await key('Enter');
        expect(picked).toEqual(['lower']);
        expect(input.value).toBe('Gate');
        expect(input.getAttribute('aria-expanded')).toBe('false');
        expect(host.textContent).toContain('Basement');
    });

    it('skips disabled entries, preserves a missing value on cancel, and clears only on request', async () => {
        const {fixture, host, input, picked, type, key} = await setup('missing-id');
        expect(host.textContent).toContain('Missing target: missing-id');
        expect(input.getAttribute('aria-invalid')).toBe('true');
        await type('Gate');
        await key('ArrowUp');
        expect(document.getElementById(input.getAttribute('aria-activedescendant')!)?.textContent).toContain('Basement');
        await key('Escape');
        expect(picked).toEqual([]);
        expect(input.value).toBe('missing-id');
        host.querySelector<HTMLButtonElement>('[aria-label="Clear Choose target"]')!.click();
        await fixture.whenStable();
        expect(picked).toEqual(['']);
        expect(input.value).toBe('');
        expect(input.hasAttribute('aria-invalid')).toBe(false);
    });
});
