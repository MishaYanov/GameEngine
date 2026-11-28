import {ComponentFixture, TestBed} from '@angular/core/testing';
import {type Vector3Change, Vector3Field} from './vector3-field';

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

    it('shows concise resting values while editing and emitting full precision', async () => {
        const value = 180.00000265910896;
        const edited = -179.123456789;
        fixture.componentRef.setInput('compactNumbers', true);
        fixture.componentRef.setInput('value', [value, -3.123456789e-17, 6.012345e20]);
        await fixture.whenStable();
        const field = x();
        expect(field.value).toBe('180');
        expect(field.title).toBe(String(value));
        const display = [...(fixture.nativeElement as HTMLElement).querySelectorAll('input')].map(input => input.value);
        expect(display.every(number => number.length <= 8)).toBe(true);
        expect(Number(display[1])).not.toBe(0);

        const values: Vector3Change[] = [];
        fixture.componentInstance.valueChanged.subscribe(change => values.push(change));
        field.focus();
        await fixture.whenStable();
        expect(field.value).toBe(String(value));
        expect(values).toEqual([]);

        field.value = String(edited);
        field.dispatchEvent(new Event('input'));
        fixture.componentRef.setInput('value', [edited, -3.123456789e-17, 6.012345e20]);
        await fixture.whenStable();
        expect(values).toEqual([{axis: 0, value: edited}]);
        expect(field.value).toBe(String(edited));
        field.blur();
        await fixture.whenStable();
        expect(field.value).toBe('-179.123');
        expect(field.title).toBe(String(edited));
    });

    it('opts into an accessible link toggle and keeps it disabled with the field', async () => {
        const host: HTMLElement = fixture.nativeElement;
        const findToggle = () => host.querySelector<HTMLButtonElement>('button[aria-label="Link Position axes"]');
        expect(findToggle()).toBeNull();

        const hint = 'Apply the same offset to every axis.';
        fixture.componentRef.setInput('lockable', true);
        fixture.componentRef.setInput('lockHint', hint);
        await fixture.whenStable();
        const toggle = findToggle()!;
        expect(toggle.getAttribute('aria-pressed')).toBe('false');
        expect(toggle.title).toBe(hint);
        const description = host.ownerDocument.getElementById(toggle.getAttribute('aria-describedby')!);
        expect(description?.textContent).toBe(hint);

        const changes: boolean[] = [];
        fixture.componentInstance.locked.subscribe(locked => changes.push(locked));
        toggle.click();
        await fixture.whenStable();
        expect(toggle.getAttribute('aria-pressed')).toBe('true');
        expect(changes).toEqual([true]);
        expect(x().value).toBe('1');

        fixture.componentRef.setInput('disabled', true);
        await fixture.whenStable();
        expect(toggle.disabled).toBe(true);
        toggle.click();
        expect(changes).toEqual([true]);
    });

    it('commits the active input on focus loss before changing the lock', async () => {
        fixture.componentRef.setInput('lockable', true);
        await fixture.whenStable();
        const toggle: HTMLButtonElement = fixture.nativeElement.querySelector('.axis-lock');
        const changes: string[] = [];
        fixture.componentInstance.editCommitted.subscribe(() => changes.push('commit'));
        fixture.componentInstance.locked.subscribe(() => changes.push('lock'));
        x().focus();
        toggle.focus();
        toggle.click();
        expect(changes).toEqual(['commit', 'lock']);
    });
});
