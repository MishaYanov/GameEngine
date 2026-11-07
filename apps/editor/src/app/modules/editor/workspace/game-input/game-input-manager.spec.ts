import {computed, signal} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import {vi} from 'vitest';
import type {EditorCommand, EditorSnapshot, ProjectData} from '../../../../core/editor/editor.models';
import {EditorService} from '../../../../core/editor/editor.service';
import {LAYOUT_PREFERENCES_STORAGE} from '../../../../shared/services/layout-preferences-storage';
import {GameInputManager} from './game-input-manager';

const project: ProjectData = {
    schema_version: 3, name: 'Input fixture', scenes: [{id: 'main', asset_path: 'scenes/main.json'}],
    startup_scene_id: 'main', fixed_hz: 60, max_catchup_steps: 8, behaviors: [], behavior_definitions: [],
};

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

    async function setup() {
        const snapshot = signal<Pick<EditorSnapshot, 'mode' | 'project' | 'transaction'>>({
            mode: 'edit',
            project,
            transaction: null
        });
        const workspaceGeneration = signal(1);
        const execute = vi.fn(async (command: EditorCommand) => {
            if (command.type === 'set_project') snapshot.update(value => ({...value, project: command.project}));
        });
        TestBed.configureTestingModule({
            providers: [
                {provide: LAYOUT_PREFERENCES_STORAGE, useValue: {load: async () => null, save: async () => undefined}},
                {
                    provide: EditorService, useValue: {
                        nativeAvailable: true, snapshot, workspaceGeneration, execute,
                        busy: signal(false), playing: computed(() => snapshot().mode === 'play'),
                    }
                },
            ]
        });
        const fixture = TestBed.createComponent(GameInputManager);
        await fixture.whenStable();
        const root: HTMLElement = fixture.nativeElement;
        const button = (text: string) => [...root.querySelectorAll<HTMLButtonElement>('button')]
            .find(candidate => candidate.textContent?.trim() === text)!;
        button('Game Input…').click();
        await fixture.whenStable();
        return {fixture, root, button, execute, workspaceGeneration};
    }

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
        });
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
});
