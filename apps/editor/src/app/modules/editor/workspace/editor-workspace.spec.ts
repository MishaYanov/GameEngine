import {ComponentFixture, TestBed} from '@angular/core/testing';
import {provideRouter} from '@angular/router';

import {EditorWorkspace} from './editor-workspace';

describe('EditorWorkspace', () => {
    let component: EditorWorkspace;
    let fixture: ComponentFixture<EditorWorkspace>;

    beforeEach(async () => {
        await TestBed.configureTestingModule({
            imports: [EditorWorkspace],
            providers: [provideRouter([])],
        }).compileComponents();

        fixture = TestBed.createComponent(EditorWorkspace);
        component = fixture.componentInstance;
        await fixture.whenStable();
    });

    it('should create', () => {
        expect(component).toBeTruthy();
    });
});
