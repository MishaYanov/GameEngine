import {Component, effect, input, linkedSignal, output} from '@angular/core';
import {form, FormField, min} from '@angular/forms/signals';
import {EditorSubject} from '../../subject/subject';
import {EditorRow} from '../../row/row';
import {TRANSITION_PARAMETERS, TransitionParameter, TransitionSettings, validTransition,} from './animation.models';

@Component({
    selector: 'editor-transition-inspector',
    imports: [EditorSubject, EditorRow, FormField],
    template: `
        <editor-subject title="Transition" icon="graph" [form]="fields" [renderFields]="false">
            <editor-row [columns]="2">
                <label
                >Duration (s)<input
                        type="number"
                        step="0.01"
                        [formField]="fields.duration"
                        aria-label="Transition duration"
                /></label>
                <label class="check"
                ><input type="checkbox" [formField]="fields.enabled" aria-label="Transition enabled"/>
                    Enabled</label
                >
            </editor-row>
            <label class="check"
            ><input type="checkbox" [formField]="fields.useExitTime" aria-label="Use exit time"/> Use
                exit time</label
            >
            @if (draft().useExitTime) {
                <label
                >Normalized exit time<input
                        type="number"
                        step="0.01"
                        [formField]="fields.exitTime"
                        aria-label="Normalized exit time"
                /></label>
            }
            <p class="hint">Conditions must all match. Earlier transitions have priority.</p>
            @for (condition of fields.conditions; track $index; let index = $index) {
                <div class="condition">
                    <editor-row [columns]="2">
                        <label
                        >Parameter<select
                                [formField]="condition.parameter"
                                [attr.aria-label]="'Condition ' + (index + 1) + ' parameter'"
                                (change)="parameterChanged(index, $event)"
                        >
                            @for (parameter of parameters; track parameter) {
                                <option [value]="parameter">{{ parameter }}</option>
                            }
                        </select></label
                        >
                        <label
                        >Operator<select
                                [formField]="condition.operator"
                                [attr.aria-label]="'Condition ' + (index + 1) + ' operator'"
                        >
                            @for (operator of operators(condition.parameter().value()); track operator) {
                                <option [value]="operator">{{ operator }}</option>
                            }
                        </select></label
                        >
                    </editor-row>
                    @if (!isBoolean(condition.parameter().value())) {
                        <label
                        >Threshold<input
                                type="number"
                                step="any"
                                [formField]="condition.value"
                                [attr.aria-label]="'Condition ' + (index + 1) + ' threshold'"
                        /></label>
                    }
                    <button
                            type="button"
                            (click)="remove(index)"
                            [attr.aria-label]="'Remove condition ' + (index + 1)"
                    >
                        Remove condition
                    </button>
                </div>
            }
            <button type="button" (click)="add()">Add condition</button>
            @if (!valid(draft())) {
                <p class="warning" role="alert">
                    Duration and exit time must be nonnegative finite numbers.
                </p>
            }
        </editor-subject>
        <button type="button" (click)="removed.emit()">Delete transition</button>
    `,
    styleUrl: './animation-inspector.scss',
})
export class TransitionInspector {
    readonly settings = input.required<TransitionSettings>();
    readonly changed = output<TransitionSettings>();
    readonly removed = output<void>();
    protected readonly draft = linkedSignal(() => this.settings());
    protected readonly fields = form(this.draft, (path) => {
        min(path.duration, 0);
        min(path.exitTime, 0);
    });
    protected readonly parameters = TRANSITION_PARAMETERS;
    protected readonly valid = validTransition;

    constructor() {
        effect(() => {
            const value = this.draft();
            if (value !== this.settings() && validTransition(value)) this.changed.emit(value);
        });
    }

    protected isBoolean(parameter: TransitionParameter): boolean {
        return parameter === 'grounded' || parameter === 'jump';
    }

    protected operators(parameter: TransitionParameter): readonly string[] {
        return this.isBoolean(parameter) ? ['true', 'false'] : ['greater', 'less'];
    }

    protected parameterChanged(index: number, event: Event): void {
        const parameter = (event.target as HTMLSelectElement).value as TransitionParameter;
        this.draft.update((value) => ({
            ...value,
            conditions: value.conditions.map((condition, i) =>
                i === index
                    ? {...condition, parameter, operator: this.isBoolean(parameter) ? 'true' : 'greater'}
                    : condition,
            ),
        }));
    }

    protected add(): void {
        this.draft.update((value) => ({
            ...value,
            conditions: [...value.conditions, {parameter: 'grounded', operator: 'true', value: 0}],
        }));
    }

    protected remove(index: number): void {
        this.draft.update((value) => ({
            ...value,
            conditions: value.conditions.filter((_, i) => i !== index),
        }));
    }
}
