import {Component, computed, input, output, signal} from '@angular/core';
import {MAX_AUTHORING_NUMBER_LENGTH, parseF32} from '../../../../core/editor/authoring-number';
import type {SceneEntity} from '../../../../core/editor/editor.models';
import type {TransformField, TransformFieldEdit} from '../scene-editing.service';

const PROPERTIES = [
    {key: 'position', label: 'Position'},
    {key: 'rotation', label: 'Rotation'},
    {key: 'scale', label: 'Scale'},
] as const;
const AXES = ['X', 'Y', 'Z'] as const;
const DEGREES_PER_RADIAN = 180 / Math.PI;

@Component({
    selector: 'editor-batch-transform-inspector',
    templateUrl: './batch-transform-inspector.html',
    styleUrl: './batch-transform-inspector.scss',
})
export class BatchTransformInspector {
    readonly objects = input.required<readonly SceneEntity[]>();
    readonly editable = input(false);
    readonly edited = output<TransformFieldEdit>();
    protected readonly maxLength = MAX_AUTHORING_NUMBER_LENGTH;
    protected readonly error = signal<string | null>(null);
    protected readonly fields = computed(() => {
        const [first, ...remaining] = this.objects();
        return PROPERTIES.map(property => ({
            ...property,
            axes: ([0, 1, 2] as const).map(index => {
                const value = first?.transform[property.key][index];
                const mixed = value !== undefined &&
                    remaining.some(object => object.transform[property.key][index] !== value);
                return {index, label: AXES[index], mixed,
                    display: value === undefined || mixed ? '' :
                        String(property.key === 'rotation' ? value * DEGREES_PER_RADIAN : value)};
            }),
        }));
    });

    protected apply(field: TransformField, axis: 0 | 1 | 2, element: HTMLInputElement, display: string): void {
        const text = element.value.trim();
        this.reset(element, display);
        if (!text || text === display || !this.editable()) return;
        const parsed = parseF32(text);
        const value = parsed === null ? null : field === 'rotation' ? parsed / DEGREES_PER_RADIAN : parsed;
        if (value === null || !Number.isFinite(Math.fround(value)) || value !== 0 && Math.fround(value) === 0) {
            this.error.set('Enter a number that fits the selected transform axis.');
            return;
        }
        this.edited.emit({field, axis, value});
    }

    protected reset(element: HTMLInputElement, display: string): void {
        element.value = display;
        this.error.set(null);
    }
}
