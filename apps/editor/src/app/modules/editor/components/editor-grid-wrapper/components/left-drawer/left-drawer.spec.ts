import {ComponentFixture, TestBed} from '@angular/core/testing';

import {LeftDrawer} from './left-drawer';

describe('LeftDrawer', () => {
    let component: LeftDrawer;
    let fixture: ComponentFixture<LeftDrawer>;

    beforeEach(async () => {
        await TestBed.configureTestingModule({
            imports: [LeftDrawer],
        }).compileComponents();

        fixture = TestBed.createComponent(LeftDrawer);
        component = fixture.componentInstance;
        await fixture.whenStable();
    });

    it('should create', () => {
        expect(component).toBeTruthy();
    });
});
