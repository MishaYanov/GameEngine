import {ComponentFixture, TestBed} from '@angular/core/testing';

import {RightDrawer} from './right-drawer';

describe('RightDrawer', () => {
    let component: RightDrawer;
    let fixture: ComponentFixture<RightDrawer>;

    beforeEach(async () => {
        await TestBed.configureTestingModule({
            imports: [RightDrawer],
        }).compileComponents();

        fixture = TestBed.createComponent(RightDrawer);
        component = fixture.componentInstance;
        await fixture.whenStable();
    });

    it('should create', () => {
        expect(component).toBeTruthy();
    });
});
