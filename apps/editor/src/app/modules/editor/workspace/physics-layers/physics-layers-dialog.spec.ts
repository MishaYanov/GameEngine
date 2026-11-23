import {signal} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import {vi} from 'vitest';
import type {EditorCommand, EditorSnapshot, ProjectData} from '../../../../core/editor/editor.models';
import {EditorService} from '../../../../core/editor/editor.service';
import {LAYOUT_PREFERENCES_STORAGE} from '../../../../shared/services/layout-preferences-storage';
import {SceneEditingService} from '../scene-editing.service';
import {PhysicsLayersDialog} from './physics-layers-dialog';

const project: ProjectData = {
    schema_version: 3, name: 'Fixture', startup_scene_id: 'level',
    scenes: [{id: 'level', asset_path: 'scenes/level.json'}],
    fixed_hz: 60, max_catchup_steps: 8, behaviors: [], behavior_definitions: [],
};

describe('Physics Layers project dialog', () => {
    const prototype = HTMLDialogElement.prototype;
    const originalShow = Object.getOwnPropertyDescriptor(prototype, 'showModal');
    const originalClose = Object.getOwnPropertyDescriptor(prototype, 'close');

    beforeAll(() => {
        Object.defineProperty(prototype, 'showModal', {
            configurable: true, value(this: HTMLDialogElement) { this.setAttribute('open', ''); },
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

    async function setup() {
        const snapshot = signal<Pick<EditorSnapshot, 'mode' | 'revision' | 'project'>>({
            mode: 'edit', revision: 8, project,
        });
        const error = signal<string | null>(null);
        const execute = vi.fn(async (command: EditorCommand) => {
            if (command.type === 'set_project') snapshot.update(value => ({...value, project: command.project}));
        });
        TestBed.configureTestingModule({providers: [
            {provide: LAYOUT_PREFERENCES_STORAGE, useValue: {load: vi.fn(async () => ({})), save: vi.fn()}},
            {provide: EditorService, useValue: {snapshot, workspaceGeneration: () => 2, execute}},
            {provide: SceneEditingService, useValue: {ready: () => true, error, withCommittedEdit: async (write: () => Promise<void>) => {
                try { await write(); return true; }
                catch (failure) { error.set(String(failure)); return false; }
            }}},
        ]});
        const fixture = TestBed.createComponent(PhysicsLayersDialog);
        await fixture.whenStable();
        expect(fixture.componentInstance.show()).toBe(true);
        await fixture.whenStable();
        const root = fixture.nativeElement as HTMLElement;
        const click = async (name: string) => {
            const button = [...root.querySelectorAll('button')].find(item =>
                item.textContent?.trim() === name || item.getAttribute('aria-label') === name);
            expect(button, name).toBeDefined();
            button!.click();
            await fixture.whenStable();
        };
        return {fixture, root, click, snapshot, execute};
    }

    it('previews the preset, mirrors a changed pair and applies one revisioned project edit', async () => {
        const {root, click, execute, snapshot} = await setup();
        await click('Preview Standard World');
        expect(root.textContent).toContain('Standard World preview');
        expect(execute).not.toHaveBeenCalled();
        await click('Use preset');
        await click('Terrain and Player, Allow');
        const player = [...root.querySelectorAll<HTMLTableRowElement>('tbody tr')]
            .find(row => row.querySelector('th')?.textContent === 'Player')!;
        expect(player.cells[2].textContent).toBe('Ignore');
        expect(player.cells[2].querySelector('button')).toBeNull();
        await click('Apply');
        expect(execute).toHaveBeenCalledExactlyOnceWith({
            type: 'set_project', project: snapshot().project,
        }, 2, 8);
        expect(snapshot().project.physics_layers?.ignored_pairs).toContainEqual([1, 3]);
        expect(root.querySelector('dialog[open]')).toBeNull();
    });

    it('discards a draft and keeps an attempted stale apply visible for review', async () => {
        const {fixture, root, click, execute, snapshot} = await setup();
        await click('Preview Standard World');
        await click('Use preset');
        await click('Cancel');
        expect(execute).not.toHaveBeenCalled();
        expect(fixture.componentInstance.show()).toBe(true);
        await fixture.whenStable();
        expect(root.textContent).not.toContain('Terrain');
        await click('Default and Default, Allow');
        snapshot.update(value => ({
            ...value, revision: 9,
            project: {...project, physics_layers: {layers: [], ignored_pairs: [[5, 5]]}},
        }));
        await click('Apply');
        expect(execute).not.toHaveBeenCalled();
        expect(root.querySelector('dialog[open]')).not.toBeNull();
        expect(root.querySelector('[role=alert]')?.textContent).toContain('Physics layers changed');
    });

    it('keeps preview and draft changes open when project navigation is attempted', async () => {
        const {fixture, root, click, execute} = await setup();
        const dialog = fixture.componentInstance;
        await click('Preview Standard World');
        expect(dialog.canLeaveProject()).toBe(false);
        await fixture.whenStable();
        expect(root.querySelector('dialog[open]')).not.toBeNull();
        expect(root.querySelector('[role=alert]')?.textContent).toContain('Use or discard');

        await click('Discard preview');
        expect(dialog.canLeaveProject()).toBe(true);
        await fixture.whenStable();
        expect(root.querySelector('dialog[open]')).toBeNull();

        expect(dialog.show()).toBe(true);
        await fixture.whenStable();
        await click('Default and Default, Allow');
        expect(dialog.canLeaveProject()).toBe(false);
        await fixture.whenStable();
        expect(root.querySelector('dialog[open]')).not.toBeNull();
        expect(root.querySelector('[role=alert]')?.textContent).toContain('Apply your Physics Layers changes');
        expect(execute).not.toHaveBeenCalled();

        await click('Cancel');
        expect(dialog.canLeaveProject()).toBe(true);
    });
});
