import {validInputActionsId} from '../game-input/input-actions.models';
import {validGameActionId} from '../game-input/game-input.models';

export type ControlActionKind = 'button' | 'vector2' | 'delta2';
export type ControlValueKind = ControlActionKind | 'bool';
export type ControlInput = 'input' | 'condition' | 'left' | 'right';
export type ControlNodeData =
    | {readonly kind: 'action'; readonly action: string; readonly value: ControlActionKind}
    | {readonly kind: 'boolean'; readonly value: boolean}
    | {readonly kind: 'button_held' | 'not' | 'move' | 'look' | 'jump' | 'attack'; readonly input: string}
    | {readonly kind: 'and' | 'or'; readonly left: string; readonly right: string}
    | {readonly kind: 'gate'; readonly condition: string; readonly input: string}
    | {readonly kind: 'animator_bool'; readonly parameter: string; readonly input: string};
export type ControlNodeKind = ControlNodeData['kind'];
export interface ControlNode {
    readonly id: string;
    readonly data: ControlNodeData;
}
export interface ControlGraphPosition { readonly x: number; readonly y: number }
export interface ControlGraph {
    readonly schema_version: 1;
    readonly nodes: readonly ControlNode[];
    readonly layout?: Readonly<Record<string, ControlGraphPosition>>;
}
export interface ControlGraphAsset {
    readonly id: string;
    readonly kind: 'controls_graph';
    readonly path: string;
    readonly revision: number;
    readonly fingerprint: string;
    readonly bytes: number;
}
export type ControlGraphCapture = Pick<ControlGraphAsset, 'id' | 'fingerprint'>;
export interface ControlGraphDocument {
    readonly asset: ControlGraphAsset;
    readonly graph: ControlGraph;
}
export interface ControlButton {
    readonly held: boolean;
    readonly pressed: boolean;
    readonly released: boolean;
}
type Vector = readonly [number, number];
export type ControlActionSample =
    | ({readonly action: string; readonly kind: 'button'} & ControlButton)
    | {readonly action: string; readonly kind: 'vector2'; readonly value: Vector}
    | {readonly action: string; readonly kind: 'delta2'; readonly value: Vector; readonly rate?: Vector};
export interface ControlGraphOutput {
    readonly intent: {
        readonly movement: Vector;
        readonly look: {readonly delta: Vector; readonly rate: Vector};
        readonly jump: ControlButton;
        readonly attack: ControlButton;
    };
    readonly presentation: Readonly<Record<string, boolean>>;
}
export interface ControlGraphProbe {
    readonly order: readonly string[];
    readonly active_node_ids: readonly string[];
    readonly output: ControlGraphOutput;
}
export interface ControlPort {
    readonly field: ControlInput;
    readonly label: string;
    readonly type: ControlValueKind | 'control_value';
    readonly source: string;
}
export interface ControlIssue { readonly node: string | null; readonly message: string }

export const CONTROL_NODE_LABELS: Readonly<Record<ControlNodeKind, string>> = {
    action: 'Action', boolean: 'Boolean', button_held: 'Button Held', not: 'Not', and: 'And', or: 'Or',
    gate: 'Gate', move: 'Move', look: 'Look', jump: 'Jump', attack: 'Attack', animator_bool: 'Animator Boolean',
};
export const MAX_CONTROL_NODES = 128;
const maxBytes = 128 * 1024;
const encoder = new TextEncoder();
const actionTypes: readonly ControlActionKind[] = ['button', 'vector2', 'delta2'];

export function emptyControlGraph(): ControlGraph {
    return {schema_version: 1, nodes: []};
}

export function controlPath(value: string): boolean {
    return value.toLowerCase().endsWith('.controls.json') && encoder.encode(value).length <= 4096 &&
        !/[:\\\p{Cc}]/u.test(value) && value.split('/').every(part => !!part && part !== '.' && part !== '..');
}

export function newControlData(kind: ControlNodeKind): ControlNodeData {
    switch (kind) {
        case 'action': return {kind, action: 'jump', value: 'button'};
        case 'boolean': return {kind, value: true};
        case 'and':
        case 'or': return {kind, left: '', right: ''};
        case 'gate': return {kind, condition: '', input: ''};
        case 'animator_bool': return {kind, parameter: '', input: ''};
        default: return {kind, input: ''};
    }
}
export function controlPorts(data: ControlNodeData): readonly ControlPort[] {
    switch (data.kind) {
        case 'action':
        case 'boolean': return [];
        case 'and':
        case 'or': return [
            {field: 'left', label: 'Left', type: 'bool', source: data.left},
            {field: 'right', label: 'Right', type: 'bool', source: data.right},
        ];
        case 'gate': return [
            {field: 'condition', label: 'Condition', type: 'bool', source: data.condition},
            {field: 'input', label: 'Value', type: 'control_value', source: data.input},
        ];
        case 'button_held':
        case 'attack':
        case 'jump': return [{field: 'input', label: 'Button', type: 'button', source: data.input}];
        case 'look': return [{field: 'input', label: 'Look', type: 'delta2', source: data.input}];
        case 'move': return [{field: 'input', label: 'Movement', type: 'vector2', source: data.input}];
        default: return [{field: 'input', label: 'Boolean', type: 'bool', source: data.input}];
    }
}

export function controlPortType(nodes: ReadonlyMap<string, ControlNodeData>, start: string): ControlValueKind | null {
    const path = new Set<string>();
    let id = start;
    while (id && !path.has(id)) {
        path.add(id);
        const node = nodes.get(id);
        switch (node?.kind) {
            case 'action': return node.value;
            case 'boolean':
            case 'and':
            case 'or':
            case 'not':
            case 'button_held': return 'bool';
            case 'gate':
                id = node.input;
                break;
            default: return null;
        }
    }
    return null;
}

export function controlIssues(graph: ControlGraph): readonly ControlIssue[] {
    const issues: ControlIssue[] = [];
    const outputs = new Set<string>();
    const nodes = new Map(graph.nodes.map(node => [node.id, node.data]));
    for (const node of graph.nodes) {
        const {id, data} = node;
        for (const port of controlPorts(data)) {
            const type = controlPortType(nodes, port.source);
            if (!type || (port.type !== 'control_value' && port.type !== type)) {
                issues.push({node: id, message: `${CONTROL_NODE_LABELS[data.kind]} requires a compatible ${port.label} input.`});
            }
        }
        if (data.kind === 'action' && !validGameActionId(data.action)) issues.push({node: id, message: 'Enter a valid semantic action ID.'});
        if (data.kind === 'animator_bool' && !validGameActionId(data.parameter)) {
            issues.push({node: id, message: 'Enter the ID of an Animator Boolean parameter.'});
        }
        if (data.kind === 'move' || data.kind === 'look' || data.kind === 'jump' ||
            data.kind === 'attack' || data.kind === 'animator_bool') {
            const key = data.kind === 'animator_bool' ? 'animator:' + data.parameter : data.kind;
            if (outputs.has(key)) issues.push({node: id, message: 'Each Controls Graph output has one writer.'});
            outputs.add(key);
        }
    }
    if (!outputs.size) issues.push({node: null, message: 'Add at least one intent or Animator output.'});
    if (nodesHaveCycle(nodes)) issues.push({node: null, message: 'Controls Graph connections must be acyclic.'});
    if (encoder.encode(JSON.stringify(graph, null, 2)).length > maxBytes) {
        issues.push({node: null, message: 'Controls Graph exceeds 128 KiB.'});
    }
    return issues;
}

export function readControlGraph(value: unknown): ControlGraph {
    if (!record(value) || !only(value, ['schema_version', 'nodes', 'layout']) || value['schema_version'] !== 1 ||
        !Array.isArray(value['nodes']) || !value['nodes'].length || value['nodes'].length > MAX_CONTROL_NODES) {
        throw new Error('Controls Graph schema 1 requires 1–128 nodes.');
    }
    const ids = new Set<string>();
    const nodes = value['nodes'].map((node: unknown): ControlNode => {
        if (!record(node) || !only(node, ['id', 'data']) || !validGameActionId(node['id']) || ids.has(node['id'])) {
            throw new Error('Invalid or duplicate Controls Graph node ID.');
        }
        ids.add(node['id']);
        return {id: node['id'], data: readNodeData(node['data'])};
    });
    const layout = value['layout'];
    if (layout !== undefined && (!record(layout) || Object.entries(layout).some(([id, position]) =>
        !ids.has(id) || !record(position) || !only(position, ['x', 'y']) ||
        !integer(position['x'], 100_000) || !integer(position['y'], 100_000)))) {
        throw new Error('Controls Graph layout has an invalid node or position.');
    }
    const graph = {schema_version: 1, nodes, ...(layout === undefined ? {} : {layout})} as ControlGraph;
    const firstIssue = controlIssues(graph)[0];
    if (firstIssue) throw new Error(firstIssue.message);
    return graph;
}

export function readControlGraphCatalog(value: unknown): readonly ControlGraphAsset[] {
    if (!Array.isArray(value) || value.length > 4096) throw new Error('Invalid Controls Graph asset catalog.');
    const entries = value.map(readAsset);
    if (new Set(entries.map(asset => asset.id)).size !== entries.length) throw new Error('Duplicate Controls Graph asset ID.');
    return entries.sort((a, b) => a.path.localeCompare(b.path));
}

export function readControlGraphDocument(value: unknown): ControlGraphDocument {
    if (!record(value)) throw new Error('Invalid Controls Graph document.');
    return {asset: readAsset(value['asset']), graph: readControlGraph(value['graph'])};
}

export function readControlGraphProbe(value: unknown): ControlGraphProbe {
    if (!record(value) || !Array.isArray(value['order']) || value['order'].length > MAX_CONTROL_NODES ||
        !value['order'].every(validGameActionId) || new Set(value['order']).size !== value['order'].length ||
        !Array.isArray(value['active_node_ids']) || value['active_node_ids'].length > value['order'].length ||
        new Set(value['active_node_ids']).size !== value['active_node_ids'].length ||
        !validControlOutput(value['output'])) throw new Error('Invalid Controls Graph sample output.');
    const order = new Set(value['order']);
    if (!value['active_node_ids'].every(id => order.has(id))) throw new Error('Invalid Controls Graph sample node activity.');
    return {order: value['order'], active_node_ids: value['active_node_ids'], output: value['output']};
}

export function validControlOutput(value: unknown): value is ControlGraphOutput {
    if (!record(value) || !record(value['intent']) || !record(value['presentation'])) return false;
    const {movement, look, jump, attack} = value['intent'];
    return vector(movement) && record(look) && vector(look['delta']) && vector(look['rate']) && button(jump) && button(attack) &&
        Object.entries(value['presentation']).length <= MAX_CONTROL_NODES &&
        Object.entries(value['presentation']).every(([id, state]) => validGameActionId(id) && typeof state === 'boolean');
}

function readNodeData(value: unknown): ControlNodeData {
    if (!record(value)) throw new Error('Invalid Controls Graph node.');
    const {kind, input, left, right, condition, action, value: constant, parameter} = value;
    if (kind === 'action' && only(value, ['kind', 'action', 'value']) && validGameActionId(action) &&
        actionTypes.some(type => type === constant)) return {kind, action, value: constant as ControlActionKind};
    if (kind === 'boolean' && only(value, ['kind', 'value']) && typeof constant === 'boolean') return {kind, value: constant};
    if ((kind === 'and' || kind === 'or') && only(value, ['kind', 'left', 'right']) &&
        validGameActionId(left) && validGameActionId(right)) return {kind, left, right};
    if (kind === 'gate' && only(value, ['kind', 'condition', 'input']) &&
        validGameActionId(condition) && validGameActionId(input)) return {kind, condition, input};
    if (kind === 'animator_bool' && only(value, ['kind', 'parameter', 'input']) &&
        validGameActionId(parameter) && validGameActionId(input)) return {kind, parameter, input};
    if ((kind === 'button_held' || kind === 'not' || kind === 'move' || kind === 'look' || kind === 'jump' || kind === 'attack') &&
        only(value, ['kind', 'input']) && validGameActionId(input)) return {kind, input};
    throw new Error('Invalid Controls Graph node fields.');
}

function readAsset(value: unknown): ControlGraphAsset {
    if (!record(value) || !validInputActionsId(value['id']) || value['kind'] !== 'controls_graph' ||
        typeof value['path'] !== 'string' || !controlPath(value['path']) || typeof value['fingerprint'] !== 'string' ||
        !/^[0-9a-f]{64}$/.test(value['fingerprint']) || !positiveInteger(value['revision'], Number.MAX_SAFE_INTEGER) ||
        !positiveInteger(value['bytes'], maxBytes)) throw new Error('Invalid Controls Graph asset metadata.');
    const {id, kind, path, fingerprint, revision, bytes} = value;
    return {id, kind, path, fingerprint, revision, bytes};
}

export function controlHasCycle(graph: ControlGraph): boolean {
    return nodesHaveCycle(new Map(graph.nodes.map(node => [node.id, node.data])));
}

function nodesHaveCycle(nodes: ReadonlyMap<string, ControlNodeData>): boolean {
    const complete = new Set<string>();
    const active = new Set<string>();
    function visit(id: string): boolean {
        if (active.has(id)) return true;
        const data = nodes.get(id);
        if (!data || complete.has(id)) return false;
        active.add(id);
        if (controlPorts(data).some(port => visit(port.source))) return true;
        active.delete(id);
        complete.add(id);
        return false;
    }
    return [...nodes.keys()].some(visit);
}

function record(value: unknown): value is Record<string, unknown> {
    return value !== null && typeof value === 'object' && !Array.isArray(value);
}
function only(value: Record<string, unknown>, fields: readonly string[]): boolean {
    return Object.keys(value).every(field => fields.includes(field));
}
function integer(value: unknown, maximum: number): value is number {
    return typeof value === 'number' && Number.isSafeInteger(value) && Math.abs(value) <= maximum;
}
function positiveInteger(value: unknown, maximum: number): value is number {
    return integer(value, maximum) && value > 0;
}
function vector(value: unknown): value is Vector {
    return Array.isArray(value) && value.length === 2 && value.every(component =>
        typeof component === 'number' && Number.isFinite(component));
}
function button(value: unknown): value is ControlButton {
    return record(value) && ['held', 'pressed', 'released'].every(key => typeof value[key] === 'boolean');
}