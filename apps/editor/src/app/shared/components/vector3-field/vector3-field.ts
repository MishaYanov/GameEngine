import {Component, input, model, output, signal} from '@angular/core';
import {EditorIcon} from '../../icons/editor-icon';

export type Vector3Value = readonly [number, number, number];
export type Vector3Axis = 0 | 1 | 2;

export interface Vector3Change {
    readonly axis: Vector3Axis;
    readonly value: number;
}

let nextVectorId = 0;

function compactNumber(value: number): string {
    const exact = String(value);
    if (exact.length <= 8 || !Number.isFinite(value)) return exact;
    for (let digits = 6; digits >= 1; digits--) {
        const rounded = Number(value.toPrecision(digits));
        if (!Number.isFinite(rounded)) continue;
        const decimal = String(rounded);
        if (decimal.length <= 8) return decimal;
        const scientific = rounded.toExponential().replace('e+', 'e');
        if (scientific.length <= 8) return scientific;
    }
    return exact;
}

/** Numeric presentation and field gestures; the owner decides how values are applied. */
@Component({
    selector: 'editor-vector3-field',
    imports: [EditorIcon],
    templateUrl: './vector3-field.html',
    styleUrl: './vector3-field.scss',
})
export class Vector3Field {
    readonly label = input.required<string>();
    readonly value = input.required<Vector3Value>();
    readonly unit = input('');
    readonly compactNumbers = input(false);
    readonly disabled = input(false);
    readonly lockable = input(false);
    readonly lockHint = input('');
    readonly locked = model(false);
    readonly valueChanged = output<Vector3Change>();
    readonly editStarted = output<void>();
    readonly editCommitted = output<void>();
    readonly editCancelled = output<void>();

    protected readonly axes: readonly {readonly index: Vector3Axis; readonly name: string}[] = [
        {index: 0, name: 'X'},
        {index: 1, name: 'Y'},
        {index: 2, name: 'Z'},
    ];
    private readonly fieldId = `vector3-${nextVectorId++}`;
    protected readonly labelId = `${this.fieldId}-label`;
    protected readonly errorId = `${this.fieldId}-error`;
    protected readonly lockHintId = `${this.fieldId}-lock-hint`;
    protected readonly activeAxis = signal<Vector3Axis | null>(null);
    protected readonly invalidAxis = signal<Vector3Axis | null>(null);
    protected readonly draft = signal('');
    private initialValue = '';

    protected toggleLock(): void {
        if (this.lockable() && !this.disabled()) this.locked.update(locked => !locked);
    }

    protected displayValue(axis: Vector3Axis): string {
        if (this.activeAxis() === axis) return this.draft();
        return this.restingValue(axis);
    }

    protected preciseValue(axis: Vector3Axis): string {
        return String(this.value()[axis]);
    }

    private restingValue(axis: Vector3Axis): string {
        return this.compactNumbers() ? compactNumber(this.value()[axis]) : this.preciseValue(axis);
    }

    protected start(input: HTMLInputElement, axis: Vector3Axis): void {
        if (this.beginAxis(axis)) input.value = this.initialValue;
    }

    private beginAxis(axis: Vector3Axis): boolean {
        if (this.disabled() || this.activeAxis() === axis) return false;
        this.initialValue = this.preciseValue(axis);
        this.activeAxis.set(axis);
        this.draft.set(this.initialValue);
        this.invalidAxis.set(null);
        this.editStarted.emit();
        return true;
    }

    protected change(input: HTMLInputElement, axis: Vector3Axis): void {
        if (this.disabled()) return;
        this.beginAxis(axis);
        this.draft.set(input.value);
        const value = input.valueAsNumber;
        if (input.value.trim() === '' || !Number.isFinite(value)) {
            this.invalidAxis.set(axis);
            return;
        }
        this.invalidAxis.set(null);
        this.valueChanged.emit({axis, value});
    }

    protected finish(input: HTMLInputElement, axis: Vector3Axis): void {
        if (this.activeAxis() !== axis) return;
        this.activeAxis.set(null);
        this.invalidAxis.set(null);
        // An invalid intermediate entry was never emitted. Restore the last valid value.
        input.value = this.restingValue(axis);
        this.editCommitted.emit();
    }

    protected keydown(event: KeyboardEvent, input: HTMLInputElement, axis: Vector3Axis): void {
        if (event.key !== 'Enter' && event.key !== 'Escape') return;
        event.preventDefault();
        event.stopPropagation();
        if (event.key === 'Escape' && this.activeAxis() === axis) {
            this.activeAxis.set(null);
            this.invalidAxis.set(null);
            input.value = this.compactNumbers() ? compactNumber(Number(this.initialValue)) : this.initialValue;
            this.editCancelled.emit();
        } else if (event.key === 'Enter') {
            this.finish(input, axis);
        }
        input.blur();
    }
}
