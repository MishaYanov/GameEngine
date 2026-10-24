import {ComponentFixture, TestBed} from '@angular/core/testing';

import {EditorNavManager} from './editor-nav-manager';

describe('EditorNavManager', () => {
    let component: EditorNavManager;
    let fixture: ComponentFixture<EditorNavManager>;

    beforeEach(async () => {
        await TestBed.configureTestingModule({
            imports: [EditorNavManager],
        }).compileComponents();

        fixture = TestBed.createComponent(EditorNavManager);
        component = fixture.componentInstance;
        await fixture.whenStable();
    });

    it('should create', () => {
        expect(component).toBeTruthy();
    });
});
