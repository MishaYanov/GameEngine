import {ComponentFixture, TestBed} from '@angular/core/testing';

import {TabsManager} from './tabs-manager';

describe('TabsManager', () => {
    let component: TabsManager;
    let fixture: ComponentFixture<TabsManager>;

    beforeEach(async () => {
        await TestBed.configureTestingModule({
            imports: [TabsManager],
        }).compileComponents();

        fixture = TestBed.createComponent(TabsManager);
        component = fixture.componentInstance;
        await fixture.whenStable();
    });

    it('should create', () => {
        expect(component).toBeTruthy();
    });
});
