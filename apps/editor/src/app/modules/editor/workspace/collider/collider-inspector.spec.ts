import {ComponentFixture, TestBed} from '@angular/core/testing';
import axe from 'axe-core';
import type {Collider} from '../../../../core/editor/editor.models';
import {ColliderInspector} from './collider-inspector';
import type {ColliderInteractionChange} from './collider';

describe('ColliderInspector', () => {
    let fixture: ComponentFixture<ColliderInspector>;
    const sphere: Collider = {kind: 'sphere', radius: 0.5, offset: [0, 0, 0]};

    beforeEach(async () => {
        fixture = TestBed.createComponent(ColliderInspector);
        fixture.componentRef.setInput('editable', true);
        fixture.componentRef.setInput('collider', sphere);
        fixture.componentRef.setInput('contextKey', '1:main:player');
        await fixture.whenStable();
    });

    function radius(): HTMLInputElement {
        return fixture.nativeElement.querySelector('[aria-label="Collider Radius"]');
    }

    function change(raw: string): void {
        radius().dispatchEvent(new FocusEvent('focus'));
        radius().value = raw;
        radius().dispatchEvent(new Event('input'));
    }

    it('preserves incomplete text and cancels on invalid blur', async () => {
        const changes: Collider[] = [];
        let cancelled = 0;
        let committed = 0;
        fixture.componentInstance.colliderChanged.subscribe(value => changes.push(value));
        fixture.componentInstance.editCancelled.subscribe(() => cancelled++);
        fixture.componentInstance.editCommitted.subscribe(() => committed++);
        change('1e-');
        await fixture.whenStable();
        expect(radius().value).toBe('1e-');
        expect(radius().getAttribute('aria-invalid')).toBe('true');
        expect(changes).toEqual([]);
        radius().dispatchEvent(new FocusEvent('blur'));
        await fixture.whenStable();
        expect(cancelled).toBe(1);
        expect(committed).toBe(0);
        expect(radius().value).toBe('0.5');
    });

    it('rejects underflow and nonpositive sizes and commits a corrected decimal with Enter', async () => {
        const changes: Collider[] = [];
        let committed = 0;
        fixture.componentInstance.colliderChanged.subscribe(value => changes.push(value));
        fixture.componentInstance.editCommitted.subscribe(() => committed++);
        for (const raw of ['1e-50', '0', '-2']) change(raw);
        expect(changes).toEqual([]);
        change('1.1');
        radius().dispatchEvent(new KeyboardEvent('keydown', {key: 'Enter', bubbles: true}));
        radius().dispatchEvent(new FocusEvent('blur'));
        expect(changes).toEqual([{...sphere, radius: 1.1}]);
        expect(committed).toBe(1);
    });

    it('cancels Escape once and restores authoritative shape after a rejected choice', async () => {
        let cancelled = 0;
        const shapes: string[] = [];
        fixture.componentInstance.editCancelled.subscribe(() => cancelled++);
        fixture.componentInstance.shapeChanged.subscribe(kind => shapes.push(kind));
        change('2');
        radius().dispatchEvent(new KeyboardEvent('keydown', {key: 'Escape', bubbles: true}));
        radius().dispatchEvent(new FocusEvent('blur'));
        expect(cancelled).toBe(1);
        const select: HTMLSelectElement = fixture.nativeElement.querySelector('select');
        select.value = 'box';
        select.dispatchEvent(new Event('change'));
        expect(shapes).toEqual(['box']);
        expect(select.value).toBe('sphere');
    });

    it('clears drafts on context replacement and disables all authoring fields', async () => {
        change('-');
        fixture.componentRef.setInput('contextKey', '2:main:player');
        fixture.componentRef.setInput('collider', {...sphere, radius: 3});
        await fixture.whenStable();
        expect(radius().value).toBe('3');
        expect(radius().getAttribute('aria-invalid')).toBeNull();
        fixture.componentRef.setInput('editable', false);
        await fixture.whenStable();
        expect([...fixture.nativeElement.querySelectorAll('input,select')].every(element => (element as HTMLInputElement).disabled)).toBe(true);
    });

    it('has accessible controls and invalid feedback for both shapes', async () => {
        for (const value of [sphere, {kind: 'box', offset: [0, 0, 0], half_extents: [0.5, 0.5, 0.5]}] as const) {
            fixture.componentRef.setInput('collider', value);
            await fixture.whenStable();
            fixture.nativeElement.querySelector('details').open = true;
            const report = await axe.run(fixture.nativeElement, {rules: {'color-contrast': {enabled: false}}});
            expect(report.violations.map(violation => violation.id)).toEqual([]);
        }
    });

    it('shows legacy blocking defaults and emits one unsigned layer intent without optimistic changes', async () => {
        const changes: ColliderInteractionChange[] = [];
        fixture.componentInstance.interactionChanged.subscribe(value => changes.push(value));
        const mode: HTMLSelectElement = fixture.nativeElement.querySelectorAll('select')[1];
        const member: HTMLInputElement = fixture.nativeElement.querySelector('[aria-label="Belongs to layer 1"]');
        const top: HTMLInputElement = fixture.nativeElement.querySelector('[aria-label="Belongs to layer 32"]');
        const mask: HTMLInputElement = fixture.nativeElement.querySelector('[aria-label="Detects layer 32"]');
        expect(mode.value).toBe('blocking');
        expect(member.checked).toBe(true);
        expect(top.checked).toBe(false);
        expect(mask.checked).toBe(true);
        mode.value = 'sensor';
        mode.dispatchEvent(new Event('change'));
        expect(mode.value).toBe('blocking');
        top.checked = true;
        top.dispatchEvent(new Event('change'));
        expect(top.checked).toBe(false);
        expect(changes).toEqual([
            {kind: 'sensor', sensor: true},
            {kind: 'layer', field: 'layers', bit: 31, enabled: true},
        ]);
        fixture.componentRef.setInput('collider', {...sphere, interaction: {sensor: true, layers: 0x80000001, mask: 0}});
        await fixture.whenStable();
        expect(mode.value).toBe('sensor');
        expect(top.checked).toBe(true);
        expect(mask.checked).toBe(false);
    });

    it('hides interaction controls without a collider and resets them across contexts', async () => {
        fixture.componentRef.setInput('collider', {...sphere, interaction: {sensor: true, layers: 0, mask: 0}});
        await fixture.whenStable();
        fixture.componentRef.setInput('contextKey', '2:other:player');
        fixture.componentRef.setInput('collider', sphere);
        await fixture.whenStable();
        expect(fixture.nativeElement.querySelectorAll('select')[1].value).toBe('blocking');
        fixture.componentRef.setInput('collider', null);
        await fixture.whenStable();
        expect(fixture.nativeElement.querySelector('details')).toBeNull();
    });

    it('exposes accessible bulk controls, empty-group feedback and read-only behavior', async () => {
        const changes: ColliderInteractionChange[] = [];
        fixture.componentInstance.interactionChanged.subscribe(value => changes.push(value));
        const buttons = () => [...fixture.nativeElement.querySelectorAll('.filter-actions button')] as HTMLButtonElement[];
        expect(buttons().map(button => button.getAttribute('aria-label'))).toEqual([
            'Belong to all layers', 'Belong to no layers', 'Detect all layers', 'Detect no layers',
        ]);
        buttons()[3].click();
        expect(changes).toEqual([{kind: 'filter', field: 'mask', value: 0}]);
        fixture.componentRef.setInput('collider', {...sphere, interaction: {sensor: false, layers: 1, mask: 0}});
        await fixture.whenStable();
        expect(fixture.nativeElement.textContent).toContain('Collision and overlap detection are disabled');
        fixture.componentRef.setInput('editable', false);
        await fixture.whenStable();
        expect(buttons().every(button => button.disabled)).toBe(true);
        buttons()[2].click();
        expect(changes).toHaveLength(1);
    });
});
