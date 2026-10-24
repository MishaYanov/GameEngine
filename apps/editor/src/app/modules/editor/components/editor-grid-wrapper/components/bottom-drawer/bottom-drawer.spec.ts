import {ComponentFixture, TestBed} from '@angular/core/testing';

import {BottomDrawer} from './bottom-drawer';

describe('BottomDrawer', () => {
    let component: BottomDrawer;
    let fixture: ComponentFixture<BottomDrawer>;

    beforeEach(async () => {
        await TestBed.configureTestingModule({
            imports: [BottomDrawer],
        }).compileComponents();

        fixture = TestBed.createComponent(BottomDrawer);
        component = fixture.componentInstance;
        await fixture.whenStable();
    });

    it('should create', () => {
        expect(component).toBeTruthy();
    });
});
