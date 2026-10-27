import {Component, signal} from '@angular/core';
import {createGraphNode, GraphDocument, GraphDomain} from '../../../../src/app/shared/components/graph/graph.models';
import {EditorGraph} from '../../../../src/app/shared/components/graph/graph';
import {
    AnimationClipAsset,
    defaultLocomotion
} from '../../../../src/app/shared/components/graph/animation/animation.models';

export const EXAMPLE_CLIPS: readonly AnimationClipAsset[] = [
    {id: 'clip:idle', name: 'Idle', duration: 2, skeletonId: 'humanoid'},
    {id: 'clip:walk', name: 'Walk forward', duration: 1, skeletonId: 'humanoid'},
    {id: 'clip:run', name: 'Run forward', duration: 0.7, skeletonId: 'humanoid'},
    {id: 'clip:jump', name: 'Jump start', duration: 0.3, skeletonId: 'humanoid'},
    {id: 'clip:fall', name: 'Falling loop', duration: 1, skeletonId: 'humanoid'},
    {id: 'clip:land', name: 'Landing', duration: 0.4, skeletonId: 'humanoid'},
];

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
                {
                    ...createGraphNode('parameter.vector', 'velocity', {x: 40, y: 40}),
                    values: {name: 'Local velocity', x: 0, y: 0, z: 2},
                },
                {
                    ...createGraphNode('parameter.bool', 'grounded', {x: 40, y: 350}),
                    values: {name: 'Grounded', value: true},
                },
                {
                    ...createGraphNode('animation.locomotion', 'locomotion', {x: 390, y: 150}),
                    values: {
                        ...defaultLocomotion(),
                        skeletonId: 'humanoid',
                        motions: {
                            ...defaultLocomotion().motions,
                            idle: 'clip:idle',
                            walk: 'clip:walk',
                            run: 'clip:run',
                        },
                    },
                },
                createGraphNode('animation.output', 'output', {x: 740, y: 190}),
            ],
            connections: [
                {
                    id: 'velocity-locomotion',
                    from: {node: 'velocity', port: 'value'},
                    to: {node: 'locomotion', port: 'velocity'},
                },
                {
                    id: 'grounded-locomotion',
                    from: {node: 'grounded', port: 'value'},
                    to: {node: 'locomotion', port: 'grounded'},
                },
                {
                    id: 'locomotion-output',
                    from: {node: 'locomotion', port: 'pose'},
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
                [clipAssets]="clips"
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
    readonly clips = EXAMPLE_CLIPS;
    readonly document = signal(exampleGraph('shader'));
    private readonly documents = new Map<GraphDomain, GraphDocument>();

    show(domain: GraphDomain): void {
        this.documents.set(this.document().domain, this.document());
        this.document.set(this.documents.get(domain) ?? exampleGraph(domain));
    }
}

export default {component: GraphPreview};
