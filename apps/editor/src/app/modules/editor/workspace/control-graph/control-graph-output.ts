import {Component, computed, input} from '@angular/core';
import type {ControlButton, ControlGraphOutput} from '../../../../core/editor/controls-graph.models';

@Component({
    selector: 'editor-control-graph-output',
    template: `
        <dl class="outputs">
            <div><dt>Move</dt><dd>{{ result().intent.movement.join(', ') }}</dd></div>
            <div><dt>Look delta</dt><dd>{{ result().intent.look.delta.join(', ') }}</dd></div>
            <div><dt>Look rate</dt><dd>{{ result().intent.look.rate.join(', ') }}</dd></div>
            <div><dt>Jump</dt><dd>{{ button(result().intent.jump) }}</dd></div>
            <div><dt>Attack</dt><dd>{{ button(result().intent.attack) }}</dd></div>
            @for (parameter of presentation(); track parameter[0]) {
                <div><dt>{{ parameter[0] }}</dt><dd>{{ parameter[1] ? 'True' : 'False' }}</dd></div>
            }
        </dl>
    `,
    styles: `
        .outputs { margin: .25rem 0 .5rem; }
        .outputs > div { display: flex; justify-content: space-between; gap: .5rem; }
        dd { margin: 0; text-align: end; }
    `,
})
export class ControlGraphOutputView {
    readonly result = input.required<ControlGraphOutput>();
    protected readonly presentation = computed(() => Object.entries(this.result().presentation));
    protected button(value: ControlButton): string {
        return [value.pressed ? 'Pressed' : '', value.held ? 'Held' : '', value.released ? 'Released' : '']
            .filter(Boolean).join(', ') || 'Idle';
    }
}