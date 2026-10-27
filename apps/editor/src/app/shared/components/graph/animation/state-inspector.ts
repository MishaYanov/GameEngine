import {Component, effect, input, linkedSignal, output} from '@angular/core';
import {form, required} from '@angular/forms/signals';
import {EditorSubject} from '../../subject/subject';
import {GraphNodeValues, validGraphValues} from '../graph.models';

@Component({
    selector: 'editor-state-inspector',
    imports: [EditorSubject],
    template: '<editor-subject title="State" icon="graph" [form]="fields" [options]="options" />',
})
export class StateInspector {
    readonly values = input.required<GraphNodeValues['animation.state']>();
    readonly changed = output<GraphNodeValues['animation.state']>();
    protected readonly options = {
        motion: ['grounded', 'jump', 'rise', 'fall', 'land', 'reference'],
    };
    private readonly draft = linkedSignal(() => this.values());
    protected readonly fields = form(this.draft, (path) =>
        required(path.name, {message: 'Enter a state name.'}),
    );

    constructor() {
        effect(() => {
            const value = this.draft();
            if (value !== this.values() && validGraphValues('animation.state', value))
                this.changed.emit(value);
        });
    }
}
