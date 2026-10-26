import {Component, signal} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import {By} from '@angular/platform-browser';
import {form, required} from '@angular/forms/signals';
import {vi} from 'vitest';
import axe from 'axe-core';
import {LAYOUT_PREFERENCES_STORAGE} from '../../services/layout-preferences-storage';
import {LayoutPreferencesService} from '../../services/layout-preferences.service';
import {defaultTransform, SUBJECT_DEFINITIONS} from '../../../core/subjects/subject.models';
import {EditorDialog} from './dialog';
import {dialogSettings, DialogSubject} from './dialog-settings';

@Component({
    imports: [EditorDialog],
    template: `<editor-dialog
		title="Properties"
		preferenceKey="test:dialog"
		[(open)]="open"
		[form]="fields"
		[subjects]="subjects"
		(submitted)="applied.set(applied() + 1)"
	/>`,
})
class DialogHost {
    readonly open = signal(true);
    readonly applied = signal(0);
    readonly data = signal({
        object: {name: 'Cube', visible: true},
        transform: defaultTransform(),
    });
    readonly fields = form(this.data, (path) =>
        required(path.object.name, {message: 'Enter a name.'}),
    );
    readonly subjects: readonly DialogSubject[] = [
        {
            id: 'object',
            title: 'Object',
            form: this.fields.object,
            inputs: SUBJECT_DEFINITIONS.object.inputs,
        },
        {
            id: 'transform',
            title: 'Transform',
            form: this.fields.transform,
            inputs: SUBJECT_DEFINITIONS.transform.inputs,
        },
    ];
}

describe('Dialog', () => {
    const load = vi.fn<() => Promise<unknown>>();
    const save = vi.fn().mockResolvedValue(undefined);
    const prototype = HTMLDialogElement.prototype;
    const originalShow = Object.getOwnPropertyDescriptor(prototype, 'showModal');
    const originalClose = Object.getOwnPropertyDescriptor(prototype, 'close');
    beforeAll(() => {
        // JSDOM does not implement the native modal lifecycle.
        Object.defineProperty(prototype, 'showModal', {
            configurable: true,
            value: function (this: HTMLDialogElement) {
                this.setAttribute('open', '');
            },
        });
        Object.defineProperty(prototype, 'close', {
            configurable: true,
            value: function (this: HTMLDialogElement) {
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
    beforeEach(() => {
        load.mockReset().mockResolvedValue(null);
        TestBed.configureTestingModule({
            providers: [{provide: LAYOUT_PREFERENCES_STORAGE, useValue: {load, save}}],
        });
    });

    async function setup() {
        const fixture = TestBed.createComponent(DialogHost);
        fixture.detectChanges();
        await fixture.whenStable();
        const component: EditorDialog = fixture.debugElement.query(
            By.directive(EditorDialog),
        ).componentInstance;
        await vi.waitFor(() => expect(component.configReady()).toBe(true));
        fixture.detectChanges();
        const element: HTMLElement = fixture.nativeElement;
        const dialog = element.querySelector('dialog')!;
        const button = (name: string) =>
            element.querySelector<HTMLButtonElement>(`[aria-label="${name}"]`)!;
        return {fixture, component, element, dialog, button, host: fixture.componentInstance};
    }

    it('opens a modal with multiple subjects bound to the same Signal Form', async () => {
        const {fixture, dialog, element, host} = await setup();
        expect(dialog.open).toBe(true);
        expect(element.querySelectorAll('editor-subject')).toHaveLength(2);
        const input = element.querySelector<HTMLInputElement>('[aria-label="Transform Position X"]')!;
        input.value = '7';
        input.dispatchEvent(new Event('input'));
        fixture.detectChanges();
        expect(host.data().transform.position.x).toBe(7);
    });

    it('defers closed dialog DOM until first use and listens for viewport resizing only while open', async () => {
        const add = vi.spyOn(window, 'addEventListener');
        const remove = vi.spyOn(window, 'removeEventListener');
        const fixture = TestBed.createComponent(DialogHost);
        fixture.componentInstance.open.set(false);
        fixture.detectChanges();
        await fixture.whenStable();
        expect(fixture.nativeElement.querySelector('dialog')).toBeNull();
        expect(add.mock.calls.filter(([event]) => event === 'resize')).toHaveLength(0);
        fixture.componentInstance.open.set(true);
        fixture.detectChanges();
        await fixture.whenStable();
        const dialog = fixture.nativeElement.querySelector('dialog');
        expect(dialog.open).toBe(true);
        expect(add.mock.calls.filter(([event]) => event === 'resize')).toHaveLength(1);
        fixture.componentInstance.open.set(false);
        fixture.detectChanges();
        await fixture.whenStable();
        expect(fixture.nativeElement.querySelector('dialog')).toBe(dialog);
        expect(remove.mock.calls.filter(([event]) => event === 'resize')).toHaveLength(1);
    });

    it('coalesces corner resizing, preserves the top-left anchor and saves its dimensions', async () => {
        const {fixture, component, dialog, button} = await setup();
        let frame!: FrameRequestCallback;
        vi.spyOn(window, 'requestAnimationFrame').mockImplementation((callback) => {
            frame = callback;
            return 77;
        });
        vi.spyOn(document.documentElement, 'clientWidth', 'get').mockReturnValue(1000);
        vi.spyOn(dialog, 'getBoundingClientRect')
            .mockReturnValueOnce(new DOMRect(100, 100, 400, 300))
            .mockReturnValueOnce(new DOMRect(50, 75, 500, 350))
            .mockReturnValue(new DOMRect(100, 100, 500, 350));
        const handle = button('Resize Properties, use arrow keys');
        handle.setPointerCapture = vi.fn();
        handle.hasPointerCapture = () => true;
        handle.releasePointerCapture = vi.fn();
        const pointer = (name: string, x: number, y: number) =>
            handle.dispatchEvent(
                Object.assign(new Event(name, {bubbles: true, cancelable: true}), {
                    pointerId: 1,
                    button: 0,
                    clientX: x,
                    clientY: y,
                }),
            );
        pointer('pointerdown', 500, 400);
        pointer('pointermove', 550, 425);
        pointer('pointermove', 600, 450);
        expect(component.config().width).not.toBe('500px');
        frame(0);
        fixture.detectChanges();
        expect(component.config()).toMatchObject({width: '500px', height: '350px', x: 50, y: 25});
        pointer('pointerup', 600, 450);
        expect(handle.releasePointerCapture).toHaveBeenCalledWith(1);
        await TestBed.inject(LayoutPreferencesService).flush();
        expect(save.mock.calls.at(-1)?.[0].components['test:dialog']).toMatchObject({
            width: '500px',
            height: '350px',
        });
    });

    it('can disable resizing and removes its grip when expanded', async () => {
        const {fixture, component, button} = await setup();
        component.config.update((config) => ({...config, resizable: false}));
        fixture.detectChanges();
        expect(button('Resize Properties, use arrow keys')).toBeNull();
        component.config.update((config) => ({...config, resizable: true}));
        fixture.detectChanges();
        expect(button('Resize Properties, use arrow keys')).not.toBeNull();
        button('Expand Properties').click();
        fixture.detectChanges();
        expect(button('Resize Properties, use arrow keys')).toBeNull();
    });

    it('validates the whole form before emitting Apply, including folded subject fields', async () => {
        const {fixture, host, element} = await setup();
        host.fields.object.name().value.set('');
        fixture.detectChanges();
        element.querySelector('form')!.dispatchEvent(new Event('submit', {cancelable: true}));
        await fixture.whenStable();
        fixture.detectChanges();
        expect(host.applied()).toBe(0);
        expect(host.fields.object.name().touched()).toBe(true);
        host.fields.object.name().value.set('Camera');
        fixture.detectChanges();
        element.querySelector('form')!.dispatchEvent(new Event('submit', {cancelable: true}));
        await fixture.whenStable();
        expect(host.applied()).toBe(1);
    });

    it('preserves form controls and edits across close and reopen', async () => {
        const {fixture, host, dialog, element, button} = await setup();
        const control = element.querySelector('[aria-label="Object Name"]');
        host.fields.object.name().value.set('Camera');
        button('Close Properties').click();
        fixture.detectChanges();
        await fixture.whenStable();
        expect(dialog.open).toBe(false);
        host.open.set(true);
        fixture.detectChanges();
        await fixture.whenStable();
        expect(dialog.open).toBe(true);
        expect(element.querySelector('[aria-label="Object Name"]')).toBe(control);
        expect(host.data().object.name).toBe('Camera');
    });

    it('restores saved sizing and expands without losing the configured size', async () => {
        load.mockResolvedValue({
            schema_version: 1,
            components: {'test:dialog': {width: '420px', height: '500px', draggable: false}},
        });
        const {fixture, component, dialog, button} = await setup();
        expect(dialog.style.getPropertyValue('--dialog-width')).toBe('420px');
        expect(button('Move Properties, use arrow keys')).toBeNull();
        button('Expand Properties').click();
        fixture.detectChanges();
        expect(dialog.classList.contains('expanded')).toBe(true);
        button('Restore Properties').click();
        fixture.detectChanges();
        expect(component.config().width).toBe('420px');
        expect(dialog.classList.contains('expanded')).toBe(false);
    });

    it('supports keyboard movement and constrains the dialog to the viewport', async () => {
        const {fixture, component, dialog, button} = await setup();
        vi.spyOn(dialog, 'getBoundingClientRect').mockImplementation(
            () =>
                new DOMRect(100 + (component.config().x ?? 0), 100 + (component.config().y ?? 0), 400, 300),
        );
        button('Move Properties, use arrow keys').dispatchEvent(
            new KeyboardEvent('keydown', {key: 'ArrowRight', shiftKey: true, bubbles: true}),
        );
        fixture.detectChanges();
        expect(component.config().x).toBe(40);
        for (let count = 0; count < 20; count++)
            button('Move Properties, use arrow keys').dispatchEvent(
                new KeyboardEvent('keydown', {key: 'ArrowUp', shiftKey: true, bubbles: true}),
            );
        expect(component.config().y).toBe(-100);
    });

    it('honors Escape and backdrop dismissal settings', async () => {
        const {fixture, component, dialog, host} = await setup();
        component.config.update((config) => ({
            ...config,
            closeOnEscape: false,
            closeOnBackdrop: true,
        }));
        fixture.detectChanges();
        const cancel = new Event('cancel', {cancelable: true});
        dialog.dispatchEvent(cancel);
        expect(cancel.defaultPrevented).toBe(true);
        expect(host.open()).toBe(true);
        vi.spyOn(dialog, 'getBoundingClientRect').mockReturnValue(new DOMRect(100, 100, 400, 300));
        dialog.dispatchEvent(new MouseEvent('click', {clientX: 120, clientY: 120}));
        expect(host.open()).toBe(true);
        dialog.dispatchEvent(new MouseEvent('click', {clientX: 20, clientY: 20}));
        fixture.detectChanges();
        await fixture.whenStable();
        expect(host.open()).toBe(false);
    });

    it('has accessible dialog and form controls', async () => {
        const {element} = await setup();
        expect(
            (await axe.run(element, {rules: {'color-contrast': {enabled: false}}})).violations.map(
                (v) => v.id,
            ),
        ).toEqual([]);
    });

    it('ignores malformed saved config fields and retains valid overrides', () => {
        expect(
            dialogSettings({
                width: 'bad',
                height: '70vh',
                draggable: 'yes',
                expandable: false,
                x: Infinity,
                y: -50,
            }),
        ).toEqual({height: '70vh', expandable: false, y: -50});
    });
});
