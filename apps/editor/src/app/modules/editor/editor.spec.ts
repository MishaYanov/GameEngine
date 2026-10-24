import {ComponentFixture, TestBed} from '@angular/core/testing';

import {Editor} from './editor';

describe('Editor', () => {
    let component: Editor;
    let fixture: ComponentFixture<Editor>;

    beforeEach(async () => {
        await TestBed.configureTestingModule({
            imports: [Editor],
        }).compileComponents();

        fixture = TestBed.createComponent(Editor);
        component = fixture.componentInstance;
        await fixture.whenStable();
    });

    it('should create', () => {
        expect(component).toBeTruthy();
    });
});
