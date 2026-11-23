import {ComponentFixture, TestBed} from '@angular/core/testing';

import {WorkspaceTabs} from './workspace-tabs';

describe('WorkspaceTabs', () => {
    let component: WorkspaceTabs;
    let fixture: ComponentFixture<WorkspaceTabs>;

    beforeEach(async () => {
        await TestBed.configureTestingModule({
            imports: [WorkspaceTabs],
        }).compileComponents();

        fixture = TestBed.createComponent(WorkspaceTabs);
        component = fixture.componentInstance;
        await fixture.whenStable();
    });

    it('moves roving focus without selecting and activates the focused tab with Enter or Space', async () => {
        fixture.componentRef.setInput('propAvailable', true);
        fixture.componentRef.setInput('animationAvailable', true);
        fixture.componentRef.setInput('activeViewKind', 'prop-workspace');
        await fixture.whenStable();
        const buttons = [...fixture.nativeElement.querySelectorAll('[role=tab]')] as HTMLButtonElement[];
        expect(buttons.map(button => button.textContent?.trim())).toEqual(['Scene', 'Game', 'Prop', 'Animation']);
        const select = vi.fn((view: string) => fixture.componentRef.setInput('activeViewKind', view));
        component.viewSelected.subscribe(select);
        const press = (button: HTMLButtonElement, key: string) =>
            button.dispatchEvent(new KeyboardEvent('keydown', {key, cancelable: true}));
        const expectRovingFocus = async (focused: HTMLButtonElement, selected: HTMLButtonElement) => {
            await fixture.whenStable();
            expect(document.activeElement).toBe(focused);
            expect(buttons.filter(button => button.tabIndex === 0)).toEqual([focused]);
            expect(buttons.filter(button => button.getAttribute('aria-selected') === 'true')).toEqual([selected]);
        };
        const [scene, game, prop, animation] = buttons;
        prop.focus();
        press(prop, 'ArrowRight');
        await expectRovingFocus(animation, prop);
        press(animation, 'ArrowRight');
        await expectRovingFocus(scene, prop);
        press(scene, 'ArrowLeft');
        await expectRovingFocus(animation, prop);
        press(animation, 'Home');
        await expectRovingFocus(scene, prop);
        press(scene, 'End');
        await expectRovingFocus(animation, prop);
        expect(select).not.toHaveBeenCalled();
        expect(press(animation, 'Enter')).toBe(false);
        await expectRovingFocus(animation, animation);
        press(animation, 'ArrowLeft');
        await expectRovingFocus(prop, animation);
        expect(press(prop, ' ')).toBe(false);
        await expectRovingFocus(prop, prop);
        game.click();
        await fixture.whenStable();
        expect(select.mock.calls.map(([view]) => view)).toEqual(['animation-workspace', 'prop-workspace', 'game-preview']);
    });

    it('returns the tab stop to the selected tab when focus leaves the tab list', async () => {
        const root = fixture.nativeElement as HTMLElement;
        const [scene, game] = root.querySelectorAll<HTMLButtonElement>('[role=tab]');
        const outside = document.createElement('button');
        root.append(outside);
        scene.focus();
        scene.dispatchEvent(new KeyboardEvent('keydown', {key: 'ArrowRight'}));
        await fixture.whenStable();
        expect(game.tabIndex).toBe(0);
        outside.focus();
        await fixture.whenStable();
        expect(document.activeElement).toBe(outside);
        expect(scene.tabIndex).toBe(0);
        expect(game.tabIndex).toBe(-1);
    });

    it('should create', () => {
        expect(component).toBeTruthy();
    });
});
