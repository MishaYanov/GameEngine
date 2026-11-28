import {signal} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import {vi} from 'vitest';
import {MAX_AUTHORING_NUMBER_LENGTH} from '../../../../core/editor/authoring-number';
import type {EditorCommand, Light} from '../../../../core/editor/editor.models';
import {EditorService} from '../../../../core/editor/editor.service';
import {SceneEditingService} from '../scene-editing.service';
import {SceneLightsDialog} from './scene-lights-dialog';

const original: Light = {
    key: 'lamp', name: 'Lamp', kind: 'point', position: [0, 1, 0], direction: [0, -1, 0],
    color: [1, 1, 1], intensity: 2, range: 10,
};

function button(root: HTMLElement, label: string): HTMLButtonElement {
    return [...root.querySelectorAll('button')].find(item => item.textContent?.trim() === label)!;
}

describe('Scene Lights', () => {
    it('edits scene-owned lighting through a revisioned command and keeps a stale draft visible', async () => {
        const prototype = HTMLDialogElement.prototype;
        let destroyFixture: (() => void) | undefined;
        const show = Object.getOwnPropertyDescriptor(prototype, 'showModal');
        const close = Object.getOwnPropertyDescriptor(prototype, 'close');
        Object.defineProperty(prototype, 'showModal', {
            configurable: true, value(this: HTMLDialogElement) {
                this.setAttribute('open', '');
            }
        });
        Object.defineProperty(prototype, 'close', {
            configurable: true, value(this: HTMLDialogElement) {
                this.removeAttribute('open');
                this.dispatchEvent(new Event('close'));
            }
        });
        try {
            const longLight = {
                ...original, key: 'balcony',
                name: 'The unusually long brass lantern illuminating the back wall of the upper balcony'
            };
            const snapshot = signal({
                mode: 'edit', revision: 7, active_scene_id: 'main', document: {lights: [original, longLight]},
            });
            const mutationError = signal<string | null>(null);
            const execute = vi.fn(async (command: EditorCommand) => {
                const lights = command.type === 'upsert_light' ? [command.light] : [];
                snapshot.update(current => ({...current, revision: current.revision + 1, document: {lights}}));
            });
            TestBed.configureTestingModule({
                providers: [
                    {
                        provide: EditorService, useValue: {
                            snapshot, workspaceGeneration: () => 2, document: () => snapshot().document,
                            capabilities: () => ({commands: [{name: 'remove_light'}]}), execute,
                        }
                    },
                    {
                        provide: SceneEditingService, useValue: {
                            ready: () => true, error: mutationError,
                            withCommittedEdit: async (mutation: () => Promise<void>) => {
                                try {
                                    await mutation();
                                    return true;
                                } catch (error) {
                                    mutationError.set(String(error));
                                    return false;
                                }
                            },
                        }
                    },
                ]
            });

            const fixture = TestBed.createComponent(SceneLightsDialog);
            destroyFixture = () => fixture.destroy();
            const root = fixture.nativeElement as HTMLElement;
            expect(fixture.componentInstance.show('lamp')).toBe(true);
            await fixture.whenStable();
            const name = root.querySelector<HTMLInputElement>('input[maxlength="1024"]')!;
            const picker = root.querySelector<HTMLInputElement>('input[role="combobox"]')!;
            const pick = async (query: string) => {
                picker.value = query;
                picker.dispatchEvent(new Event('input', {bubbles: true}));
                await fixture.whenStable();
                picker.dispatchEvent(new KeyboardEvent('keydown', {key: 'Enter', bubbles: true, cancelable: true}));
                await fixture.whenStable();
            };
            await pick('back wall');
            expect(name.value).toBe(longLight.name);
            expect(picker.title).toBe(longLight.name);
            await pick('Lamp');
            expect(name.value).toBe('Lamp');
            const rename = async (value: string) => {
                name.value = value;
                name.dispatchEvent(new Event('input'));
                await fixture.whenStable();
            };
            await rename('Lantern');
            expect(picker.disabled).toBe(true);
            const fields = [...root.querySelectorAll<HTMLInputElement>('input[inputmode="decimal"]')];
            expect(fields).toHaveLength(8);
            expect(fields.every(input => input.maxLength === MAX_AUTHORING_NUMBER_LENGTH)).toBe(true);
            const enter = async (input: HTMLInputElement, value: string) => {
                input.value = value;
                input.dispatchEvent(new Event('input'));
                await fixture.whenStable();
            };
            const x = fields[0];
            await enter(x, '-');
            expect(x.value).toBe('-');
            expect(button(root, 'Apply').disabled).toBe(true);
            await enter(x, '0'.repeat(MAX_AUTHORING_NUMBER_LENGTH) + '1');
            expect(button(root, 'Apply').disabled).toBe(true);
            expect(execute).not.toHaveBeenCalled();

            await enter(x, '-1.25');
            await enter(fields[4], '.5');
            await enter(fields[6], '+2.');
            await enter(fields[7], '1.25e1');
            expect(button(root, 'Apply').disabled).toBe(false);
            expect(fixture.componentInstance.canLeaveProject()).toBe(false);
            button(root, 'Apply').click();
            await fixture.whenStable();
            const applied: Light = {
                ...original,
                name: 'Lantern',
                position: [-1.25, 1, 0],
                color: [1, 0.5, 1],
                range: 12.5
            };
            expect(execute).toHaveBeenCalledExactlyOnceWith({type: 'upsert_light', light: applied}, 2, 7);

            await rename('Local edit');
            snapshot.update(current => ({
                ...current, revision: current.revision + 1,
                document: {lights: [{...applied, intensity: 3}]},
            }));
            await fixture.whenStable();
            button(root, 'Apply').click();
            await fixture.whenStable();
            expect(execute).toHaveBeenCalledOnce();
            expect(root.querySelector('[role="alert"]')?.textContent).toContain('This light changed');
            expect(name.value).toBe('Local edit');

            button(root, 'Revert Draft').click();
            await fixture.whenStable();
            button(root, 'Remove Light').click();
            await fixture.whenStable();
            expect(execute).toHaveBeenLastCalledWith({type: 'remove_light', key: 'lamp'}, 2, 9);
            expect(root.textContent).toContain('Add a Point Light or Directional Light');
            expect(fixture.componentInstance.canLeaveProject()).toBe(true);
            await fixture.whenStable();
        } finally {
            destroyFixture?.();
            if (show) Object.defineProperty(prototype, 'showModal', show);
            else Reflect.deleteProperty(prototype, 'showModal');
            if (close) Object.defineProperty(prototype, 'close', close);
            else Reflect.deleteProperty(prototype, 'close');
        }
    });
});
