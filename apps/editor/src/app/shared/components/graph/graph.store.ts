import {computed} from '@angular/core';
import {patchState, signalStore, withComputed, withMethods, withState} from '@ngrx/signals';
import {
    connectionError,
    createGraphNode,
    emptyGraph,
    GraphConnection,
    graphDefinition,
    GraphDocument,
    GraphEndpoint,
    GraphNodeKind,
    GraphPoint,
    GraphValues,
    validGraphValues,
} from './graph.models';

interface GraphState {
    document: GraphDocument;
    selected: string | null;
    error: string | null;
}

export const GraphStore = signalStore(
    withState<GraphState>({document: emptyGraph(), selected: null, error: null}),
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
                if (document.version !== 1 || !['animation', 'shader'].includes(document.domain))
                    return fail('Unsupported graph document.');
                if (new Set(document.nodes.map((node) => node.id)).size !== document.nodes.length)
                    return fail('Node IDs must be unique.');
                for (const node of document.nodes) {
                    const definition = graphDefinition(node.kind);
                    if (
                        !definition ||
                        !node.id ||
                        (definition.domain !== 'both' && definition.domain !== document.domain) ||
                        !Number.isFinite(node.position.x) ||
                        !Number.isFinite(node.position.y) ||
                        !validGraphValues(node.kind, node.values)
                    )
                        return fail('Invalid node for this graph.');
                }
                const accepted: GraphConnection[] = [];
                const ids = new Set<string>();
                const inputs = new Set<string>();
                for (const edge of document.connections) {
                    const key = `${edge.to.node}:${edge.to.port}`;
                    const error = connectionError({...document, connections: accepted}, edge.from, edge.to);
                    if (!edge.id || ids.has(edge.id) || inputs.has(key) || error)
                        return fail(error ?? 'Connections must have unique IDs and input ports.');
                    accepted.push(edge);
                    ids.add(edge.id);
                    inputs.add(key);
                }
                patchState(store, {document: structuredClone(document), selected: null, error: null});
                return true;
            },
            select(id: string | null): void {
                patchState(store, {selected: id && store.nodeById().has(id) ? id : null});
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
                if (definition.domain !== 'both' && definition.domain !== store.document().domain) {
                    fail('This node belongs to another graph type.');
                    return null;
                }
                let id: string;
                do {
                    id = `node-${++nextId}`;
                } while (store.nodeById().has(id));
                const node = createGraphNode(kind, id, position);
                patchState(store, {
                    document: {...store.document(), nodes: [...store.document().nodes, node]},
                    selected: id,
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
                patchState(store, {
                    document: {
                        ...store.document(),
                        nodes: store.document().nodes.map((node) => (node.id === id ? updated : node)),
                    },
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
                patchState(store, {
                    document: {
                        ...store.document(),
                        connections: [
                            ...previous.filter((edge) => edge.to.node !== to.node || edge.to.port !== to.port),
                            {id, from, to},
                        ],
                    },
                    error: null,
                });
                return true;
            },
            disconnect(id: string): void {
                patchState(store, {
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
                });
            },
        };
    }),
);
