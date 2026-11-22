import type {
    AnimatorCondition, AnimatorGraph, AnimatorParameter, AnimatorState, AnimatorTransition,
} from '../../../../core/editor/animator.models';
import type {GraphDocument, GraphPoint} from '../../../../shared/components/graph/graph.models';
import {DEFAULT_TRANSITION} from '../../../../shared/components/graph/animation/animation.models';

export type AnimatorEdit =
    | {readonly kind: 'add_state' | 'update_state'; readonly state: AnimatorState}
    | {readonly kind: 'set_entry' | 'remove_state'; readonly id: string}
    | {readonly kind: 'move_state'; readonly id: string; readonly position: GraphPoint}
    | {readonly kind: 'connect_states'; readonly from: string; readonly to: string; readonly id: string; readonly expectedRevision: number}
    | {readonly kind: 'update_transition'; readonly transition: AnimatorTransition}
    | {readonly kind: 'move_transition'; readonly id: string; readonly delta: -1 | 1}
    | {readonly kind: 'remove_transition'; readonly id: string}
    | {readonly kind: 'add_parameter'; readonly parameter: AnimatorParameter}
    | {readonly kind: 'remove_parameter'; readonly id: string};

export interface AnimatorIssue {
    readonly target: string;
    readonly message: string;
}

export function animatorCanvas(graph: AnimatorGraph): GraphDocument {
    return {
        version: 1,
        domain: 'animation',
        kind: 'stateMachine',
        nodes: graph.states.map((state, index) => ({
            id: state.id, kind: 'animation.state', position: graph.layout?.[state.id] ?? defaultPosition(index),
            values: {name: state.name, motion: 'reference'},
        })),
        connections: graph.transitions.map(transition => ({
            id: transition.id,
            from: {node: transition.from, port: 'next'},
            to: {node: transition.to, port: 'enter'},
            kind: 'transition',
            settings: DEFAULT_TRANSITION,
        })),
    };
}

export function animatorIssues(graph: AnimatorGraph): readonly AnimatorIssue[] {
    const reachable = new Set<string>();
    const pending = [graph.entry];
    const states = new Set(graph.states.map(state => state.id));
    if (!states.has(graph.entry)) return [{target: graph.entry, message: 'Choose an existing entry state.'}];
    while (pending.length) {
        const item = pending.pop()!;
        if (reachable.has(item)) continue;
        reachable.add(item);
        for (const edge of graph.transitions) if (edge.from === item && states.has(edge.to)) pending.push(edge.to);
    }
    return graph.states
        .filter(state => !reachable.has(state.id))
        .map(state => ({target: state.id, message: `${state.name} is unreachable from the entry state.`}));
}

export function applyAnimatorEdit(graph: AnimatorGraph, edit: AnimatorEdit, revision: number): AnimatorGraph {
    switch (edit.kind) {
        case 'add_state': {
            if (graph.states.length >= 64 || graph.states.some(state => state.id === edit.state.id)) {
                throw new Error('An Animator supports 64 states with distinct IDs.');
            }
            validateLabel(edit.state.name);
            return {...graph, states: [...graph.states, edit.state]};
        }
        case 'update_state': {
            requireState(graph, edit.state.id);
            validateLabel(edit.state.name);
            return {...graph, states: graph.states.map(state => state.id === edit.state.id ? edit.state : state)};
        }
        case 'set_entry':
            requireState(graph, edit.id);
            return {...graph, entry: edit.id};
        case 'remove_state': {
            requireState(graph, edit.id);
            const states = graph.states.filter(state => state.id !== edit.id);
            if (!states.length) throw new Error('An Animator needs at least one state.');
            const layout = {...graph.layout};
            delete layout[edit.id];
            return {...graph, layout, states, entry: graph.entry === edit.id ? states[0].id : graph.entry,
                transitions: graph.transitions.filter(edge => edge.from !== edit.id && edge.to !== edit.id)};
        }
        case 'move_state': {
            const index = graph.states.findIndex(state => state.id === edit.id);
            requireState(graph, edit.id);
            if (!Number.isFinite(edit.position.x) || !Number.isFinite(edit.position.y)) throw new Error('Invalid Animator coordinates.');
            const next = {x: clamp(edit.position.x), y: clamp(edit.position.y)};
            const before = graph.layout?.[edit.id] ?? defaultPosition(index);
            return before.x === next.x && before.y === next.y ? graph : {...graph, layout: {...graph.layout, [edit.id]: next}};
        }
        case 'connect_states':
            if (edit.expectedRevision !== revision) throw new Error('The Animator changed during the wire gesture.');
            requireState(graph, edit.from);
            requireState(graph, edit.to);
            if (graph.transitions.length >= 256 || graph.transitions.some(edge => edge.id === edit.id)) {
                throw new Error('An Animator supports 256 transitions with distinct IDs.');
            }
            return {...graph, transitions: [...graph.transitions, {
                id: edit.id, from: edit.from, to: edit.to, condition: {kind: 'always'}, blend_ticks: 0,
            }]};
        case 'update_transition': {
            const edge = edit.transition;
            requireTransition(graph, edge.id);
            requireState(graph, edge.from);
            requireState(graph, edge.to);
            validateCondition(graph, edge.condition);
            if (!integer(edge.blend_ticks, 0, 3600) ||
                (edge.exit_after_ticks !== undefined && !integer(edge.exit_after_ticks, 0, 1_000_000))) {
                throw new Error('Choose blend ticks between 0 and 3,600 and exit ticks between 0 and 1,000,000.');
            }
            return {...graph, transitions: graph.transitions.map(prior => prior.id === edge.id ? edge : prior)};
        }
        case 'move_transition': {
            const index = requireTransition(graph, edit.id);
            const next = index + edit.delta;
            if (next < 0 || next >= graph.transitions.length) return graph;
            const transitions = [...graph.transitions];
            [transitions[index], transitions[next]] = [transitions[next], transitions[index]];
            return {...graph, transitions};
        }
        case 'remove_transition':
            requireTransition(graph, edit.id);
            return {...graph, transitions: graph.transitions.filter(edge => edge.id !== edit.id)};
        case 'add_parameter':
            validateLabel(edit.parameter.name);
            if (graph.parameters.length >= 64 || graph.parameters.some(parameter => parameter.id === edit.parameter.id)) {
                throw new Error('An Animator supports 64 parameters with distinct IDs.');
            }
            return {...graph, parameters: [...graph.parameters, edit.parameter]};
        case 'remove_parameter':
            if (graph.transitions.some(edge => edge.condition.kind !== 'always' && edge.condition.parameter === edit.id)) {
                throw new Error('Choose another condition on each transition using this parameter first.');
            }
            return {...graph, parameters: graph.parameters.filter(parameter => parameter.id !== edit.id)};
    }
}

export function isTrigger(parameter: AnimatorParameter): boolean {
    return parameter.source.kind === 'jumped' || parameter.source.kind === 'action_accepted';
}

export function runtimeDefinition(graph: AnimatorGraph): string {
    return JSON.stringify({
        schema_version: graph.schema_version,
        entry: graph.entry,
        states: graph.states.map(({id, clip, looping}) => ({id, name: '', clip: clip.kind === 'authored'
            ? {kind: 'authored', asset_id: clip.asset_id, expected_sha256: clip.expected_sha256}
            : {kind: 'imported', source: {
                asset_id: clip.source.asset_id, expected_sha256: clip.source.expected_sha256,
                scene_index: clip.source.scene_index, mesh_node: clip.source.mesh_node, skin_index: clip.source.skin_index,
            }, clip_index: clip.clip_index}, looping})),
        parameters: graph.parameters.map(({id, source}) => ({
            id, name: '', source: source.kind === 'moving' ? {kind: source.kind, minimum_mm_per_second: source.minimum_mm_per_second}
                : source.kind === 'action_accepted' ? {kind: source.kind, action: source.action}
                : source.kind === 'boolean' ? {kind: source.kind, default: source.default}
                : {kind: source.kind},
        })),
        transitions: graph.transitions.map((edge) => ({
            id: edge.id, from: edge.from, to: edge.to, condition: edge.condition.kind === 'always'
                ? {kind: 'always'} : edge.condition.kind === 'bool'
                    ? {kind: 'bool', parameter: edge.condition.parameter, value: edge.condition.value}
                    : {kind: 'trigger', parameter: edge.condition.parameter},
            ...(edge.exit_after_ticks === undefined ? {} : {exit_after_ticks: edge.exit_after_ticks}),
            blend_ticks: edge.blend_ticks,
        })),
    });
}

function defaultPosition(index: number): GraphPoint {
    return {x: 40 + (index % 3) * 250, y: 32 + Math.floor(index / 3) * 145};
}

function requireState(graph: AnimatorGraph, id: string): void {
    if (!graph.states.some(state => state.id === id)) throw new Error('Select an existing source and destination state.');
}

function requireTransition(graph: AnimatorGraph, id: string): number {
    const index = graph.transitions.findIndex(edge => edge.id === id);
    if (index < 0) throw new Error('Select an existing transition.');
    return index;
}

function validateCondition(graph: AnimatorGraph, condition: AnimatorCondition): void {
    if (condition.kind === 'always') return;
    const parameter = graph.parameters.find(entry => entry.id === condition.parameter);
    if (!parameter || isTrigger(parameter) !== (condition.kind === 'trigger')) {
        throw new Error('Choose a parameter that matches the Boolean or Trigger condition.');
    }
}

function validateLabel(name: string): void {
    if (!name.trim() || name !== name.trim() || /\p{Cc}/u.test(name) || new TextEncoder().encode(name).length > 96) {
        throw new Error('Names must contain 1–96 UTF-8 bytes with no leading or trailing spaces.');
    }
}

function integer(value: number, min: number, max: number): boolean {
    return Number.isSafeInteger(value) && value >= min && value <= max;
}

function clamp(value: number): number {
    return Math.max(-100_000, Math.min(100_000, Math.round(value)));
}
