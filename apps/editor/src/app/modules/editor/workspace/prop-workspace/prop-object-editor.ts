import {Component, computed, input, linkedSignal, output} from '@angular/core';
import type {EntityKey, Transform} from '../../../../core/editor/editor.models';
import {type Vector3Change, Vector3Field} from '../../../../shared/components/vector3-field/vector3-field';
import type {PropWorkspaceObject} from './prop-workspace.service';

export type PropObjectEdit =
    | {readonly key: EntityKey; readonly name: string}
    | {readonly key: EntityKey; readonly parent: EntityKey | null}
    | {readonly key: EntityKey; readonly transform: Transform};

@Component({
    selector: 'prop-object-editor',
    imports: [Vector3Field],
    template: `
        <fieldset [disabled]="disabled()">
            <legend>Object properties</legend>
            <label>Prop object name
                <input #name [value]="object().name" (change)="rename(name)" type="text"/>
            </label>
            <label>Prop parent
                <select #parent [value]="object().parent ?? ''" (change)="reparent(parent.value)">
                    @if (!requireParent()) {
                        <option value="">Prop root</option>
                    }
                    @for (candidate of parents(); track candidate.key) {
                        <option [value]="candidate.key">{{ candidate.name }}</option>
                    }
                </select>
            </label>
            <section aria-label="Local transform">
                <h4>Transform <small>Local space</small></h4>
                @for (field of fields; track field.key) {
                    <editor-vector3-field [label]="field.label" [unit]="field.key === 'rotation' ? 'degrees' : ''"
                                          [value]="displayTransform()[field.key]" [disabled]="disabled()"
                                          (valueChanged)="change(field.key, $event)" (editCommitted)="commit()"
                                          (editCancelled)="cancel()"/>
                }
            </section>
        </fieldset>
        @if (error(); as message) {
            <p role="alert">{{ message }}</p>
        }
    `,
    styles: `
        fieldset {
            display: grid;
            gap: var(--space-2);
            min-width: 0;
            padding: var(--space-2);
            border: 1px solid var(--color-border);
            margin: 0;
        }
        section {display: grid; gap: var(--space-1);}
        h4 {margin: 0;}
        label {display: grid; gap: var(--space-1);}
        input, select {
            min-width: 0;
            min-height: var(--layout-subject-control-height);
            padding: var(--space-1);
            border: 1px solid var(--color-border);
            border-radius: var(--radius-sm);
            background: var(--color-control);
            color: var(--color-text);
            font: inherit;
        }
        p {color: var(--color-danger);}
    `,
})
export class PropObjectEditor {
    readonly object = input.required<PropWorkspaceObject>();
    readonly objects = input.required<readonly PropWorkspaceObject[]>();
    readonly disabled = input(false);
    readonly requireParent = input(false);
    readonly maxNameBytes = input(1_024);
    readonly edited = output<PropObjectEdit>();

    protected readonly transform = linkedSignal(() => this.object().transform);
    protected readonly displayTransform = computed(() => {
        const transform = this.transform();
        const [x, y, z] = transform.rotation;
        return {...transform, rotation: [x * 180 / Math.PI, y * 180 / Math.PI, z * 180 / Math.PI] as const};
    });
    protected readonly error = linkedSignal({source: this.object, computation: (): string | null => null});
    protected readonly fields = [
        {key: 'position', label: 'Prop local position'},
        {key: 'rotation', label: 'Prop local rotation'},
        {key: 'scale', label: 'Prop local scale'},
    ] as const;
    protected readonly parents = computed(() => {
        const key = this.object().key;
        const objects = this.objects();
        const blocked = new Set([key]);
        // Input is in parent-first order.
        for (const object of objects) {
            if (object.parent !== null && blocked.has(object.parent)) blocked.add(object.key);
        }
        return objects.filter(object => !blocked.has(object.key));
    });

    protected rename(input: HTMLInputElement): void {
        if (this.disabled()) return;
        const name = input.value.trim();
        if (!name || /\p{Cc}/u.test(name) || new TextEncoder().encode(name).length > this.maxNameBytes()) {
            input.value = this.object().name;
            this.error.set(`Use a visible object name up to ${this.maxNameBytes().toLocaleString()} UTF-8 bytes.`);
            return;
        }
        this.error.set(null);
        if (name !== this.object().name) this.edited.emit({key: this.object().key, name});
    }

    protected reparent(parent: string): void {
        if (this.disabled() || (!parent && this.requireParent()) ||
            (parent && !this.parents().some(object => object.key === parent))) return;
        const key = parent || null;
        if (key !== this.object().parent) this.edited.emit({key: this.object().key, parent: key});
    }

    protected change(field: keyof Transform, change: Vector3Change): void {
        const authored = field === 'rotation' ? change.value * Math.PI / 180 : change.value;
        if (this.disabled() || !Number.isFinite(Math.fround(authored))) return;
        const value = this.transform();
        const next: [number, number, number] = [...value[field]];
        next[change.axis] = authored;
        this.transform.set({...value, [field]: next});
    }

    protected commit(): void {
        const previous = this.object().transform;
        const transform = this.transform();
        if (this.disabled() || this.fields.every(({key}) => transform[key].every((value, index) => value === previous[key][index]))) return;
        this.edited.emit({key: this.object().key, transform});
    }

    protected cancel(): void {
        this.transform.set(this.object().transform);
    }
}
