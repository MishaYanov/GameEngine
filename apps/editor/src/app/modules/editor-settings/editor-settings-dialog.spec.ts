import {TestBed} from '@angular/core/testing';
import axe from 'axe-core';
import {EditorSettingsService} from '../../core/editor/settings/editor-settings.service';
import {LAYOUT_PREFERENCES_STORAGE} from '../../shared/services/layout-preferences-storage';
import {EditorSettingsDialog} from './editor-settings-dialog';

describe('Editor Settings dialog', () => {
    const prototype = HTMLDialogElement.prototype;
    const originalShow = Object.getOwnPropertyDescriptor(prototype, 'showModal');
    const originalClose = Object.getOwnPropertyDescriptor(prototype, 'close');

    beforeAll(() => {
        Object.defineProperty(prototype, 'showModal', {
            configurable: true,
            value(this: HTMLDialogElement) {
                this.setAttribute('open', '');
            },
        });
        Object.defineProperty(prototype, 'close', {
            configurable: true,
            value(this: HTMLDialogElement) {
                this.removeAttribute('open');
                this.dispatchEvent(new Event('close'));
            },
        });
    });

    afterAll(() => {
        if (originalShow) Object.defineProperty(prototype, 'showModal', originalShow);
        else Reflect.deleteProperty(prototype, 'showModal');
        if (originalClose) Object.defineProperty(prototype, 'close', originalClose);
        else Reflect.deleteProperty(prototype, 'close');
    });

    it('shows the editor-only keymap, switches pages, and clears open state on close', async () => {
        TestBed.configureTestingModule({
            providers: [{
                provide: LAYOUT_PREFERENCES_STORAGE,
                useValue: {load: async () => null, save: async () => undefined},
            }],
        });
        const settings = TestBed.inject(EditorSettingsService);
        const fixture = TestBed.createComponent(EditorSettingsDialog);
        const root: HTMLElement = fixture.nativeElement;
        expect(root.querySelector('dialog')).toBeNull();

        settings.setOpen(true);
        await fixture.whenStable();
        const dialog = root.querySelector<HTMLDialogElement>('dialog')!;
        expect(dialog.open).toBe(true);
        expect(dialog.querySelector('[role="treeitem"][aria-selected="true"]')?.textContent).toContain('Keymap');
        expect(dialog.textContent).toContain('Ctrl/Cmd + S');
        expect(dialog.textContent).toContain('Shift + backtick');
        expect(dialog.textContent).not.toContain('Game Input');
        expect(dialog.querySelector('[type="submit"]')).toBeNull();
        const audit = await axe.run(dialog, {rules: {'color-contrast': {enabled: false}}});
        expect(audit.violations.map(violation => violation.id)).toEqual([]);

        dialog.querySelector<HTMLElement>('[role="treeitem"]')!.click();
        await fixture.whenStable();
        expect(settings.state().page).toBe('editor');
        expect(dialog.textContent).toContain('Settings for the editor application.');

        dialog.dispatchEvent(new Event('cancel', {cancelable: true}));
        await fixture.whenStable();
        expect(dialog.open).toBe(false);
        expect(settings.state()).toEqual({open: false, page: 'editor'});
    });
});
