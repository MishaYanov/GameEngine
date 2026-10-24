import {ComponentFixture, TestBed} from '@angular/core/testing';

import {EditorGridWrapper} from './editor-grid-wrapper';

describe('EditorGridWrapper', () => {
    let component: EditorGridWrapper;
    let fixture: ComponentFixture<EditorGridWrapper>;

    beforeEach(async () => {
        await TestBed.configureTestingModule({
            imports: [EditorGridWrapper],
        }).compileComponents();

        fixture = TestBed.createComponent(EditorGridWrapper);
        component = fixture.componentInstance;
        await fixture.whenStable();
    });

    it('should create', () => {
        expect(component).toBeTruthy();
    });
});
