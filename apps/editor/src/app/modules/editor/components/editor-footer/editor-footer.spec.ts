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

    it('should create', () => {
        expect(component).toBeTruthy();
    });
});
