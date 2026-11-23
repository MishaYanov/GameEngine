import {computed, signal} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import {vi} from 'vitest';
import axe from 'axe-core';
import type {EditorCommand, EditorSnapshot, ProjectData} from '../../../../core/editor/editor.models';
import {EditorService} from '../../../../core/editor/editor.service';
import {LAYOUT_PREFERENCES_STORAGE} from '../../../../shared/services/layout-preferences-storage';
import {GameInputManager} from './game-input-manager';
import {InputActionsKeyField} from './input-actions-key-field';
import {InputActionsMonitor} from './input-actions-monitor';
import {starterInputActions} from '../../../../core/game-input/input-actions.models';
import {SceneEditingService} from '../scene-editing.service';

const project: ProjectData = {
    schema_version: 3, name: 'Input fixture', scenes: [{id: 'main', asset_path: 'scenes/main.json'}],
    startup_scene_id: 'main', fixed_hz: 60, max_catchup_steps: 8, behaviors: [], behavior_definitions: [],
    gameplay_module: 'campaign',
};

it('expires a key capture and rejects events from its expired or disabled field', async () => {
    const fixture = TestBed.createComponent(InputActionsKeyField);
    fixture.componentRef.setInput('label', 'Keyboard key');
    fixture.componentRef.setInput('value', 'space');
    const captured = vi.fn();
    fixture.componentInstance.keySelected.subscribe(captured);
    await fixture.whenStable();
    const root: HTMLElement = fixture.nativeElement;
    const button = root.querySelector('button')!;
    const press = () => button.dispatchEvent(new KeyboardEvent('keydown', {code: 'KeyQ', bubbles: true, cancelable: true}));
    vi.useFakeTimers({toFake: ['setTimeout', 'clearTimeout']});
    try {
        button.click();
        fixture.detectChanges();
        expect(button.getAttribute('aria-pressed')).toBe('true');
        vi.advanceTimersByTime(8000);
        fixture.detectChanges();
        expect(root.textContent).toContain('Key capture timed out');
        press();
        expect(captured).not.toHaveBeenCalled();
        button.click();
        fixture.componentRef.setInput('disabled', true);
        fixture.detectChanges();
        press();
        expect(captured).not.toHaveBeenCalled();
    } finally {
        vi.useRealTimers();
    }
});

it('updates the monitored face buttons and sticks across gamepad disconnect and reconnect while focused', async () => {
    const fixture = TestBed.createComponent(InputActionsMonitor);
    fixture.componentRef.setInput('map', starterInputActions());
    fixture.componentRef.setInput('active', true);
    let pad: Partial<Gamepad> | null = null;
    const property = Object.getOwnPropertyDescriptor(navigator, 'getGamepads');
    Object.defineProperty(navigator, 'getGamepads', {configurable: true, value: () => [pad]});
    await fixture.whenStable();
    const root: HTMLElement = fixture.nativeElement;
    const monitor = root.querySelector('section')!;
    const value = (action: string) => [...monitor.querySelectorAll('dt')]
        .find(label => label.textContent === action)?.nextElementSibling?.textContent;
    vi.useFakeTimers({toFake: ['setInterval', 'clearInterval']});
    const tick = () => {
        vi.advanceTimersByTime(50);
        fixture.detectChanges();
    };
    try {
        monitor.focus();
        fixture.detectChanges();
        expect(monitor.textContent).toContain('No gamepad detected');
        pad = {connected: true, mapping: 'standard', axes: [1, 0, 0, -1],
            buttons: [{pressed: true, touched: true, value: 1}]};
        tick();
        expect(value('Jump')).toBe('Pressed');
        expect(value('Move')).toBe('1.00, 0.00');
        expect(value('Look')).toContain('0.00, 1.00');
        pad = null;
        tick();
        expect(value('Jump')).toBe('Released');
        expect(value('Move')).toBe('0.00, 0.00');
        pad = {connected: true, mapping: 'standard', axes: [-1, 0, 0, 0], buttons: []};
        tick();
        expect(value('Move')).toBe('-1.00, 0.00');
        monitor.blur();
        tick();
        expect(value('Move')).toBe('0.00, 0.00');
    } finally {
        fixture.destroy();
        vi.useRealTimers();
        if (property) Object.defineProperty(navigator, 'getGamepads', property);
        else Reflect.deleteProperty(navigator, 'getGamepads');
    }
});

describe('Game Input Manager', () => {
    const prototype = HTMLDialogElement.prototype;
    const originalShow = Object.getOwnPropertyDescriptor(prototype, 'showModal');
    const originalClose = Object.getOwnPropertyDescriptor(prototype, 'close');

    beforeAll(() => {
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

    async function setup(withInspector = false) {
        const snapshot = signal<Pick<EditorSnapshot, 'mode' | 'project' | 'transaction' | 'revision'>>({
            mode: 'edit',
            project,
            transaction: null,
            revision: 7,
        });
        const workspaceGeneration = signal(1);
        const busy = signal(false);
        const execute = vi.fn(async (command: EditorCommand, _generation?: number, _revision?: number) => {
            if (command.type === 'set_project') snapshot.update(value => ({...value, project: command.project, revision: value.revision + 1}));
            return snapshot();
        });
        const withCommittedEdit = vi.fn(async (action: () => Promise<void>) => {
            await action();
            return true;
        });
        TestBed.configureTestingModule({
            providers: [
                {provide: LAYOUT_PREFERENCES_STORAGE, useValue: {load: async () => null, save: async () => undefined}},
                {
                    provide: EditorService, useValue: {
                        nativeAvailable: true, snapshot, workspaceGeneration, execute,
                        busy, playing: computed(() => snapshot().mode === 'play'),
                    }
                },
                ...(withInspector ? [{provide: SceneEditingService, useValue: {
                    ready: computed(() => !busy() && snapshot().mode === 'edit'),
                    error: signal<string | null>(null), withCommittedEdit,
                }}] : []),
            ]
        });
        const fixture = TestBed.createComponent(GameInputManager);
        await fixture.whenStable();
        const root: HTMLElement = fixture.nativeElement;
        const button = (text: string) => [...root.querySelectorAll<HTMLButtonElement>('button')]
            .find(candidate => candidate.textContent?.trim() === text)!;
        const opener = document.createElement('button');
        opener.textContent = 'Controls';
        root.prepend(opener);
        opener.focus();
        fixture.componentInstance.show();
        await fixture.whenStable();
        return {fixture, root, button, execute, workspaceGeneration, snapshot, withCommittedEdit, busy, opener};
    }

    it('restores focus to the opening control when Escape cancels the modal', async () => {
        const {fixture, root, opener} = await setup();
        const dialog = root.querySelector<HTMLDialogElement>('dialog[open]')!;
        dialog.querySelector('button')!.focus();
        dialog.dispatchEvent(new Event('cancel', {cancelable: true}));
        await fixture.whenStable();
        expect(dialog.open).toBe(false);
        expect(document.activeElement).toBe(opener);
    });

    it('previews the physical key through its game action and animation parameter and applies it to the project', async () => {
        const {fixture, root, button, execute} = await setup();
        button('Add Action').click();
        await fixture.whenStable();
        const [name, parameter] = root.querySelectorAll<HTMLInputElement>('.action-row input');
        name.value = 'jump';
        name.dispatchEvent(new Event('input'));
        parameter.value = 'jumpPressed';
        parameter.dispatchEvent(new Event('input'));
        const forward = root.querySelectorAll<HTMLSelectElement>('.movement-keys select')[3];
        forward.value = 'arrow_up';
        forward.dispatchEvent(new Event('input'));
        await fixture.whenStable();

        const preview = root.querySelector('.preview')!;
        const press = new KeyboardEvent('keydown', {code: 'Space', bubbles: true, cancelable: true});
        preview.dispatchEvent(press);
        preview.dispatchEvent(new KeyboardEvent('keydown', {code: 'ArrowUp', bubbles: true, cancelable: true}));
        await fixture.whenStable();
        expect(press.defaultPrevented).toBe(true);
        expect([...preview.querySelectorAll('dd')].map(node => node.textContent)).toEqual(['0.00, 1.00', 'jump', 'True']);
        preview.dispatchEvent(new FocusEvent('blur'));
        await fixture.whenStable();
        expect([...preview.querySelectorAll('dd')].map(node => node.textContent)).toEqual(['0.00, 0.00', 'None', 'False']);

        button('Apply Bindings').click();
        await fixture.whenStable();
        expect(execute).toHaveBeenCalledWith({
            type: 'set_project', project: {
                ...project, input_profile: {
                    schema_version: 1,
                    movement: {negative_x: 'a', positive_x: 'd', negative_y: 's', positive_y: 'arrow_up'},
                    actions: [{id: 'jump', key: 'space', animation_parameter: 'jumpPressed'}],
                }
            }
        }, 1, 7);
    });

    it('closes the draft when a different project becomes active', async () => {
        const {fixture, root, button, execute, workspaceGeneration} = await setup();
        button('Add Action').click();
        await fixture.whenStable();
        workspaceGeneration.update(generation => generation + 1);
        await fixture.whenStable();
        expect(root.querySelector('dialog[open]')).toBeNull();
        expect(execute).not.toHaveBeenCalled();
    });

    function changeMovement(root: HTMLElement, key = 'arrow_up'): void {
        const forward = root.querySelectorAll<HTMLSelectElement>('.movement-keys select')[3];
        forward.value = key;
        forward.dispatchEvent(new Event('input'));
    }

    it('builds the project payload after Inspector commit and captures its revision and generation', async () => {
        const {fixture, root, button, execute, snapshot, withCommittedEdit} = await setup(true);
        changeMovement(root);
        await fixture.whenStable();
        withCommittedEdit.mockImplementationOnce(async action => {
            snapshot.update(value => ({...value, revision: 12, project: {...value.project, name: 'Latest project', fixed_hz: 30}}));
            await action();
            return true;
        });
        button('Apply Bindings').click();
        await fixture.whenStable();
        expect(execute).toHaveBeenCalledExactlyOnceWith({
            type: 'set_project',
            project: {...project, name: 'Latest project', fixed_hz: 30, input_profile: {
                schema_version: 1,
                movement: {negative_x: 'a', positive_x: 'd', negative_y: 's', positive_y: 'arrow_up'},
                actions: [],
            }},
        }, 1, 12);
        expect(button('Apply Bindings').disabled).toBe(true);
    });

    it('rejects bindings changed while the dialog was open and retains its unapplied draft', async () => {
        const {fixture, root, button, execute, snapshot} = await setup();
        changeMovement(root);
        snapshot.update(value => ({...value, project: {...value.project, input_profile: {
            schema_version: 1,
            movement: {negative_x: 'arrow_left', positive_x: 'arrow_right', negative_y: 'arrow_down', positive_y: 'arrow_up'},
        }}}));
        await fixture.whenStable();
        button('Apply Bindings').click();
        await fixture.whenStable();
        expect(execute).not.toHaveBeenCalled();
        expect(root.querySelector('[role="alert"]')?.textContent).toContain('bindings changed');
        expect(button('Apply Bindings').disabled).toBe(false);
    });

    it('keeps a rejected input change available for retry', async () => {
        const {fixture, root, button, execute} = await setup();
        changeMovement(root);
        execute.mockRejectedValueOnce(new Error('This project revision changed.'));
        await fixture.whenStable();
        button('Apply Bindings').click();
        await fixture.whenStable();
        expect(root.querySelector('[role="alert"]')?.textContent).toContain('revision changed');
        expect(root.querySelector('[role="status"]')).toBeNull();
        expect(button('Apply Bindings').disabled).toBe(false);
    });

    it('ignores late Apply errors after the workspace changes', async () => {
        const {fixture, root, button, execute, workspaceGeneration} = await setup();
        changeMovement(root);
        let reject!: (error: Error) => void;
        execute.mockImplementationOnce(() => new Promise((_resolve, fail) => reject = fail));
        await fixture.whenStable();
        button('Apply Bindings').click();
        await vi.waitFor(() => expect(reject).toBeDefined());
        workspaceGeneration.set(2);
        await fixture.whenStable();
        reject(new Error('An old workspace request failed.'));
        await fixture.whenStable();
        expect(root.querySelector('dialog[open]')).toBeNull();
        expect(root.querySelector('[role="alert"]')).toBeNull();
        expect(root.querySelector('[role="status"]')).toBeNull();
    });

    it('does not send a project update when the workspace changes during Inspector commit', async () => {
        const {fixture, root, button, execute, workspaceGeneration, withCommittedEdit} = await setup(true);
        changeMovement(root);
        withCommittedEdit.mockImplementationOnce(async action => {
            workspaceGeneration.set(2);
            await action();
            return true;
        });
        await fixture.whenStable();
        button('Apply Bindings').click();
        await fixture.whenStable();
        expect(execute).not.toHaveBeenCalled();
        expect(root.querySelector('[role="alert"]')).toBeNull();
    });

    it('neutralizes diagonal preview input on window focus loss and disables Apply during Play', async () => {
        const {fixture, root, snapshot} = await setup();
        const preview = root.querySelector('.preview')!;
        preview.dispatchEvent(new KeyboardEvent('keydown', {code: 'KeyW', bubbles: true}));
        preview.dispatchEvent(new KeyboardEvent('keydown', {code: 'KeyD', bubbles: true}));
        await fixture.whenStable();
        expect(preview.querySelector('dd')?.textContent).toBe('0.71, 0.71');
        window.dispatchEvent(new Event('blur'));
        await fixture.whenStable();
        expect(preview.querySelector('dd')?.textContent).toBe('0.00, 0.00');
        snapshot.update(value => ({...value, mode: 'play'}));
        await fixture.whenStable();
        expect(root.querySelector('dialog[open]')).toBeNull();
        expect(fixture.componentInstance.available()).toBe(false);
    });

    it('has accessible controls for movement and a named action', async () => {
        const {fixture, root, button} = await setup();
        button('Add Action').click();
        await fixture.whenStable();
        const report = await axe.run(root, {rules: {'color-contrast': {enabled: false}}});
        expect(report.violations.map(violation => violation.id)).toEqual([]);
    });
});
