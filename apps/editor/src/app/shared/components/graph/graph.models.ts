export type GraphDomain = 'animation' | 'shader';
export type GraphPortType = 'float' | 'vector3' | 'pose';

export interface GraphPoint {
    readonly x: number;
    readonly y: number;
}

export interface GraphPort {
    readonly id: string;
    readonly label: string;
    readonly type: GraphPortType;
}

export interface GraphNodeValues {
    'animation.clip': { clip: string; speed: number; loop: boolean };
    'animation.blend': { weight: number };
    'animation.speed': { speed: number };
    'animation.output': Record<string, never>;
    'shader.time': { scale: number };
    'shader.color': { red: number; green: number; blue: number };
    'shader.multiply': { factor: number };
    'shader.output': Record<string, never>;
    value: { value: number };
}

export type GraphNodeKind = keyof GraphNodeValues;
export type GraphValues = GraphNodeValues[GraphNodeKind];
export type GraphNode = {
    [K in GraphNodeKind]: {
        readonly id: string;
        readonly kind: K;
        readonly position: GraphPoint;
        readonly values: GraphNodeValues[K];
    };
}[GraphNodeKind];

export interface GraphEndpoint {
    readonly node: string;
    readonly port: string;
}

export interface GraphConnection {
    readonly id: string;
    readonly from: GraphEndpoint;
    readonly to: GraphEndpoint;
}

export interface GraphDocument {
    readonly version: 1;
    readonly domain: GraphDomain;
    readonly nodes: readonly GraphNode[];
    readonly connections: readonly GraphConnection[];
}

export interface GraphNodeDefinition<K extends GraphNodeKind = GraphNodeKind> {
    readonly title: string;
    readonly domain: GraphDomain | 'both';
    readonly category: string;
    readonly inputs: readonly GraphPort[];
    readonly outputs: readonly GraphPort[];
    readonly defaults: GraphNodeValues[K];
}

export const GRAPH_NODES = {
    'animation.clip': {
        title: 'Animation clip',
        domain: 'animation',
        category: 'Source',
        inputs: [],
        outputs: [{id: 'pose', label: 'Pose', type: 'pose'}],
        defaults: {clip: 'Walk', speed: 1, loop: true},
    },
    'animation.blend': {
        title: 'Blend poses',
        domain: 'animation',
        category: 'Blend',
        inputs: [
            {id: 'a', label: 'Pose A', type: 'pose'},
            {id: 'b', label: 'Pose B', type: 'pose'},
            {id: 'weight', label: 'Weight', type: 'float'},
        ],
        outputs: [{id: 'pose', label: 'Pose', type: 'pose'}],
        defaults: {weight: 0.5},
    },
    'animation.speed': {
        title: 'Playback speed',
        domain: 'animation',
        category: 'Modifier',
        inputs: [
            {id: 'pose', label: 'Pose', type: 'pose'},
            {id: 'speed', label: 'Speed', type: 'float'},
        ],
        outputs: [{id: 'pose', label: 'Pose', type: 'pose'}],
        defaults: {speed: 1},
    },
    'animation.output': {
        title: 'Animation output',
        domain: 'animation',
        category: 'Output',
        inputs: [{id: 'pose', label: 'Pose', type: 'pose'}],
        outputs: [],
        defaults: {},
    },
    'shader.time': {
        title: 'Time',
        domain: 'shader',
        category: 'Source',
        inputs: [],
        outputs: [{id: 'time', label: 'Time', type: 'float'}],
        defaults: {scale: 1},
    },
    'shader.color': {
        title: 'Color',
        domain: 'shader',
        category: 'Source',
        inputs: [],
        outputs: [{id: 'color', label: 'Color', type: 'vector3'}],
        defaults: {red: 0.25, green: 0.45, blue: 0.95},
    },
    'shader.multiply': {
        title: 'Multiply',
        domain: 'shader',
        category: 'Math',
        inputs: [
            {id: 'value', label: 'Color', type: 'vector3'},
            {id: 'factor', label: 'Factor', type: 'float'},
        ],
        outputs: [{id: 'result', label: 'Result', type: 'vector3'}],
        defaults: {factor: 1},
    },
    'shader.output': {
        title: 'Material output',
        domain: 'shader',
        category: 'Output',
        inputs: [
            {id: 'color', label: 'Base color', type: 'vector3'},
            {id: 'roughness', label: 'Roughness', type: 'float'},
        ],
        outputs: [],
        defaults: {},
    },
    value: {
        title: 'Value',
        domain: 'both',
        category: 'Parameter',
        inputs: [],
        outputs: [{id: 'value', label: 'Value', type: 'float'}],
        defaults: {value: 0.5},
    },
} as const satisfies { [K in GraphNodeKind]: GraphNodeDefinition<K> };

export function graphDefinition(kind: GraphNodeKind): GraphNodeDefinition {
    return GRAPH_NODES[kind];
}

export function emptyGraph(domain: GraphDomain = 'shader'): GraphDocument {
    return {version: 1, domain, nodes: [], connections: []};
}

export function createGraphNode<K extends GraphNodeKind>(
    kind: K,
    id: string,
    position: GraphPoint,
): Extract<GraphNode, { kind: K }> {
    return {id, kind, position, values: {...GRAPH_NODES[kind].defaults}} as Extract<
        GraphNode,
        { kind: K }
    >;
}

export function validGraphValues(kind: GraphNodeKind, values: GraphValues): boolean {
    const defaults = GRAPH_NODES[kind].defaults;
    return (
        Object.keys(values).length === Object.keys(defaults).length &&
        Object.entries(defaults).every(([name, initial]) => {
            const value: unknown = Reflect.get(values, name);
            return (
                typeof value === typeof initial && (typeof value !== 'number' || Number.isFinite(value))
            );
        })
    );
}

/** Validate a connection before replacing the existing input wire. Graphs are acyclic. */
export function connectionError(
    document: GraphDocument,
    from: GraphEndpoint,
    to: GraphEndpoint,
): string | null {
    const source = document.nodes.find((node) => node.id === from.node);
    const target = document.nodes.find((node) => node.id === to.node);
    const output =
        source && graphDefinition(source.kind).outputs.find((port) => port.id === from.port);
    const input = target && graphDefinition(target.kind).inputs.find((port) => port.id === to.port);
    if (!output || !input) return 'Connect an output to an input.';
    if (source?.id === target?.id) return 'A node cannot connect to itself.';
    if (output.type !== input.type) return `Cannot connect ${output.type} to ${input.type}.`;
    const queue = [to.node];
    const visited = new Set<string>();
    while (queue.length) {
        const id = queue.pop()!;
        if (id === from.node) return 'This connection would create a cycle.';
        if (visited.has(id)) continue;
        visited.add(id);
        for (const edge of document.connections) if (edge.from.node === id) queue.push(edge.to.node);
    }
    return null;
}
