import {TestBed} from '@angular/core/testing';
import axe from 'axe-core';
import {vi} from 'vitest';
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
        const choice = (label: string) => [...dialog.querySelectorAll<HTMLElement>('[role="treeitem"]')]
            .find(item => item.getAttribute('aria-label') === label)!;
        expect(choice('Appearance').getAttribute('aria-selected')).toBe('true');
        choice('Keymap').click();
        await fixture.whenStable();
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
        expect(settings.sceneWarmingMode()).toBe('off');
        expect(dialog.querySelector('#editor-scene-warming-mode')).toBeNull();

        dialog.dispatchEvent(new Event('cancel', {cancelable: true}));
        await fixture.whenStable();
        expect(dialog.open).toBe(false);
        expect(settings.state()).toEqual({open: false, page: 'editor'});
    });

    it('offers both themes when Settings opens and applies the selected theme', async () => {
        TestBed.configureTestingModule({
            providers: [{
                provide: LAYOUT_PREFERENCES_STORAGE,
                useValue: {load: async () => null, save: async () => undefined},
            }],
        });
        const host = document.documentElement;
        const previousTheme = host.getAttribute('data-theme');
        const storageKey = 'game-engine.editor.theme';
        const previousChoice = localStorage.getItem(storageKey);
        localStorage.removeItem(storageKey);
        try {
            const settings = TestBed.inject(EditorSettingsService);
            const fixture = TestBed.createComponent(EditorSettingsDialog);
            settings.setOpen(true);
            await fixture.whenStable();
            const root: HTMLElement = fixture.nativeElement;
            const select = root.querySelector<HTMLSelectElement>('editor-theme-picker select')!;
            expect([...select.options].map(option => option.textContent?.trim())).toEqual(['Workbench', 'Graphite (Classic)']);
            expect(select.value).toBe('workbench');
            select.value = 'graphite';
            select.dispatchEvent(new Event('change'));
            expect(host.dataset['theme']).toBe('graphite');
            expect(localStorage.getItem(storageKey)).toBe('graphite');
        } finally {
            if (previousTheme === null) host.removeAttribute('data-theme');
            else host.setAttribute('data-theme', previousTheme);
            if (previousChoice === null) localStorage.removeItem(storageKey);
            else localStorage.setItem(storageKey, previousChoice);
        }
    });

    it('restores and saves the warming mode through local editor preferences', async () => {
        const load = vi.fn(async () => ({
            schema_version: 1,
            components: {'editor:scene-warming': {mode: 'references'}},
        }));
        const save = vi.fn(async () => undefined);
        TestBed.configureTestingModule({
            providers: [{provide: LAYOUT_PREFERENCES_STORAGE, useValue: {load, save}}],
        });
        const fixture = TestBed.createComponent(EditorSettingsDialog);
        fixture.componentRef.setInput('sceneWarmingSupported', true);
        const settings = TestBed.inject(EditorSettingsService);
        settings.select('editor');
        settings.setOpen(true);
        await fixture.whenStable();

        const root: HTMLElement = fixture.nativeElement;
        const select = root.querySelector<HTMLSelectElement>('#editor-scene-warming-mode')!;
        expect([...select.options].map(option => option.textContent?.trim())).toEqual(['Off', 'References', 'Models']);
        expect(select.value).toBe('references');
        select.value = 'models';
        select.dispatchEvent(new Event('change'));
        await fixture.whenStable();
        expect(settings.sceneWarmingMode()).toBe('models');
        expect(save).toHaveBeenLastCalledWith(expect.objectContaining({
            components: expect.objectContaining({'editor:scene-warming': {mode: 'models'}}),
        }));
        const audit = await axe.run(root.querySelector('dialog')!, {rules: {'color-contrast': {enabled: false}}});
        expect(audit.violations.map(violation => violation.id)).toEqual([]);
    });
});
