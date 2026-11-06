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

    it('should create', () => {
        expect(component).toBeTruthy();
    });
});
