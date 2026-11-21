import {signal} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import type {EditorActionId} from '../../../../core/editor/editor-actions';
import {LAYOUT_PREFERENCES_STORAGE} from '../../../../shared/services/layout-preferences-storage';
import {CommandPalette} from './command-palette';

describe('Command Palette', () => {
    const dialogPrototype = HTMLDialogElement.prototype;
    const showModal = Object.getOwnPropertyDescriptor(dialogPrototype, 'showModal');
    const close = Object.getOwnPropertyDescriptor(dialogPrototype, 'close');

    beforeAll(() => {
        // Dispatch close separately so the test can change availability during dismissal.
        Object.defineProperty(dialogPrototype, 'showModal', {
            configurable: true,
            value(this: HTMLDialogElement) {
                this.open = true;
                this.querySelector<HTMLElement>('[autofocus]')?.focus();
            },
        });
        Object.defineProperty(dialogPrototype, 'close', {
            configurable: true,
            value(this: HTMLDialogElement) {
                this.open = false;
            },
        });
    });

    afterAll(() => {
        if (showModal) Object.defineProperty(dialogPrototype, 'showModal', showModal);
        else Reflect.deleteProperty(dialogPrototype, 'showModal');
        if (close) Object.defineProperty(dialogPrototype, 'close', close);
        else Reflect.deleteProperty(dialogPrototype, 'close');
    });

    async function setup() {
        const available = signal<readonly EditorActionId[]>(['document.save']);
        TestBed.configureTestingModule({
            providers: [{
                provide: LAYOUT_PREFERENCES_STORAGE,
                useValue: {load: async () => null, save: async () => undefined},
            }]
        });
        const fixture = TestBed.createComponent(CommandPalette);
        const root: HTMLElement = fixture.nativeElement;
        const trigger = root.appendChild(document.createElement('button'));
        trigger.focus();
        fixture.componentRef.setInput('canRun', (id: EditorActionId) => available().includes(id));
        fixture.componentRef.setInput('open', true);
        const executed: EditorActionId[] = [];
        fixture.componentInstance.commandRequested.subscribe(id => executed.push(id));
        await fixture.whenStable();
        const field = root.querySelector<HTMLInputElement>('[role="combobox"]')!;
        const dialog = root.querySelector('dialog')!;
        const search = async (text: string) => {
            field.value = text;
            field.dispatchEvent(new Event('input', {bubbles: true}));
            await fixture.whenStable();
        };
        const enter = async () => {
            field.dispatchEvent(new KeyboardEvent('keydown', {key: 'Enter', bubbles: true, cancelable: true}));
            await fixture.whenStable();
        };
        return {root, trigger, field, dialog, search, enter, executed, available};
    }

    it('finds registry commands and invokes an enabled result once after closing', async () => {
        const {root, trigger, field, dialog, search, enter, executed} = await setup();
        expect(document.activeElement).toBe(field);
        await search('duplicate');
        const disabled = root.querySelector('[role="option"]');
        expect(disabled?.textContent).toContain('Duplicate selection');
        expect(disabled?.getAttribute('aria-disabled')).toBe('true');
        await enter();
        expect(dialog.open).toBe(true);
        await search('save scene');
        expect(root.querySelector('[role="option"]')?.textContent).toContain('Ctrl/Cmd + S');
        await enter();
        expect(dialog.open).toBe(false);
        expect(executed).toEqual([]);
        dialog.dispatchEvent(new Event('close'));
        dialog.dispatchEvent(new Event('close'));
        expect(document.activeElement).toBe(trigger);
        expect(executed).toEqual(['document.save']);
    });

    it('drops a pending command if it becomes unavailable while the dialog closes', async () => {
        const {dialog, search, enter, executed, available} = await setup();
        await search('save scene');
        await enter();
        available.set([]);
        dialog.dispatchEvent(new Event('close'));
        expect(executed).toEqual([]);
    });
});
