import {ComponentFixture, TestBed} from '@angular/core/testing';
import {Vector3Field, type Vector3Change} from './vector3-field';

describe('Vector3Field', () => {
    let fixture: ComponentFixture<Vector3Field>;

    beforeEach(async () => {
        await TestBed.configureTestingModule({imports: [Vector3Field]}).compileComponents();
        fixture = TestBed.createComponent(Vector3Field);
        fixture.componentRef.setInput('label', 'Position');
        fixture.componentRef.setInput('value', [1, 2, 3]);
        await fixture.whenStable();
    });

    function x(): HTMLInputElement {
        return fixture.nativeElement.querySelector('input[aria-label="Position X"]');
    }

    it('keeps incomplete input local and restores the last valid value on blur', async () => {
        const changed: Vector3Change[] = [];
        let committed = 0;
        fixture.componentInstance.valueChanged.subscribe(value => changed.push(value));
        fixture.componentInstance.editCommitted.subscribe(() => committed++);
        const field = x();
        field.dispatchEvent(new FocusEvent('focus'));
        field.value = '';
        field.dispatchEvent(new Event('input'));
        await fixture.whenStable();
        expect(field.getAttribute('aria-invalid')).toBe('true');
        expect(changed).toEqual([]);
        field.dispatchEvent(new FocusEvent('blur'));
        await fixture.whenStable();
        expect(field.value).toBe('1');
        expect(field.getAttribute('aria-invalid')).toBeNull();
        expect(committed).toBe(1);
    });

    it('emits valid values live and commits Enter only once', () => {
        const changed: Vector3Change[] = [];
        let committed = 0;
        fixture.componentInstance.valueChanged.subscribe(value => changed.push(value));
        fixture.componentInstance.editCommitted.subscribe(() => committed++);
        const field = x();
        field.dispatchEvent(new FocusEvent('focus'));
        field.value = '-2.5';
        field.dispatchEvent(new Event('input'));
        field.dispatchEvent(new KeyboardEvent('keydown', {key: 'Enter', bubbles: true}));
        field.dispatchEvent(new FocusEvent('blur'));
        expect(changed).toEqual([{axis: 0, value: -2.5}]);
        expect(committed).toBe(1);
    });

    it('cancels Escape once and stops it from reaching editor shortcuts', () => {
        let cancelled = 0;
        let committed = 0;
        fixture.componentInstance.editCancelled.subscribe(() => cancelled++);
        fixture.componentInstance.editCommitted.subscribe(() => committed++);
        const field = x();
        field.dispatchEvent(new FocusEvent('focus'));
        field.value = '20';
        field.dispatchEvent(new Event('input'));
        const escape = new KeyboardEvent('keydown', {key: 'Escape', bubbles: true, cancelable: true});
        field.dispatchEvent(escape);
        field.dispatchEvent(new FocusEvent('blur'));
        expect(escape.defaultPrevented).toBe(true);
        expect(field.value).toBe('1');
        expect(cancelled).toBe(1);
        expect(committed).toBe(0);
    });

    it('reflects authoritative changes when no field is being edited', async () => {
        fixture.componentRef.setInput('value', [9, 8, 7]);
        await fixture.whenStable();
        expect(x().value).toBe('9');
    });
});
