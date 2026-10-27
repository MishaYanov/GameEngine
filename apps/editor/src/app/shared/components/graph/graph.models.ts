import {ANIMATION_NODES} from './animation/animation.nodes';
import {
    AnimationNodeValues,
    MOTION_SLOTS,
    slotVisible,
    TransitionSettings,
    validTransition,
} from './animation/animation.models';

export type GraphDomain = 'animation' | 'shader';
export type GraphMode = 'pose' | 'stateMachine';
export type GraphPortType =
    | 'float'
    | 'int'
    | 'bool'
    | 'trigger'
    | 'vector3'
    | 'pose'
    | 'rootMotion'
    | 'events'
    | 'transition';

export interface GraphPoint {
    readonly x: number;
    readonly y: number;
}

export interface GraphPort {
    readonly id: string;
    readonly label: string;
    readonly type: GraphPortType;
}

export interface GraphNodeValues extends AnimationNodeValues {
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

interface ConnectionEndpoints {
    readonly id: string;
    readonly from: GraphEndpoint;
    readonly to: GraphEndpoint;
}

export type GraphConnection = ConnectionEndpoints &
    (
        | { readonly kind?: 'data' }
        | { readonly kind: 'transition'; readonly settings: TransitionSettings }
        );

export interface GraphDocument {
    readonly version: 1;
    readonly domain: GraphDomain;
    readonly kind?: GraphMode;
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
    readonly graph?: GraphMode;
    readonly options?: Readonly<Record<string, readonly string[]>>;
}

export const GRAPH_NODES = {
    ...ANIMATION_NODES,
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
    return {id, kind, position, values: structuredClone(GRAPH_NODES[kind].defaults)} as Extract<
        GraphNode,
        { kind: K }
    >;
}

export function validGraphValues(kind: GraphNodeKind, values: GraphValues): boolean {
    const definition = graphDefinition(kind);
    if (!definition || !values || typeof values !== 'object') return false;
    if (kind === 'animation.locomotion') {
        const loco = values as GraphNodeValues['animation.locomotion'];
        const {machine: _defaultMachine, ...defaults} =
            ANIMATION_NODES['animation.locomotion'].defaults;
        const {machine, ...fields} = loco;
        return (
            sameShape(fields, defaults) &&
            ['single', 'speed1d', 'direction2d'].includes(loco.blend.mode) &&
            ['in_place', 'extract'].includes(loco.rootMotion) &&
            loco.blend.walkSpeed > 0 &&
            loco.blend.runSpeed > loco.blend.walkSpeed &&
            machine?.domain === 'animation' &&
            machine.kind === 'stateMachine' &&
            graphDocumentError(machine) === null
        );
    }
    return (
        sameShape(values, definition.defaults) &&
        Object.entries(definition.options ?? {}).every(([name, choices]) =>
            choices.includes(Reflect.get(values, name)),
        ) &&
        (kind !== 'parameter.int' || Number.isInteger(Reflect.get(values, 'value'))) &&
        (kind !== 'animation.select' || [0, 1].includes(Reflect.get(values, 'index'))) &&
        (kind !== 'animation.state' || !!Reflect.get(values, 'name').trim()) &&
        (kind !== 'animation.blend1d' ||
            (Reflect.get(values, 'thresholdA') < Reflect.get(values, 'thresholdB') &&
                Reflect.get(values, 'thresholdB') < Reflect.get(values, 'thresholdC'))) &&
        (kind !== 'animation.mixer' ||
            (['weightA', 'weightB', 'weightC'].every((key) => Reflect.get(values, key) >= 0) &&
                ['weightA', 'weightB', 'weightC'].some((key) => Reflect.get(values, key) > 0))) &&
        (kind !== 'math.remap' || Reflect.get(values, 'inputMin') !== Reflect.get(values, 'inputMax'))
    );
}

function sameShape(value: unknown, shape: unknown): boolean {
    if (typeof shape === 'number') return typeof value === 'number' && Number.isFinite(value);
    if (shape === null || typeof shape !== 'object') return typeof value === typeof shape;
    if (!value || typeof value !== 'object' || Array.isArray(value)) return false;
    return (
        Object.keys(value).length === Object.keys(shape).length &&
        Object.entries(shape).every(([key, item]) => sameShape(Reflect.get(value, key), item))
    );
}

export function graphPorts(node: GraphNode, direction: 'input' | 'output'): readonly GraphPort[] {
    const definition = graphDefinition(node.kind);
    const ports = direction === 'input' ? definition.inputs : definition.outputs;
    if (node.kind !== 'animation.locomotion') return ports;
    const values = node.values;
    if (direction === 'input')
        return [
            ...ports,
            ...(values.air.enabled
                ? [{id: 'jump', label: 'Jump request', type: 'trigger' as const}]
                : []),
            ...MOTION_SLOTS.filter((slot) => values.exposed[slot.id] && slotVisible(values, slot)).map(
                (slot) => ({id: `motion.${slot.id}`, label: slot.label, type: 'pose' as const}),
            ),
        ];
    return [
        ...ports,
        ...(values.rootMotion === 'extract'
            ? [{id: 'rootMotion', label: 'Root motion', type: 'rootMotion' as const}]
            : []),
        ...(values.emitEvents ? [{id: 'events', label: 'Events', type: 'events' as const}] : []),
    ];
}

export function nodeAllowed(
    kind: GraphNodeKind,
    document: Pick<GraphDocument, 'domain' | 'kind'>,
): boolean {
    const definition = graphDefinition(kind);
    return (
        !!definition &&
        (definition.domain === 'both' || definition.domain === document.domain) &&
        (definition.graph ?? 'pose') === (document.kind ?? 'pose')
    );
}

export function graphDocumentError(document: GraphDocument): string | null {
    if (
        !document ||
        document.version !== 1 ||
        !['animation', 'shader'].includes(document.domain) ||
        !['pose', 'stateMachine'].includes(document.kind ?? 'pose') ||
        (document.kind === 'stateMachine' && document.domain !== 'animation') ||
        !Array.isArray(document.nodes) ||
        !Array.isArray(document.connections)
    )
        return 'Unsupported graph document.';
    if (new Set(document.nodes.map((node) => node?.id)).size !== document.nodes.length)
        return 'Node IDs must be unique.';
    for (const node of document.nodes)
        if (
            !node ||
            typeof node.id !== 'string' ||
            !nodeAllowed(node.kind, document) ||
            !node.id ||
            !Number.isFinite(node.position?.x) ||
            !Number.isFinite(node.position?.y) ||
            !validGraphValues(node.kind, node.values)
        )
            return 'Invalid node for this graph.';
    if (
        document.kind === 'stateMachine' &&
        document.nodes.filter((node) => node.kind === 'animation.entry').length > 1
    )
        return 'A state machine has one entry node.';
    const accepted: GraphConnection[] = [];
    const ids = new Set<string>();
    const inputs = new Set<string>();
    for (const edge of document.connections) {
        if (
            !edge ||
            typeof edge.id !== 'string' ||
            !edge.from ||
            !edge.to ||
            ![edge.from.node, edge.from.port, edge.to.node, edge.to.port].every(
                (value) => typeof value === 'string' && value.length > 0,
            )
        )
            return 'Invalid graph connection.';
        const key = `${edge.to.node}:${edge.to.port}`;
        const error = connectionError({...document, connections: accepted}, edge.from, edge.to);
        const transition = document.kind === 'stateMachine';
        if (
            !edge.id ||
            ids.has(edge.id) ||
            error ||
            (transition
                ? edge.kind !== 'transition' || !validTransition(edge.settings)
                : (edge.kind !== undefined && edge.kind !== 'data') || inputs.has(key))
        )
            return error ?? 'Invalid or duplicate graph connection.';
        accepted.push(edge);
        ids.add(edge.id);
        if (!transition) inputs.add(key);
    }
    return null;
}

/** Transition loops are valid; dataflow dependencies remain acyclic. */
export function connectionError(
    document: GraphDocument,
    from: GraphEndpoint,
    to: GraphEndpoint,
): string | null {
    const source = document.nodes.find((node) => node.id === from.node);
    const target = document.nodes.find((node) => node.id === to.node);
    const output = source && graphPorts(source, 'output').find((port) => port.id === from.port);
    const input = target && graphPorts(target, 'input').find((port) => port.id === to.port);
    if (!output || !input) return 'Connect an output to an input.';
    if (output.type !== input.type) return `Cannot connect ${output.type} to ${input.type}.`;
    if (document.kind === 'stateMachine') {
        if (output.type !== 'transition') return 'State machines use transition connections.';
        if (
            source?.kind === 'animation.entry' &&
            document.connections.some((edge) => edge.from.node === from.node && edge.to.node !== to.node)
        )
            return 'Entry has one destination. Disconnect its transition first.';
        return null;
    }
    if (output.type === 'transition') return 'Transition connections belong in a state machine.';
    if (source?.id === target?.id) return 'A node cannot connect to itself.';
    const queue = [to.node];
    const visited = new Set<string>();
    while (queue.length) {
        const id = queue.pop()!;
        if (id === from.node) return 'This connection would create a cycle.';
        if (visited.has(id)) continue;
        visited.add(id);
        for (const edge of document.connections)
            if (edge.kind !== 'transition' && edge.from.node === id) queue.push(edge.to.node);
    }
    return null;
}
