import {afterNextRender, Component, computed, ElementRef, inject, Injector, signal} from '@angular/core';
import {
    type ControlActionKind, type ControlActionSample, type ControlButton, type ControlGraph, type ControlNode, type ControlNodeData, type ControlNodeKind,
    CONTROL_NODE_LABELS, MAX_CONTROL_NODES, controlPorts, newControlData,
} from '../../../../core/editor/controls-graph.models';
import {validGameActionId} from '../../../../core/game-input/game-input.models';
import {resolveEditorShortcut} from '../../../../core/editor/editor-keymap';
import {EditorGraph, type GraphCanvasEdit} from '../../../../shared/components/graph/graph';
import {EditorService} from '../../../../core/editor/editor.service';
import {controlCanvas, controlLabel, edgeId} from './control-graph-draft';
import {ControlGraphWorkspaceService} from './control-graph-workspace.service';
import {ControlGraphOutputView} from './control-graph-output';

@Component({
    selector: 'editor-control-graph-workspace',
    imports: [EditorGraph, ControlGraphOutputView],
    templateUrl: './control-graph-workspace.html',
    styleUrl: './control-graph-workspace.scss',
    host: {'(keydown)': 'shortcut($event)'},
})
export class ControlGraphWorkspace {
    protected readonly workspace = inject(ControlGraphWorkspaceService);
    private readonly editor = inject(EditorService);
    private readonly injector = inject(Injector);
    private readonly element = inject<ElementRef<HTMLElement>>(ElementRef).nativeElement;
    protected readonly labels = CONTROL_NODE_LABELS;
    protected readonly options = Object.entries(CONTROL_NODE_LABELS);
    protected readonly maxNodes = MAX_CONTROL_NODES;
    protected readonly label = controlLabel;
    protected readonly ports = controlPorts;
    protected readonly wireId = edgeId;
    protected readonly graphCanvas = computed(() => {
        const graph = this.workspace.graph();
        return graph && controlCanvas(graph);
    });
    protected readonly activeNodes = computed(() => new Set(this.workspace.route()?.active_node_ids ?? []));
    protected readonly selectedNode = computed(() => {
        const selection = this.workspace.selection();
        return selection?.kind === 'node' ? this.workspace.graph()?.nodes.find(node => node.id === selection.id) : null;
    });
    protected readonly selectedEdge = computed(() => {
        const selection = this.workspace.selection();
        return selection?.kind === 'connection' ? this.graphCanvas()?.connections.find(edge => edge.id === selection.id) : null;
    });
    protected readonly outputs = computed(() => this.graphCanvas()?.nodes.flatMap(node =>
        node.kind === 'control.node' && node.values.result ? [{id: node.id, label: node.values.title, type: node.values.result}] : []) ?? []);
    protected readonly issues = computed(() => {
        const issues = new Map<string | null, string[]>();
        for (const issue of this.workspace.issues()) {
            const messages = issues.get(issue.node);
            if (messages) messages.push(issue.message);
            else issues.set(issue.node, [issue.message]);
        }
        return issues;
    });
    protected readonly sampleActions = computed(() => {
        const actions = new Map<string, ControlActionKind>();
        for (const {data} of this.workspace.graph()?.nodes ?? []) {
            if (data.kind === 'action' && validGameActionId(data.action)) actions.set(data.action, data.value);
        }
        return [...actions.entries()];
    });
    private readonly samples = signal<Readonly<Record<string, ControlActionSample>>>({});
    protected readonly sampleRows = computed(() =>
        this.sampleActions().map(([action, kind]) => {
            const sample = this.samples()[action];
            return sample?.kind === kind ? sample : defaultSample(action, kind);
        }),
    );
    protected readonly selectedActor = computed(() => {
        const key = this.workspace.actorKey();
        return this.editor.snapshot()?.document.entities.find(entity => entity.key === key)?.name ?? null;
    });

    protected addNode(kind: string): void {
        if (!Object.hasOwn(CONTROL_NODE_LABELS, kind)) return;
        const id = 'node_' + crypto.randomUUID().replaceAll('-', '').slice(0, 22);
        if (!this.workspace.edit({kind: 'add', node: {id, data: newControlData(kind as ControlNodeKind)}})) return;
        this.workspace.selection.set({kind: 'node', id});
        this.focusNode(id);
    }

    protected changeAction(node: ControlNode, action: string, kind: string): void {
        if (kind !== 'button' && kind !== 'vector2' && kind !== 'delta2') return;
        this.updateData(node, {kind: 'action', action: action.trim(), value: kind});
    }

    protected updateData(node: ControlNode, data: ControlNodeData): void {
        this.workspace.edit({kind: 'update', node: {...node, data}});
    }

    protected wire(from: string, to: string, port: string, revision = this.workspace.revision()): void {
        if (this.workspace.edit({kind: 'wire', from, to, port, revision})) {
            this.workspace.selection.set({kind: 'node', id: to});
        }
    }

    protected remove(id: string, restoreFocus = true): void {
        if (this.workspace.edit({kind: 'remove', id}) && restoreFocus) this.focusNode(this.workspace.selection()?.id);
    }

    protected canvasEdit(edit: GraphCanvasEdit): void {
        if (edit.kind === 'connect') this.wire(edit.from.node, edit.to.node, edit.to.port, edit.revision);
        else if (edit.kind === 'move_node') this.workspace.edit({kind: 'move', id: edit.id, position: edit.position});
        else if (edit.kind === 'remove_node') this.remove(edit.id, false);
        else this.workspace.edit({kind: 'disconnect', edge: edit.id});
    }

    protected name(graph: ControlGraph, id: string): string {
        const data = graph.nodes.find(node => node.id === id)?.data;
        return data ? controlLabel(data) : 'Unconnected';
    }

    protected setButtonSample(action: string, field: keyof ControlButton, value: boolean): void {
        const prior = this.samples()[action];
        const button = prior?.kind === 'button' ? prior : {action, kind: 'button' as const, held: false, pressed: false, released: false};
        this.setSample({...button, [field]: value});
    }

    protected setVectorSample(action: string, kind: 'vector2' | 'delta2', field: 'value' | 'rate', axis: 0 | 1, value: number): void {
        const prior = this.samples()[action];
        const sample = prior?.kind === kind ? prior : {action, kind, value: [0, 0] as const};
        const pair = field === 'rate' && sample.kind === 'delta2' && 'rate' in sample ? sample.rate ?? [0, 0] : sample.value;
        const next = axis === 0 ? [value, pair[1]] as const : [pair[0], value] as const;
        if (field === 'rate') {
            if (kind === 'delta2') this.setSample({...sample, kind, rate: next});
        } else this.setSample({...sample, value: next});
    }

    private setSample(sample: ControlActionSample): void {
        this.samples.update(samples => ({...samples, [sample.action]: sample}));
        this.workspace.invalidateSample();
    }

    protected runProbe(): void {
        void this.workspace.probe(this.sampleRows());
    }

    protected shortcut(event: KeyboardEvent): void {
        if (event.defaultPrevented || (event.target instanceof Element && event.target.closest('input,select,textarea'))) return;
        switch (resolveEditorShortcut(event)) {
            case 'document.save':
                void this.workspace.save();
                break;
            case 'document.undo':
                this.workspace.undo();
                break;
            case 'document.redo':
                this.workspace.redo();
                break;
            default:
                return;
        }
        event.preventDefault();
        event.stopPropagation();
    }

    private focusNode(id?: string): void {
        afterNextRender(() => {
            const item = [...this.element.querySelectorAll<HTMLButtonElement>('[data-control-node]')]
                .find(button => button.dataset['controlNode'] === id);
            (item ?? this.element.querySelector<HTMLButtonElement>('[data-add-control-node]'))?.focus();
        }, {injector: this.injector});
    }
}

function defaultSample(action: string, kind: ControlActionKind): ControlActionSample {
    return kind === 'button' ? {action, kind, held: false, pressed: false, released: false} : {action, kind, value: [0, 0]};
}