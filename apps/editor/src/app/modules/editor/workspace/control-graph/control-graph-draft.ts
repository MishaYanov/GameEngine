import {
    type ControlGraph, type ControlInput, type ControlNode, type ControlNodeData, type ControlPort,
    CONTROL_NODE_LABELS, MAX_CONTROL_NODES, controlHasCycle, controlPortType, controlPorts,
} from '../../../../core/editor/controls-graph.models';
import {type GraphDocument, type GraphPoint, connectionError} from '../../../../shared/components/graph/graph.models';

export type ControlEdit =
    | {readonly kind: 'add' | 'update'; readonly node: ControlNode}
    | {readonly kind: 'remove'; readonly id: string}
    | {readonly kind: 'move'; readonly id: string; readonly position: GraphPoint}
    | {readonly kind: 'wire'; readonly from: string; readonly to: string; readonly port: string; readonly revision: number}
    | {readonly kind: 'disconnect'; readonly edge: string};

export function controlCanvas(graph: ControlGraph): GraphDocument {
    const definitions = new Map(graph.nodes.map(node => [node.id, node.data]));
    return {
        version: 1, domain: 'controls',
        nodes: graph.nodes.map((node, index) => ({
            id: node.id, kind: 'control.node', position: graph.layout?.[node.id] ?? defaultPosition(index),
            values: {title: controlLabel(node.data), nodeKind: node.data.kind, result: controlPortType(definitions, node.id) ?? ''},
        })),
        connections: graph.nodes.flatMap(node => controlPorts(node.data)
            .filter(port => compatible(definitions, port))
            .map(port => ({id: edgeId(node.id, port.field), from: {node: port.source, port: 'result'},
                to: {node: node.id, port: port.field}}))),
    };
}

export function editControlGraph(graph: ControlGraph, edit: ControlEdit, revision: number): ControlGraph {
    switch (edit.kind) {
        case 'add':
            if (graph.nodes.length >= MAX_CONTROL_NODES || graph.nodes.some(node => node.id === edit.node.id)) {
                throw new Error('Controls Graph supports 128 nodes with distinct IDs.');
            }
            return {...graph, nodes: [...graph.nodes, edit.node]};
        case 'update': {
            const prior = findNode(graph, edit.node.id);
            if (prior.data.kind !== edit.node.data.kind) throw new Error('Select the same node type to edit.');
            return replaceNode(graph, edit.node.id, edit.node.data);
        }
        case 'remove': {
            findNode(graph, edit.id);
            const layout = {...graph.layout};
            delete layout[edit.id];
            const nodes = graph.nodes.filter(node => node.id !== edit.id).map(node => {
                const ports = controlPorts(node.data).filter(port => port.source === edit.id);
                return ports.length ? {...node, data: {...node.data, ...Object.fromEntries(ports.map(port => [port.field, '']))}} : node;
            });
            return {...graph, nodes, layout};
        }
        case 'move': {
            findNode(graph, edit.id);
            if (!Number.isFinite(edit.position.x) || !Number.isFinite(edit.position.y)) throw new Error('Invalid graph position.');
            const position = {x: coordinate(edit.position.x), y: coordinate(edit.position.y)};
            const prior = graph.layout?.[edit.id] ?? defaultPosition(graph.nodes.findIndex(node => node.id === edit.id));
            return position.x === prior.x && position.y === prior.y ? graph : {...graph, layout: {...graph.layout, [edit.id]: position}};
        }
        case 'wire': {
            if (revision !== edit.revision) throw new Error('The Controls Graph changed while wiring the node.');
            const target = findNode(graph, edit.to);
            const port = controlPorts(target.data).find(port => port.field === edit.port);
            if (!port) throw new Error('Choose an existing input port.');
            const error = connectionError(controlCanvas(graph), {node: edit.from, port: 'result'}, {node: edit.to, port: port.field});
            if (error) throw new Error(error);
            const updated = replaceNode(graph, target.id, {...target.data, [port.field]: edit.from});
            if (controlHasCycle(updated)) throw new Error('This connection would create a cycle.');
            return updated;
        }
        case 'disconnect': {
            const [id, field] = edit.edge.split(':');
            const target = findNode(graph, id);
            const port = controlPorts(target.data).find(port => port.field === field && port.source);
            return port ? replaceNode(graph, id, {...target.data, [port.field]: ''}) : graph;
        }
    }
}

export function controlLabel(data: ControlNodeData): string {
    if (data.kind === 'action') return data.action ? `Action · ${data.action}` : 'Action';
    if (data.kind === 'animator_bool' && data.parameter) return `Animator · ${data.parameter}`;
    return CONTROL_NODE_LABELS[data.kind];
}

export function edgeId(node: string, port: ControlInput): string {
    return `${node}:${port}`;
}

function replaceNode(graph: ControlGraph, id: string, data: ControlNodeData): ControlGraph {
    return {...graph, nodes: graph.nodes.map(node => node.id === id ? {...node, data} : node)};
}

function findNode(graph: ControlGraph, id: string): ControlNode {
    const node = graph.nodes.find(node => node.id === id);
    if (!node) throw new Error('Choose an existing Controls Graph node.');
    return node;
}

function compatible(nodes: ReadonlyMap<string, ControlNodeData>, port: ControlPort): boolean {
    const type = controlPortType(nodes, port.source);
    return !!type && (port.type === type || port.type === 'control_value');
}

function defaultPosition(index: number): GraphPoint {
    return {x: 30 + (index % 3) * 240, y: 25 + Math.floor(index / 3) * 142};
}
function coordinate(value: number): number {
    return Math.max(-100_000, Math.min(100_000, Math.round(value)));
}