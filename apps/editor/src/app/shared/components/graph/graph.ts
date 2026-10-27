import {
    afterNextRender,
    afterRenderEffect,
    Component,
    computed,
    DestroyRef,
    DOCUMENT,
    effect,
    ElementRef,
    forwardRef,
    inject,
    Injector,
    input,
    linkedSignal,
    model,
    signal,
    untracked,
    viewChild,
    viewChildren,
} from '@angular/core';
import {form, FormField} from '@angular/forms/signals';
import {EditorIcon} from '../../icons/editor-icon';
import {GraphNodeCard, GraphPortEvent, GraphPortPointer} from './graph-node';
import {
    emptyGraph,
    GRAPH_NODES,
    graphDefinition,
    GraphDocument,
    GraphEndpoint,
    GraphNodeKind,
    GraphPoint,
    graphPorts,
    nodeAllowed,
} from './graph.models';
import {GraphStore} from './graph.store';
import {EditorDialog} from '../dialog/dialog';
import {Drawer} from '../drawer/drawer';
import {EditorPane} from '../pane/pane';
import {DrawerSettings} from '../drawer/drawer-settings';
import {Config} from '../../models/config.model';
import {AnimationClipAsset} from './animation/animation.models';
import {LocomotionInspector} from './animation/locomotion-inspector';
import {TransitionInspector} from './animation/transition-inspector';
import {StateInspector} from './animation/state-inspector';

interface GraphView {
    readonly x: number;
    readonly y: number;
    readonly zoom: number;
}

interface GraphGesture {
    readonly mode: 'pan' | 'node' | 'wire';
    readonly node?: string;
    readonly pointer: number;
    readonly target: HTMLElement;
    x: number;
    y: number;
}

let nextGraphId = 0;

@Component({
    selector: 'editor-graph',
    imports: [
        EditorIcon,
        GraphNodeCard,
        FormField,
        LocomotionInspector,
        TransitionInspector,
        StateInspector,
        EditorDialog,
        Drawer,
        EditorPane,
        forwardRef(() => EditorGraph),
    ],
    providers: [GraphStore],
    templateUrl: './graph.html',
    styleUrl: './graph.scss',
})
export class EditorGraph {
    readonly graph = model<GraphDocument>(emptyGraph());
    readonly label = input('Node graph');
    readonly clipAssets = input<readonly AnimationClipAsset[]>([]);
    protected readonly inspectorConfig = signal<Config<DrawerSettings>>({
        width: 'var(--layout-inspector-width)',
        minWidth: '16rem',
        maxWidth: '70%',
    });
    protected readonly selectedLocomotion = computed(() => {
        const node = this.store.nodeById().get(this.store.selected() ?? '');
        return node?.kind === 'animation.locomotion' ? node : null;
    });
    protected readonly selectedState = computed(() => {
        const node = this.store.nodeById().get(this.store.selected() ?? '');
        return node?.kind === 'animation.state' ? node : null;
    });
    protected readonly selectedTransition = computed(() => {
        const edge = this.store
            .document()
            .connections.find((edge) => edge.id === this.store.selectedConnection());
        return edge?.kind === 'transition' ? edge : null;
    });
    protected readonly inspecting = computed(
        () => !!this.selectedLocomotion() || !!this.selectedTransition() || !!this.selectedState(),
    );
    protected readonly machineOpen = signal(false);
    protected readonly machineDocument = signal<GraphDocument>(emptyGraph('animation'));
    protected readonly machineConfig = {
        width: 'var(--layout-graph-machine-width)',
        height: 'var(--layout-graph-machine-height)',
    };
    protected readonly wires = computed(() =>
        this.connections().flatMap((edge) => {
            const from = this.anchor(edge.from, 'output');
            const to = this.anchor(edge.to, 'input');
            const source = this.store.nodeById().get(edge.from.node);
            const port =
                source && graphPorts(source, 'output').find((port) => port.id === edge.from.port);
            return from && to && port
                ? [
                    {
                        ...edge,
                        path: edge.from.node === edge.to.node ? this.loop(from, to) : this.curve(from, to),
                        type: port.type,
                        label: `${edge.kind === 'transition' ? 'Edit transition' : 'Remove connection'} ${edge.from.node} ${edge.from.port} to ${edge.to.node} ${edge.to.port}`,
                    },
                ]
                : [];
        }),
    );
    private readonly machineOwner = signal<string | null>(null);
    protected readonly machineNode = computed(() => {
        const node = this.store.nodeById().get(this.machineOwner() ?? '');
        return node?.kind === 'animation.locomotion' ? node : null;
    });
    protected readonly store = inject(GraphStore);
    protected readonly instructionsId = `graph-instructions-${nextGraphId++}`;
    protected readonly view = signal<GraphView>({x: 40, y: 40, zoom: 1});
    protected readonly percent = computed(() => Math.round(this.view().zoom * 100));
    protected readonly transform = computed(
        () => `translate(${this.view().x}px, ${this.view().y}px) scale(${this.view().zoom})`,
    );
    private readonly portSignature = computed(() =>
        this.store
            .document()
            .nodes.map(
            (node) =>
                `${node.id}:${graphPorts(node, 'input')
                    .map((port) => port.id)
                    .join(',')}:${graphPorts(node, 'output')
                    .map((port) => port.id)
                    .join(',')}`,
        )
            .join('|'),
    );
    private readonly graphContext = computed(
        () => {
            const document = this.store.document();
            return {domain: document.domain, kind: document.kind};
        },
        {equal: (previous, next) => previous.domain === next.domain && previous.kind === next.kind},
    );
    protected readonly definition = graphDefinition;
    protected readonly pending = signal<GraphEndpoint | null>(null);
    protected readonly message = signal('');
    protected readonly interacting = signal(false);
    private readonly chosenKind = linkedSignal(() => this.palette()[0]);
    protected readonly paletteForm = form(this.chosenKind);
    private readonly connections = computed(() => this.store.document().connections);
    protected readonly connected = computed(
        () =>
            new Set(
                this.connections().flatMap((edge) => [
                    `${edge.from.node}:output:${edge.from.port}`,
                    `${edge.to.node}:input:${edge.to.port}`,
                ]),
            ),
    );
    protected readonly palette = computed(() =>
        (Object.keys(GRAPH_NODES) as GraphNodeKind[]).filter((kind) =>
            nodeAllowed(kind, this.graphContext()),
        ),
    );
    private readonly anchors = signal<ReadonlyMap<string, GraphPoint>>(new Map());
    private readonly loaded = signal(0);
    private readonly cursor = signal<GraphPoint>({x: 0, y: 0});
    protected readonly draft = computed(() => {
        const pending = this.pending();
        const from = pending && this.anchor(pending, 'output');
        return from ? this.curve(from, this.cursor()) : null;
    });
    private readonly stage = viewChild<ElementRef<HTMLElement>>('stage');
    private readonly cards = viewChildren(GraphNodeCard);
    private readonly document = inject(DOCUMENT);
    private readonly injector = inject(Injector);
    private readonly window = this.document.defaultView;
    private gesture: GraphGesture | undefined;
    private frame: number | undefined;
    private latest: GraphPoint | undefined;
    private fitOnMeasure = true;
    private suppressPortClick = false;

    constructor() {
        effect(() => {
            const source = this.graph();
            if (source !== untracked(this.store.document)) {
                this.stop();
                if (this.store.load(source)) {
                    this.machineOpen.set(false);
                    this.pending.set(null);
                    this.fitOnMeasure = true;
                    this.loaded.update((revision) => revision + 1);
                } else this.graph.set(untracked(this.store.document));
            }
        });
        effect(() => {
            const value = this.store.document();
            if (value !== untracked(this.graph)) this.graph.set(value);
        });
        afterRenderEffect((onCleanup) => {
            this.loaded();
            this.portSignature();
            const cards = this.cards();
            const observer = untracked(() => {
                const measure = () => this.measure(cards);
                measure();
                if (typeof ResizeObserver === 'undefined') return;
                const observer = new ResizeObserver(measure);
                for (const card of cards) observer.observe(card.element);
                return observer;
            });
            if (observer) onCleanup(() => observer.disconnect());
        });
        afterRenderEffect(() => {
            const inspecting = this.inspecting();
            untracked(() => {
                if (inspecting && !this.gesture) this.fit();
            });
        });
        inject(DestroyRef).onDestroy(() => this.stop());
    }

    protected openMachine(id: string): void {
        const node = this.store.nodeById().get(id);
        if (node?.kind !== 'animation.locomotion') return;
        this.machineDocument.set(node.values.machine);
        this.machineOwner.set(id);
        this.machineOpen.set(true);
    }

    protected saveMachine(document: GraphDocument): void {
        this.machineDocument.set(document);
        const node = this.machineNode();
        if (node && document !== node.values.machine)
            this.store.setValues(node.id, {...node.values, machine: document});
    }

    protected activateWire(id: string): void {
        if (this.store.document().kind === 'stateMachine') this.store.selectConnection(id);
        else this.store.disconnect(id);
    }

    protected closeInspector(): void {
        this.store.select(null);
        this.stage()?.nativeElement.focus();
    }

    protected add(): void {
        const stage = this.stage()?.nativeElement;
        if (!stage) return;
        const view = this.view();
        const offset = (this.store.document().nodes.length % 4) * 24;
        const id = this.store.add(this.chosenKind(), {
            x: (stage.clientWidth / 2 - view.x) / view.zoom + offset,
            y: (stage.clientHeight / 2 - view.y) / view.zoom + offset,
        });
        if (id)
            afterNextRender(
                () =>
                    this.cards()
                        .find((card) => card.node().id === id)
                        ?.element.querySelector<HTMLElement>('[data-node-handle]')
                        ?.focus(),
                {injector: this.injector},
            );
    }

    protected fit(): void {
        const stage = this.stage()?.nativeElement;
        const cards = this.cards();
        if (!stage || !cards.length) {
            this.view.set({x: 40, y: 40, zoom: 1});
            return;
        }
        const left = Math.min(...cards.map((card) => card.node().position.x));
        const top = Math.min(...cards.map((card) => card.node().position.y));
        const right = Math.max(
            ...cards.map((card) => card.node().position.x + card.element.offsetWidth),
        );
        const bottom = Math.max(
            ...cards.map((card) => card.node().position.y + card.element.offsetHeight),
        );
        const zoom = Math.max(
            0.3,
            Math.min(
                1,
                (stage.clientWidth - 80) / Math.max(1, right - left),
                (stage.clientHeight - 80) / Math.max(1, bottom - top),
            ),
        );
        this.view.set({
            x: (stage.clientWidth - (right - left) * zoom) / 2 - left * zoom,
            y: (stage.clientHeight - (bottom - top) * zoom) / 2 - top * zoom,
            zoom,
        });
    }

    protected zoomBy(factor: number, point?: GraphPoint): void {
        const stage = this.stage()?.nativeElement;
        if (!stage) return;
        const origin = point ?? {x: stage.clientWidth / 2, y: stage.clientHeight / 2};
        const view = this.view();
        const zoom = Math.max(0.3, Math.min(2, view.zoom * factor));
        const ratio = zoom / view.zoom;
        this.view.set({
            x: origin.x - (origin.x - view.x) * ratio,
            y: origin.y - (origin.y - view.y) * ratio,
            zoom,
        });
    }

    protected wheel(event: WheelEvent): void {
        event.preventDefault();
        const rect = this.stage()?.nativeElement.getBoundingClientRect();
        if (rect)
            this.zoomBy(Math.exp(-event.deltaY * 0.0015), {
                x: event.clientX - rect.left,
                y: event.clientY - rect.top,
            });
    }

    protected startPan(event: PointerEvent): void {
        if (
            (event.target as Element).closest('editor-graph-node, .wire-hit') ||
            (event.button !== 0 && event.button !== 1)
        )
            return;
        this.pending.set(null);
        this.store.select(null);
        this.start(event, 'pan');
    }

    protected startNode(event: PointerEvent, node: string): void {
        if (event.button === 0) {
            this.store.select(node);
            this.start(event, 'node', node);
        }
    }

    protected startPort(port: GraphPortPointer): void {
        if (port.event.button !== 0) return;
        this.suppressPortClick = false;
        this.pending.set(port.endpoint);
        this.store.clearError();
        this.updateCursor(port.event.clientX, port.event.clientY);
        this.start(port.event, 'wire');
    }

    protected portClick(port: GraphPortEvent): void {
        if (this.suppressPortClick) {
            this.suppressPortClick = false;
            return;
        }
        if (port.direction === 'output') {
            this.pending.set(port.endpoint);
            const point = this.anchor(port.endpoint, 'output');
            if (point) this.cursor.set(point);
            this.message.set('Choose a matching input.');
        } else if (this.pending()) {
            if (this.store.connect(this.pending()!, port.endpoint)) {
                this.pending.set(null);
                this.message.set('Connection added.');
            }
        }
    }

    protected move(event: PointerEvent): void {
        if (this.gesture ? this.gesture.pointer !== event.pointerId : !this.pending()) return;
        this.latest = {x: event.clientX, y: event.clientY};
        if (this.frame === undefined)
            this.frame = this.window?.requestAnimationFrame(() => {
                this.frame = undefined;
                this.flush();
            });
    }

    protected finish(event: PointerEvent): void {
        if (!this.gesture || this.gesture.pointer !== event.pointerId) return;
        this.flush();
        if (event.type === 'pointerup' && this.gesture.mode === 'wire' && this.pending()) {
            const port = this.document
                .elementFromPoint(event.clientX, event.clientY)
                ?.closest<HTMLElement>('[data-port-direction="input"]');
            const node =
                port?.closest('.stage') === this.stage()?.nativeElement
                    ? port?.closest<HTMLElement>('[data-node]')?.dataset['node']
                    : undefined;
            const id = port?.dataset['portId'];
            if (node && id && this.store.connect(this.pending()!, {node, port: id})) {
                this.pending.set(null);
                this.suppressPortClick = true;
                this.message.set('Connection added.');
            }
        }
        if (event.type !== 'pointerup' && this.gesture.mode === 'wire') this.pending.set(null);
        this.stop();
    }

    protected key(event: KeyboardEvent): void {
        if ((event.target as Element).closest('input, select, textarea')) return;
        if (event.key === 'Escape') {
            this.pending.set(null);
            this.store.clearError();
            this.message.set('');
            this.stop();
            return;
        }
        const transition = this.store.selectedConnection();
        if (transition && (event.key === 'Delete' || event.key === 'Backspace')) {
            event.preventDefault();
            this.store.disconnect(transition);
            return;
        }
        const selected =
            (event.target as Element).closest<HTMLElement>('[data-node]')?.dataset['node'] ??
            this.store.selected();
        if (selected && (event.key === 'Delete' || event.key === 'Backspace')) {
            event.preventDefault();
            this.store.remove(selected);
            this.stage()?.nativeElement.focus();
        }
        if (!(event.target as Element).closest('[data-node-handle]') || !selected) return;
        const node = this.store.nodeById().get(selected);
        const step = event.shiftKey ? 40 : 10;
        const dx = event.key === 'ArrowRight' ? step : event.key === 'ArrowLeft' ? -step : 0;
        const dy = event.key === 'ArrowDown' ? step : event.key === 'ArrowUp' ? -step : 0;
        if (node && (dx || dy)) {
            event.preventDefault();
            this.store.select(node.id);
            this.store.move(selected, {x: node.position.x + dx, y: node.position.y + dy});
        }
    }

    protected removeWire(event: KeyboardEvent, id: string): void {
        if (
            event.key === 'Enter' ||
            event.key === 'Delete' ||
            event.key === 'Backspace' ||
            event.key === ' '
        ) {
            event.preventDefault();
            event.stopPropagation();
            if (event.key === 'Delete' || event.key === 'Backspace') {
                this.store.disconnect(id);
                this.stage()?.nativeElement.focus();
            } else this.activateWire(id);
        }
    }

    protected deleteSelected(): void {
        const selected = this.store.selected();
        if (selected) {
            this.store.remove(selected);
            this.stage()?.nativeElement.focus();
        }
    }

    private measure(cards: readonly GraphNodeCard[]): void {
        const scale = this.view().zoom;
        const anchors = new Map<string, GraphPoint>();
        for (const card of cards) {
            const rect = card.element.getBoundingClientRect();
            for (const port of card.element.querySelectorAll<HTMLElement>('[data-port-id]')) {
                const bounds = port.getBoundingClientRect();
                const output = port.dataset['portDirection'] === 'output';
                const style = this.window?.getComputedStyle(port);
                const padding = Number.parseFloat(
                    output ? (style?.paddingRight ?? '0') : (style?.paddingLeft ?? '0'),
                );
                const dot =
                    Number.parseFloat(
                        this.window?.getComputedStyle(port, output ? '::after' : '::before').width ?? '8',
                    ) || 8;
                anchors.set(
                    `${card.node().id}:${port.dataset['portDirection']}:${port.dataset['portId']}`,
                    {
                        x: output
                            ? (bounds.right - rect.left) / scale - padding - dot / 2
                            : (bounds.left - rect.left) / scale + padding + dot / 2,
                        y: (bounds.top + bounds.height / 2 - rect.top) / scale,
                    },
                );
            }
        }
        this.anchors.set(anchors);
        if (this.fitOnMeasure && cards.length) {
            this.fitOnMeasure = false;
            this.fit();
        }
    }

    private anchor(endpoint: GraphEndpoint, direction: 'input' | 'output'): GraphPoint | null {
        const node = this.store.nodeById().get(endpoint.node);
        const offset = this.anchors().get(`${endpoint.node}:${direction}:${endpoint.port}`);
        return node && offset ? {x: node.position.x + offset.x, y: node.position.y + offset.y} : null;
    }

    private curve(from: GraphPoint, to: GraphPoint): string {
        const bend = Math.max(60, Math.abs(to.x - from.x) * 0.5);
        return `M ${from.x} ${from.y} C ${from.x + bend} ${from.y}, ${to.x - bend} ${to.y}, ${to.x} ${to.y}`;
    }

    private loop(from: GraphPoint, to: GraphPoint): string {
        return `M ${from.x} ${from.y} C ${from.x + 100} ${from.y - 160}, ${to.x - 100} ${to.y - 160}, ${to.x} ${to.y}`;
    }

    private start(event: PointerEvent, mode: GraphGesture['mode'], node?: string): void {
        this.stop();
        const target = event.currentTarget as HTMLElement;
        target.setPointerCapture(event.pointerId);
        this.gesture = {
            mode,
            node,
            pointer: event.pointerId,
            target,
            x: event.clientX,
            y: event.clientY,
        };
        this.interacting.set(true);
        event.preventDefault();
    }

    private flush(): void {
        const gesture = this.gesture;
        const point = this.latest;
        this.latest = undefined;
        if (!point) return;
        if (!gesture) {
            if (this.pending()) this.updateCursor(point.x, point.y);
            return;
        }
        const dx = point.x - gesture.x;
        const dy = point.y - gesture.y;
        const view = this.view();
        if (gesture.mode === 'pan') this.view.set({...view, x: view.x + dx, y: view.y + dy});
        else if (gesture.mode === 'node' && gesture.node) {
            const node = this.store.nodeById().get(gesture.node);
            if (node)
                this.store.move(node.id, {
                    x: node.position.x + dx / view.zoom,
                    y: node.position.y + dy / view.zoom,
                });
        } else this.updateCursor(point.x, point.y);
        gesture.x = point.x;
        gesture.y = point.y;
    }

    private updateCursor(x: number, y: number): void {
        const rect = this.stage()?.nativeElement.getBoundingClientRect();
        const view = this.view();
        if (rect)
            this.cursor.set({
                x: (x - rect.left - view.x) / view.zoom,
                y: (y - rect.top - view.y) / view.zoom,
            });
    }

    private stop(): void {
        const gesture = this.gesture;
        this.gesture = undefined;
        if (this.frame !== undefined) this.window?.cancelAnimationFrame(this.frame);
        this.frame = undefined;
        this.latest = undefined;
        this.interacting.set(false);
        if (gesture?.target.hasPointerCapture(gesture.pointer))
            gesture.target.releasePointerCapture(gesture.pointer);
    }
}
