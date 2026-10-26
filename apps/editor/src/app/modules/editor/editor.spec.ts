import {ComponentFixture, TestBed} from '@angular/core/testing';
import {provideRouter} from '@angular/router';
import axe from 'axe-core';

import {Editor} from './editor';

describe('Editor', () => {
    let component: Editor;
    let fixture: ComponentFixture<Editor>;

    beforeEach(async () => {
        await TestBed.configureTestingModule({
            imports: [Editor],
            providers: [provideRouter([])],
        }).compileComponents();

        fixture = TestBed.createComponent(Editor);
        component = fixture.componentInstance;
        await fixture.whenStable();
    });

    it('should create', () => {
        expect(component).toBeTruthy();
    });

    it('has no structural accessibility violations', async () => {
        // Color contrast needs a rendering browser and is checked in the visual audit.
        const results = await axe.run(fixture.nativeElement, {
            rules: {'color-contrast': {enabled: false}},
        });
        expect(
            results.violations.map((violation) => ({
                id: violation.id,
                nodes: violation.nodes.map((node) => node.target),
            })),
        ).toEqual([]);
    });
});
