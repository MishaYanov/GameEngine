import {ComponentFixture, TestBed} from '@angular/core/testing';

import {EditorPortal} from './editor-portal';

describe('EditorPortal', () => {
    let component: EditorPortal;
    let fixture: ComponentFixture<EditorPortal>;

    beforeEach(async () => {
        await TestBed.configureTestingModule({
            imports: [EditorPortal],
        }).compileComponents();

        fixture = TestBed.createComponent(EditorPortal);
        component = fixture.componentInstance;
        await fixture.whenStable();
    });

    it('should create', () => {
        expect(component).toBeTruthy();
    });
});
