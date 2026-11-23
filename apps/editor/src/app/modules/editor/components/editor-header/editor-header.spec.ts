import {signal} from '@angular/core';
import {ComponentFixture, TestBed} from '@angular/core/testing';
import {provideRouter} from '@angular/router';
import {vi} from 'vitest';

import {EditorService} from '../../../../core/editor/editor.service';
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
                {provide: EditorService, useValue: {snapshot, nativeAvailable: true}},
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
        const sceneInformation = root.querySelector<HTMLButtonElement>('#editor-game-menu button')!;
        expect(menuTriggers).toEqual(['File', 'Game']);
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

        gameMenu.querySelector<HTMLButtonElement>('button')!.click();

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
});
