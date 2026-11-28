import {signal} from '@angular/core';
import {ComponentFixture, TestBed} from '@angular/core/testing';
import {provideRouter} from '@angular/router';
import {vi} from 'vitest';

import {EditorService} from '../../../../core/editor/editor.service';
import {ContextMenuService} from '../../../../shared/components/context-menu/context-menu.service';
import {EditorHeader} from './editor-header';

describe('EditorHeader', () => {
    let component: EditorHeader;
    let fixture: ComponentFixture<EditorHeader>;
    let root: HTMLElement;
    const projectSnapshot = {project: {name: 'Cinder Oath'}, active_scene_id: 'intro', dirty: true};
    const snapshot = signal<typeof projectSnapshot | null>(null);

    beforeEach(async () => {
        snapshot.set(null);
        await TestBed.configureTestingModule({
            imports: [EditorHeader],
            providers: [
                provideRouter([]),
                {provide: EditorService, useValue: {snapshot, nativeAvailable: true, workspaceGeneration: () => 1}},
            ],
        }).compileComponents();

        fixture = TestBed.createComponent(EditorHeader);
        component = fixture.componentInstance;
        root = fixture.nativeElement;
        await fixture.whenStable();
    });

    it('should create', () => {
        expect(component).toBeTruthy();
    });

    it('places Game beside File and enables scene information after a project opens', async () => {
        const menuTriggers = [...root.querySelectorAll('button[popovertarget]')]
            .map(button => button.textContent.trim());
        const sceneInformation = [...root.querySelectorAll<HTMLButtonElement>('#editor-game-menu button')]
            .find(button => button.textContent.trim() === 'Scene Information…')!;
        expect(menuTriggers).toEqual(['File', 'Game', 'Project']);
        expect(sceneInformation.textContent.trim()).toBe('Scene Information…');
        expect(sceneInformation.disabled).toBe(true);

        snapshot.set(projectSnapshot);
        await fixture.whenStable();
        expect(sceneInformation.disabled).toBe(false);
        expect(root.querySelector('h1')?.textContent.trim()).toBe('Cinder Oath');
        expect(root.querySelector('.breadcrumb')?.textContent).not.toContain('intro');
        expect(root.querySelector('.dirty-label')?.textContent.trim()).toBe('Unsaved changes');

        fixture.componentRef.setInput('opening', true);
        await fixture.whenStable();
        expect(sceneInformation.disabled).toBe(true);
    });

    it('closes the Game menu and focuses its trigger before opening the scene dialog', async () => {
        snapshot.set(projectSnapshot);
        await fixture.whenStable();
        const gameTrigger = root.querySelector<HTMLButtonElement>('button[popovertarget="editor-game-menu"]')!;
        const gameMenu = root.querySelector<HTMLElement>('#editor-game-menu')!;
        const hidePopover = vi.fn();
        Object.defineProperty(gameMenu, 'hidePopover', {value: hidePopover});
        const openSceneInformation = vi.fn(() => {
            expect(hidePopover).toHaveBeenCalledOnce();
            expect(document.activeElement).toBe(gameTrigger);
        });
        component.sceneInformationRequested.subscribe(openSceneInformation);

        [...gameMenu.querySelectorAll<HTMLButtonElement>('button')]
            .find(button => button.textContent.trim() === 'Scene Information…')!.click();

        expect(openSceneInformation).toHaveBeenCalledOnce();
    });

    it('puts Controls next to Game and focuses the trigger before requesting its modal', async () => {
        const trigger = root.querySelector<HTMLButtonElement>('.controls-trigger')!;
        const open = vi.fn(() => expect(document.activeElement).toBe(trigger));
        component.controlsRequested.subscribe(open);
        expect(trigger.previousElementSibling?.id).toBe('editor-game-menu');
        expect(trigger.disabled).toBe(true);
        fixture.componentRef.setInput('canOpenControls', true);
        await fixture.whenStable();
        trigger.click();
        expect(open).toHaveBeenCalledOnce();
        fixture.componentRef.setInput('opening', true);
        await fixture.whenStable();
        expect(trigger.disabled).toBe(true);
    });

    it('opens the keyboard menu for supported Objects actions and restores the trigger on selection', async () => {
        const trigger = root.querySelector<HTMLButtonElement>('.objects-trigger')!;
        const menu = TestBed.inject(ContextMenuService);
        const execute = vi.fn(() => expect(document.activeElement).toBe(trigger));
        component.objectActionRequested.subscribe(execute);
        expect(trigger.disabled).toBe(true);
        snapshot.set(projectSnapshot);
        fixture.componentRef.setInput('availableObjectActions', ['browse_props', 'create_camera']);
        await fixture.whenStable();

        trigger.focus();
        const down = new KeyboardEvent('keydown', {key: 'ArrowDown', bubbles: true, cancelable: true});
        trigger.dispatchEvent(down);
        expect(down.defaultPrevented).toBe(true);
        await fixture.whenStable();
        expect(trigger.getAttribute('aria-expanded')).toBe('true');
        expect(menu.request()?.label).toBe('Objects');
        const camera = menu.request()!.items.find(item => item.id === 'create_camera')!;
        const particle = menu.request()!.items.find(item => item.id === 'create_particle_emitter')!;
        expect(camera).toMatchObject({label: 'Camera', disabled: false});
        expect(particle).toBeUndefined();
        expect(execute).not.toHaveBeenCalled();
        menu.execute(camera);
        await fixture.whenStable();
        expect(execute).toHaveBeenCalledExactlyOnceWith('create_camera');
        expect(trigger.getAttribute('aria-expanded')).toBe('false');
    });
});
