import {Component, computed, effect, ElementRef, inject, input, linkedSignal, output,} from '@angular/core';
import {form} from '@angular/forms/signals';
import {EditorIcon} from '../../icons/editor-icon';
import {SubjectFields} from '../subject/subject';
import {graphDefinition, GraphEndpoint, GraphNode, graphPorts, GraphValues} from './graph.models';

export interface GraphPortEvent {
    readonly endpoint: GraphEndpoint;
    readonly direction: 'input' | 'output';
}

export interface GraphPortPointer extends GraphPortEvent {
    readonly event: PointerEvent;
}

@Component({
    selector: 'editor-graph-values',
    imports: [SubjectFields],
    template: '<editor-subject-fields [fields]="fields" [path]="title()" [options]="options()" />',
    host: {class: 'node-values'},
})
export class GraphNodeValuesEditor {
    readonly node = input.required<GraphNode>();
    readonly title = input.required<string>();
    readonly changed = output<GraphValues>();
    private readonly values = linkedSignal(() => this.node().values);
    protected readonly fields = form(this.values);
    protected readonly options = computed(() => graphDefinition(this.node().kind).options ?? {});

    constructor() {
        effect(() => {
            const values = this.values();
            if (values !== this.node().values) this.changed.emit(values);
        });
    }
}

@Component({
    selector: 'editor-graph-node',
    imports: [EditorIcon, GraphNodeValuesEditor],
    template: `
		<header>
			<button
				type="button"
				data-node-handle
                [attr.aria-label]="'Move ' + title() + ' ' + node().id"
				(pointerdown)="moveStart.emit($event)"
			>
				<editor-icon [name]="definition().category === 'Output' ? 'layers' : 'graph'" />
                <span>{{ title() }}</span>
			</button>
			<span class="category">{{ definition().category }}</span>
		</header>
		<div class="ports">
			<div class="input-ports">
                @for (port of inputs(); track port.id) {
					<button
						type="button"
						class="port"
						data-port-direction="input"
						[attr.data-port-id]="port.id"
						[attr.data-type]="port.type"
						[class.connected]="connected().has(node().id + ':input:' + port.id)"
						[attr.aria-label]="node().id + ' ' + port.label + ' input, ' + port.type"
						[title]="port.type"
						(click)="
							portClick.emit({ endpoint: { node: node().id, port: port.id }, direction: 'input' })
						"
					>
						{{ port.label }}
					</button>
				}
			</div>
			<div class="output-ports">
                @for (port of outputs(); track port.id) {
					<button
						type="button"
						class="port"
						data-port-direction="output"
						[attr.data-port-id]="port.id"
						[attr.data-type]="port.type"
						[class.connected]="connected().has(node().id + ':output:' + port.id)"
						[attr.aria-label]="node().id + ' ' + port.label + ' output, ' + port.type"
						[title]="port.type"
						(pointerdown)="
							portPointer.emit({
								event: $event,
								endpoint: { node: node().id, port: port.id },
								direction: 'output',
							})
						"
						(click)="
							portClick.emit({ endpoint: { node: node().id, port: port.id }, direction: 'output' })
						"
					>
						{{ port.label }}
					</button>
				}
			</div>
		</div>
        @if (node().kind === 'animation.locomotion') {
            <div class="composite-summary">
                <span>{{ summary() }}</span>
                <button type="button" (click)="inspect.emit()">Configure locomotion</button>
                <button type="button" (click)="openMachine.emit()">Open state machine</button>
            </div>
        } @else if (stateMotion(); as motion) {
            <p class="state-motion">{{ motion }}</p>
        } @else if (hasValues()) {
			<editor-graph-values
				[node]="node()"
				[title]="definition().title"
				(changed)="valuesChange.emit($event)"
			/>
		}
	`,
    styleUrl: './graph-node.scss',
    host: {
        role: 'group',
        '[attr.aria-label]': 'definition().title + " " + node().id',
        '[attr.data-node]': 'node().id',
        '[attr.data-domain]': 'definition().domain',
        '[attr.data-kind]': 'node().kind',
        '[class.selected]': 'selected()',
        '[style.left.px]': 'node().position.x',
        '[style.top.px]': 'node().position.y',
        '(click)': 'selectedChange.emit(node().id)',
    },
})
export class GraphNodeCard {
    readonly node = input.required<GraphNode>();
    readonly selected = input(false);
    readonly connected = input.required<ReadonlySet<string>>();
    readonly selectedChange = output<string>();
    readonly moveStart = output<PointerEvent>();
    readonly portPointer = output<GraphPortPointer>();
    readonly portClick = output<GraphPortEvent>();
    readonly valuesChange = output<GraphValues>();
    readonly inspect = output<void>();
    readonly openMachine = output<void>();
    readonly element = inject<ElementRef<HTMLElement>>(ElementRef).nativeElement;
    protected readonly definition = computed(() => graphDefinition(this.node().kind));
    protected readonly hasValues = computed(() => Object.keys(this.node().values).length > 0);
    protected readonly stateMotion = computed(() => {
        const node = this.node();
        return node.kind === 'animation.state' ? node.values.motion : null;
    });
    protected readonly inputs = computed(() => graphPorts(this.node(), 'input'));
    protected readonly outputs = computed(() => graphPorts(this.node(), 'output'));
    protected readonly title = computed(() => {
        const node = this.node();
        return node.kind === 'animation.state' ? node.values.name : this.definition().title;
    });
    protected readonly summary = computed(() => {
        const node = this.node();
        return node.kind === 'animation.locomotion'
            ? `${node.values.blend.mode === 'direction2d' ? '2D directional' : node.values.blend.mode === 'speed1d' ? '1D speed' : 'Single motion'} · ${Object.values(node.values.motions).filter(Boolean).length} clips`
            : '';
    });
}
