import {ComponentFixture, TestBed} from '@angular/core/testing';

import {SettingsPortal} from './settings-portal';

describe('SettingsPortal', () => {
    let component: SettingsPortal;
    let fixture: ComponentFixture<SettingsPortal>;

    beforeEach(async () => {
        await TestBed.configureTestingModule({
            imports: [SettingsPortal],
        }).compileComponents();

        fixture = TestBed.createComponent(SettingsPortal);
        component = fixture.componentInstance;
        await fixture.whenStable();
    });

    it('should create', () => {
        expect(component).toBeTruthy();
    });
});
