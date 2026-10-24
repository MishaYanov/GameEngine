import {ComponentFixture, TestBed} from '@angular/core/testing';

import {EditorHeader} from './editor-header';

describe('EditorHeader', () => {
    let component: EditorHeader;
    let fixture: ComponentFixture<EditorHeader>;

    beforeEach(async () => {
        await TestBed.configureTestingModule({
            imports: [EditorHeader],
        }).compileComponents();

        fixture = TestBed.createComponent(EditorHeader);
        component = fixture.componentInstance;
        await fixture.whenStable();
    });

    it('should create', () => {
        expect(component).toBeTruthy();
    });
});
