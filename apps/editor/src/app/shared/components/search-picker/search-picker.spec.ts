import {TestBed} from '@angular/core/testing';
import axe from 'axe-core';
import {vi} from 'vitest';
import {EditorSearchPicker, type SearchPickerOption} from './search-picker';

const options: readonly SearchPickerOption[] = [
    {id: 'upper', label: 'Gate', description: 'Upstairs'},
    {id: 'lower', label: 'Gate', description: 'Basement'},
    {id: 'blocked', label: 'Gate', description: 'Courtyard', disabled: true},
];

describe('EditorSearchPicker', () => {
    async function setup(value: string | null, accept = true) {
        const fixture = TestBed.createComponent(EditorSearchPicker);
        fixture.componentRef.setInput('label', 'Choose target');
        fixture.componentRef.setInput('options', options);
        fixture.componentRef.setInput('value', value);
        fixture.componentRef.setInput('clearable', true);
        const picked: string[] = [];
        fixture.componentInstance.picked.subscribe(id => {
            picked.push(id);
            if (accept) fixture.componentRef.setInput('value', id);
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

    it('keeps keyboard selection within the visible limit and searches all choices', async () => {
        const {fixture, host, input, type, key, picked} = await setup('upper');
        fixture.componentRef.setInput('resultLimit', 1);
        await type('gate');
        expect(host.querySelectorAll('[role="option"]')).toHaveLength(1);
        expect(host.textContent).toContain('Refine your search');
        expect(input.hasAttribute('title')).toBe(false);
        await key('ArrowDown');
        expect(document.getElementById(input.getAttribute('aria-activedescendant')!)?.textContent).toContain('Upstairs');

        await type('basement');
        expect(host.querySelectorAll('[role="option"]')).toHaveLength(1);
        expect(host.textContent).not.toContain('Refine your search');
        await key('Enter');
        expect(picked).toEqual(['lower']);
        expect(input.title).toBe('Gate');
    });

    it('reveals a clipped active result in its own pane and retains focus on the input', async () => {
        const {fixture, host, input, type} = await setup(null);
        fixture.componentRef.setInput('revealResults', true);
        const pane = document.createElement('section');
        pane.style.overflowY = 'auto';
        host.before(pane);
        pane.append(host);
        Object.defineProperties(pane, {clientHeight: {value: 100}, scrollHeight: {value: 220}});
        const bounds = vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect');
        bounds.mockImplementation(function (this: HTMLElement) {
            if (this === pane) return new DOMRect(0, 0, 160, 100);
            if (this === input) return new DOMRect(0, 74 - pane.scrollTop, 160, 24);
            if (this.matches('[role="listbox"]')) return new DOMRect(0, 102 - pane.scrollTop, 160, 118);
            return new DOMRect(0, 104 - pane.scrollTop, 160, 52);
        });
        try {
            input.focus();
            await type('upstairs');
            expect(pane.scrollTop).toBe(56);
            expect(document.activeElement).toBe(input);
            expect(input.getBoundingClientRect().top).toBeGreaterThan(0);
        } finally {
            bounds.mockRestore();
            pane.before(host);
            pane.remove();
        }
    });

    it('caps rendered results while finding the last entry in a full-sized scene catalog', async () => {
        const {fixture, host, type, key, picked} = await setup(null);
        const destinations: SearchPickerOption[] = Array.from({length: 100_000}, (_, index) => ({
            id: `entry-${index}`, label: index === 99_999 ? 'Last Knight Anchor' : `Scene object ${index}`,
        }));
        fixture.componentRef.setInput('options', destinations);
        fixture.componentRef.setInput('resultLimit', 60);
        fixture.componentRef.setInput('expanded', true);
        await fixture.whenStable();
        expect(host.querySelectorAll('[role="option"]')).toHaveLength(60);

        await type('last knight');
        expect(host.querySelectorAll('[role="option"]')).toHaveLength(1);
        await key('Enter');
        expect(picked).toEqual(['entry-99999']);
    });

    it('keeps the confirmed value when its owner cannot apply the selection', async () => {
        const {input, picked, type, key, host} = await setup('upper', false);
        await type('basement');
        await key('Enter');
        expect(picked).toEqual(['lower']);
        expect(input.value).toBe('Gate');
        expect(host.querySelector('.detail')?.textContent).toContain('Upstairs');
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
