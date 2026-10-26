import {Component, signal} from '@angular/core';
import {createGraphNode, GraphDocument, GraphDomain} from './graph.models';
import {EditorGraph} from './graph';

export function exampleGraph(domain: GraphDomain): GraphDocument {
    return domain === 'shader'
        ? {
            version: 1,
            domain,
            nodes: [
                createGraphNode('shader.color', 'color', {x: 40, y: 40}),
                createGraphNode('shader.time', 'time', {x: 40, y: 350}),
                createGraphNode('shader.multiply', 'multiply', {x: 390, y: 160}),
                createGraphNode('shader.output', 'material', {x: 740, y: 190}),
            ],
            connections: [
                {
                    id: 'color-result',
                    from: {node: 'color', port: 'color'},
                    to: {node: 'multiply', port: 'value'},
                },
                {
                    id: 'time-factor',
                    from: {node: 'time', port: 'time'},
                    to: {node: 'multiply', port: 'factor'},
                },
                {
                    id: 'material-color',
                    from: {node: 'multiply', port: 'result'},
                    to: {node: 'material', port: 'color'},
                },
            ],
        }
        : {
            version: 1,
            domain,
            nodes: [
                createGraphNode('animation.clip', 'walk', {x: 40, y: 40}),
                {
                    ...createGraphNode('animation.clip', 'run', {x: 40, y: 360}),
                    values: {clip: 'Run', speed: 1.4, loop: true},
                },
                createGraphNode('animation.blend', 'blend', {x: 390, y: 160}),
                createGraphNode('animation.output', 'output', {x: 740, y: 190}),
            ],
            connections: [
                {
                    id: 'walk-blend',
                    from: {node: 'walk', port: 'pose'},
                    to: {node: 'blend', port: 'a'},
                },
                {
                    id: 'run-blend',
                    from: {node: 'run', port: 'pose'},
                    to: {node: 'blend', port: 'b'},
                },
                {
                    id: 'blend-output',
                    from: {node: 'blend', port: 'pose'},
                    to: {node: 'output', port: 'pose'},
                },
            ],
        };
}

@Component({
    imports: [EditorGraph],
    template: `
		<div class="domains" role="group" aria-label="Graph examples">
			<button
				type="button"
				[attr.aria-pressed]="document().domain === 'shader'"
				(click)="show('shader')"
			>
				Shader graph
			</button>
			<button
				type="button"
				[attr.aria-pressed]="document().domain === 'animation'"
				(click)="show('animation')"
			>
				Animation graph
			</button>
		</div>
		<editor-graph
			[(graph)]="document"
			[label]="document().domain === 'shader' ? 'Surface material' : 'Locomotion'"
		/>
	`,
    styles: `
		:host {
			display: flex;
			flex-direction: column;
			gap: var(--space-3);
			height: var(--layout-graph-preview-height);
		}
		editor-graph {
			flex: 1;
			min-height: 0;
		}
		.domains {
			display: flex;
			gap: var(--space-2);
		}
		button {
			min-height: var(--layout-subject-control-height);
			padding: var(--space-1) var(--space-3);
			border: var(--border-width) solid var(--color-graph-node-edge);
			border-radius: var(--radius-sm);
			background: var(--color-graph-node);
			color: var(--color-text-secondary);
			cursor: var(--cursor-action);
			font-size: var(--font-size-sm);
		}
		button[aria-pressed='true'] {
			color: var(--color-text);
			border-color: var(--color-action);
		}
	`,
})
class GraphPreview {
    readonly document = signal(exampleGraph('shader'));
    private readonly documents = new Map<GraphDomain, GraphDocument>();

    show(domain: GraphDomain): void {
        this.documents.set(this.document().domain, this.document());
        this.document.set(this.documents.get(domain) ?? exampleGraph(domain));
    }
}

export default {component: GraphPreview};
