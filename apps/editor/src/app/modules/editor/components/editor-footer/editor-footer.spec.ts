import {ComponentFixture, TestBed} from '@angular/core/testing';

import {EditorFooter} from './editor-footer';

describe('EditorFooter', () => {
    let component: EditorFooter;
    let fixture: ComponentFixture<EditorFooter>;

    beforeEach(async () => {
        await TestBed.configureTestingModule({
            imports: [EditorFooter],
        }).compileComponents();

        fixture = TestBed.createComponent(EditorFooter);
        component = fixture.componentInstance;
        await fixture.whenStable();
    });

    it('shows the empty state until a project is open', () => {
        expect(component).toBeTruthy();
        expect(fixture.nativeElement.textContent).toContain('No project open');
    });

    it('tracks the open project and returns to the empty state when closed', async () => {
        fixture.componentRef.setInput('projectName', 'Robot workshop');
        await fixture.whenStable();
        expect(fixture.nativeElement.textContent).toContain('Robot workshop');
        expect(fixture.nativeElement.textContent).not.toContain('No project open');
        fixture.componentRef.setInput('projectName', null);
        await fixture.whenStable();
        expect(fixture.nativeElement.textContent).toContain('No project open');
    });
});
