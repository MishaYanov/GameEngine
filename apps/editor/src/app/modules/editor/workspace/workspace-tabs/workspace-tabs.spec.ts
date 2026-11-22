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

    it('puts Animation after Prop and reaches it using the arrow keys', async () => {
        fixture.componentRef.setInput('propAvailable', true);
        fixture.componentRef.setInput('animationAvailable', true);
        fixture.componentRef.setInput('activeViewKind', 'prop-workspace');
        await fixture.whenStable();
        const buttons = [...fixture.nativeElement.querySelectorAll('[role=tab]')] as HTMLButtonElement[];
        expect(buttons.map(button => button.textContent?.trim())).toEqual(['Scene', 'Game', 'Prop', 'Animation']);
        const views: string[] = [];
        const subscription = component.viewSelected.subscribe(view => views.push(view));
        buttons[2].dispatchEvent(new KeyboardEvent('keydown', {key: 'ArrowRight'}));
        expect(views).toEqual(['animation-workspace']);
        expect(document.activeElement).toBe(buttons[3]);
        subscription.unsubscribe();
    });

    it('should create', () => {
        expect(component).toBeTruthy();
    });
});
