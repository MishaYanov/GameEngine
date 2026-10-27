import {computed} from '@angular/core';
import {patchState, signalStore, withComputed, withMethods, withState} from '@ngrx/signals';
import {
    connectionError,
    createGraphNode,
    emptyGraph,
    GraphConnection,
    graphDefinition,
    GraphDocument,
    graphDocumentError,
    GraphEndpoint,
    GraphNodeKind,
    GraphPoint,
    graphPorts,
    GraphValues,
    nodeAllowed,
    validGraphValues,
} from './graph.models';
import {DEFAULT_TRANSITION, TransitionSettings, validTransition,} from './animation/animation.models';

interface GraphState {
    document: GraphDocument;
    selected: string | null;
    selectedConnection: string | null;
    error: string | null;
}

export const GraphStore = signalStore(
    withState<GraphState>({
        document: emptyGraph(),
        selected: null,
        selectedConnection: null,
        error: null,
    }),
    withComputed((store) => ({
        nodeById: computed(() => new Map(store.document().nodes.map((node) => [node.id, node]))),
    })),
    withMethods((store) => {
        let nextId = 0;
        const fail = (error: string) => {
            patchState(store, {error});
            return false;
        };
        return {
            load(document: GraphDocument): boolean {
                const error = graphDocumentError(document);
                if (error) return fail(error);
                patchState(store, {
                    document: structuredClone(document),
                    selected: null,
                    selectedConnection: null,
                    error: null,
                });
                return true;
            },
            select(id: string | null): void {
                patchState(store, {
                    selected: id && store.nodeById().has(id) ? id : null,
                    selectedConnection: null,
                });
            },
            selectConnection(id: string | null): void {
                patchState(store, {
                    selected: null,
                    selectedConnection:
                        id && store.document().connections.some((edge) => edge.id === id) ? id : null,
                });
            },
            setTransition(id: string, settings: TransitionSettings): boolean {
                if (!validTransition(settings)) return fail('Enter valid transition settings.');
                if (
                    !store.document().connections.some((edge) => edge.id === id && edge.kind === 'transition')
                )
                    return fail('Transition not found.');
                patchState(store, {
                    document: {
                        ...store.document(),
                        connections: store
                            .document()
                            .connections.map((edge) =>
                                edge.id === id && edge.kind === 'transition'
                                    ? {...edge, settings: structuredClone(settings)}
                                    : edge,
                            ),
                    },
                    error: null,
                });
                return true;
            },
            clearError(): void {
                patchState(store, {error: null});
            },
            add(kind: GraphNodeKind, position: GraphPoint): string | null {
                const definition = graphDefinition(kind);
                if (!definition || !Number.isFinite(position.x) || !Number.isFinite(position.y)) {
                    fail('Invalid node position.');
                    return null;
                }
                if (!nodeAllowed(kind, store.document())) {
                    fail('This node belongs to another graph type.');
                    return null;
                }
                let id: string;
                do {
                    id = `node-${++nextId}`;
                } while (store.nodeById().has(id));
                const node = createGraphNode(kind, id, position);
                if (
                    kind === 'animation.entry' &&
                    store.document().nodes.some((node) => node.kind === kind)
                ) {
                    fail('A state machine has one entry node.');
                    return null;
                }
                patchState(store, {
                    document: {...store.document(), nodes: [...store.document().nodes, node]},
                    selected: id,
                    selectedConnection: null,
                    error: null,
                });
                return id;
            },
            move(id: string, position: GraphPoint): void {
                const node = store.nodeById().get(id);
                if (
                    !node ||
                    !Number.isFinite(position.x) ||
                    !Number.isFinite(position.y) ||
                    (node.position.x === position.x && node.position.y === position.y)
                )
                    return;
                patchState(store, {
                    document: {
                        ...store.document(),
                        nodes: store
                            .document()
                            .nodes.map((node) => (node.id === id ? {...node, position} : node)),
                    },
                });
            },
            setValues(id: string, values: GraphValues): boolean {
                const node = store.nodeById().get(id);
                if (!node || !validGraphValues(node.kind, values)) return fail('Enter valid node values.');
                const updated = {...node, values: structuredClone(values)} as typeof node;
                const document = {
                    ...store.document(),
                    nodes: store.document().nodes.map((node) => (node.id === id ? updated : node)),
                };
                if (node.kind === 'animation.locomotion') {
                    const inputs = new Set(graphPorts(updated, 'input').map((port) => port.id));
                    const outputs = new Set(graphPorts(updated, 'output').map((port) => port.id));
                    if (
                        document.connections.some(
                            (edge) =>
                                (edge.from.node === id && !outputs.has(edge.from.port)) ||
                                (edge.to.node === id && !inputs.has(edge.to.port)),
                        )
                    )
                        return fail('Disconnect the affected ports before hiding them.');
                }
                patchState(store, {
                    document,
                    error: null,
                });
                return true;
            },
            connect(from: GraphEndpoint, to: GraphEndpoint): boolean {
                const error = connectionError(store.document(), from, to);
                if (error) return fail(error);
                const previous = store.document().connections;
                if (
                    previous.some(
                        (edge) =>
                            edge.from.node === from.node &&
                            edge.from.port === from.port &&
                            edge.to.node === to.node &&
                            edge.to.port === to.port,
                    )
                )
                    return true;
                let id: string;
                do {
                    id = `wire-${++nextId}`;
                } while (previous.some((edge) => edge.id === id));
                const edge: GraphConnection =
                    store.document().kind === 'stateMachine'
                        ? {id, kind: 'transition', from, to, settings: structuredClone(DEFAULT_TRANSITION)}
                        : {id, kind: 'data', from, to};
                patchState(store, {
                    document: {
                        ...store.document(),
                        connections: [
                            ...previous.filter(
                                (edge) =>
                                    store.document().kind === 'stateMachine' ||
                                    edge.to.node !== to.node ||
                                    edge.to.port !== to.port,
                            ),
                            edge,
                        ],
                    },
                    error: null,
                });
                return true;
            },
            disconnect(id: string): void {
                patchState(store, {
                    selectedConnection: store.selectedConnection() === id ? null : store.selectedConnection(),
                    document: {
                        ...store.document(),
                        connections: store.document().connections.filter((edge) => edge.id !== id),
                    },
                });
            },
            remove(id: string): void {
                patchState(store, {
                    document: {
                        ...store.document(),
                        nodes: store.document().nodes.filter((node) => node.id !== id),
                        connections: store
                            .document()
                            .connections.filter((edge) => edge.from.node !== id && edge.to.node !== id),
                    },
                    selected: store.selected() === id ? null : store.selected(),
                    selectedConnection: null,
                });
            },
        };
    }),
);
