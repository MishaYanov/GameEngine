import {Component} from '@angular/core';
import {ComponentFixture, TestBed} from '@angular/core/testing';
import {provideRouter, Router} from '@angular/router';
import {vi} from 'vitest';

import {EditorFileMenu} from './editor-file-menu';

@Component({template: ''})
class ProjectSetupStub {}

describe('EditorFileMenu', () => {
    let component: EditorFileMenu;
    let fixture: ComponentFixture<EditorFileMenu>;

    beforeEach(async () => {
        await TestBed.configureTestingModule({
            imports: [EditorFileMenu],
            providers: [provideRouter([{path: 'setup', component: ProjectSetupStub}])],
        }).compileComponents();

        fixture = TestBed.createComponent(EditorFileMenu);
        component = fixture.componentInstance;
        await fixture.whenStable();
    });

    it('should create', () => {
        expect(component).toBeTruthy();
    });

    it('keeps New Project disabled when project navigation is blocked', async () => {
        const root = fixture.nativeElement as HTMLElement;
        const button = root.querySelector<HTMLButtonElement>('.file-menu button')!;
        const menu = root.querySelector<HTMLElement>('.file-menu')!;
        const hidePopover = vi.fn();
        Object.defineProperty(menu, 'hidePopover', {value: hidePopover});

        expect(button.textContent).toContain('New Project');
        expect(button.disabled).toBe(true);
        button.click();
        await fixture.whenStable();
        expect(hidePopover).not.toHaveBeenCalled();
        expect(TestBed.inject(Router).url).toBe('/');
    });

    it('closes the menu and opens setup with the editor return context', async () => {
        fixture.componentRef.setInput('canOpen', true);
        await fixture.whenStable();
        const root = fixture.nativeElement as HTMLElement;
        const button = root.querySelector<HTMLButtonElement>('.file-menu button')!;
        const menu = root.querySelector<HTMLElement>('.file-menu')!;
        const hidePopover = vi.fn();
        Object.defineProperty(menu, 'hidePopover', {value: hidePopover});
        expect(button.disabled).toBe(false);

        button.click();
        await fixture.whenStable();
        expect(hidePopover).toHaveBeenCalledOnce();
        expect(TestBed.inject(Router).url).toBe('/setup?from=editor');
    });

    it('disables both project actions if the editor becomes busy or starts Play', async () => {
        fixture.componentRef.setInput('canOpen', true);
        await fixture.whenStable();
        fixture.componentRef.setInput('canOpen', false);
        await fixture.whenStable();
        const buttons = [...(fixture.nativeElement as HTMLElement).querySelectorAll<HTMLButtonElement>('.file-menu button')];
        expect(buttons).toHaveLength(2);
        expect(buttons.every(button => button.disabled)).toBe(true);
    });
});
