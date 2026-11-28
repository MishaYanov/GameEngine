import {validGameActionId} from '../game-input/game-input.models';
import {validInputActionsId} from '../game-input/input-actions.models';
import {sameAnimationRevision, type AnimationSource, validAnimationSettings} from './animation.models';
import type {VisualClipTarget} from './authored-clip.models';
import type {EntityKey} from './editor.models';

export type AnimatorClipRef =
    | {readonly kind: 'imported'; readonly source: AnimationSource; readonly clip_index: number}
    | {readonly kind: 'authored'; readonly asset_id: string; readonly expected_sha256: string};
export interface AnimatorState {
    readonly id: string;
    readonly name: string;
    readonly clip: AnimatorClipRef;
    readonly looping: boolean;
}
export type AnimatorParameterSource =
    | {readonly kind: 'boolean'; readonly default: boolean}
    | {readonly kind: 'grounded' | 'jumped'}
    | {readonly kind: 'moving'; readonly minimum_mm_per_second: number}
    | {readonly kind: 'action_accepted'; readonly action: string};
export interface AnimatorParameter {
    readonly id: string;
    readonly name: string;
    readonly source: AnimatorParameterSource;
}
export type AnimatorCondition =
    | {readonly kind: 'always'}
    | {readonly kind: 'bool'; readonly parameter: string; readonly value: boolean}
    | {readonly kind: 'trigger'; readonly parameter: string};
export interface AnimatorTransition {
    readonly id: string;
    readonly from: string;
    readonly to: string;
    readonly condition: AnimatorCondition;
    readonly exit_after_ticks?: number;
    readonly blend_ticks: number;
}
export interface AnimatorGraph {
    readonly schema_version: 1;
    readonly entry: string;
    readonly states: readonly AnimatorState[];
    readonly parameters: readonly AnimatorParameter[];
    readonly transitions: readonly AnimatorTransition[];
    readonly layout?: Readonly<Record<string, {readonly x: number; readonly y: number}>>;
}
export interface AnimatorVisualTarget {
    readonly channel: VisualClipTarget;
    readonly entity_key: EntityKey;
}
export type AnimatorBinding =
    | {readonly kind: 'imported'; readonly source: AnimationSource}
    | {readonly kind: 'visual'; readonly targets?: readonly AnimatorVisualTarget[]};
export interface AnimatorSettings {
    readonly asset_id: string;
    readonly binding: AnimatorBinding;
    readonly root_motion_source_node?: number;
    readonly foot_ik?: FootIkSettings;
}

export interface FootIkSettings {
    readonly foot_joint: string;
    readonly reach_mm: number;
    readonly sole_offset_mm: number;
}
export interface AnimatorAttachment {
    readonly id: string;
    readonly enabled: boolean;
    readonly settings: AnimatorSettings;
}
export interface AnimatorAsset {
    readonly id: string;
    readonly kind: 'animator_graph';
    readonly path: string;
    readonly revision: number;
    readonly fingerprint: string;
    readonly bytes: number;
}
export type AnimatorAssetCapture = Pick<AnimatorAsset, 'id' | 'fingerprint'>;
export interface AnimatorDocument { readonly asset: AnimatorAsset; readonly graph: AnimatorGraph }
export interface AnimatorActorValidation {
    readonly entry: string;
    readonly states: number;
    readonly parameters: number;
    readonly runtime_digest: string;
}

const encoder = new TextEncoder();
export const MAX_ANIMATOR_BYTES = 256 * 1024;
const suffix = '.animator.json';

export function validAnimatorPath(value: string): boolean {
    return value.toLowerCase().endsWith(suffix) && encoder.encode(value).length <= 4096 &&
        !/[:\\\p{Cc}]/u.test(value) && value.split('/').every(part => !!part && part !== '.' && part !== '..');
}

export function starterAnimator(clip: AnimatorClipRef): AnimatorGraph {
    return {schema_version: 1, entry: 'idle', states: [{id: 'idle', name: 'Idle', clip, looping: true}],
        parameters: [], transitions: []};
}

export function validAnimatorSettings(value: unknown): value is AnimatorSettings {
    if (!record(value) || !validInputActionsId(value['asset_id']) || !record(value['binding'])) return false;
    const binding = value['binding'];
    const rootNode = value['root_motion_source_node'];
    const foot = value['foot_ik'];
    if (binding['kind'] === 'imported') {
        return validSource(binding['source']) && (rootNode === undefined || integer(rootNode, 0, 0xffffffff)) &&
            (foot === undefined || validFootIk(foot, binding['source']));
    }
    if (binding['kind'] !== 'visual' || rootNode !== undefined || foot !== undefined) return false;
    const targets = binding['targets'];
    if (targets === undefined) return true;
    if (!Array.isArray(targets) || targets.length > 128) return false;
    const channels = new Set<string>();
    for (const target of targets) {
        if (!record(target) || !entityKey(target['entity_key']) || !visualChannel(target['channel'])) return false;
        const key = animatorChannelKey(target['channel']);
        if (channels.has(key)) return false;
        channels.add(key);
    }
    return true;
}

function validFootIk(value: unknown, source: AnimationSource): boolean {
    return record(value) && only(value, ['foot_joint', 'reach_mm', 'sole_offset_mm']) &&
        Object.values(source.character?.joints ?? {}).some(id => id === value['foot_joint']) &&
        integer(value['reach_mm'], 1, 1000) && integer(value['sole_offset_mm'], 0, 1000);
}

export function animatorChannelKey(channel: VisualClipTarget): string {
    if (channel.kind === 'scene') return JSON.stringify([channel.kind, channel.entity_key]);
    const address = channel.address;
    return JSON.stringify([channel.kind, address.kind, address.placement_path,
        address.kind === 'object' ? address.source_entity : null]);
}

export function sameAnimatorSource(a: AnimationSource, b: AnimationSource): boolean {
    return sameAnimationRevision(a, b) && JSON.stringify(a.character) === JSON.stringify(b.character);
}

export function readAnimatorGraph(value: unknown): AnimatorGraph {
    if (!record(value) || !only(value, ['schema_version', 'entry', 'states', 'parameters', 'transitions', 'layout']) ||
        value['schema_version'] !== 1 || typeof value['entry'] !== 'string' || !Array.isArray(value['states']) ||
        value['states'].length < 1 || value['states'].length > 64) {
        throw new Error('Animator schema 1 requires one entry and between 1 and 64 states.');
    }
    const states = value['states'].map(readState);
    const ids = uniqueIds(states, 'state');
    if (!ids.has(value['entry'])) throw new Error(`Entry "${value['entry']}" must name an existing state.`);
    const parameters = collection(value['parameters'], 64, 'parameters').map(readParameter);
    uniqueIds(parameters, 'parameter');
    const transitions = collection(value['transitions'], 256, 'transitions').map(readTransition);
    uniqueIds(transitions, 'transition');
    const kinds = new Map(parameters.map(({id, source}) => [id, source.kind === 'jumped' || source.kind === 'action_accepted']));
    const edges = new Map<string, string[]>();
    for (const {id, from, to, condition} of transitions) {
        if (!ids.has(from) || !ids.has(to)) throw new Error(`Transition "${id}" needs existing source and destination states.`);
        if (condition.kind !== 'always' && kinds.get(condition.parameter) !== (condition.kind === 'trigger')) {
            throw new Error(`Transition "${id}" refers to a missing or wrongly typed parameter.`);
        }
        const adjacent = edges.get(from);
        if (adjacent) adjacent.push(to);
        else edges.set(from, [to]);
    }
    const pending = [value['entry']];
    const reached = new Set<string>(pending);
    for (let index = 0; index < pending.length; index++) {
        for (const target of edges.get(pending[index]) ?? []) {
            if (!reached.has(target)) {
                reached.add(target);
                pending.push(target);
            }
        }
    }
    const unreachable = states.find(state => !reached.has(state.id));
    if (unreachable) throw new Error(`State "${unreachable.id}" is unreachable from the entry.`);
    const layout = readLayout(value['layout'], ids);
    const graph: AnimatorGraph = {schema_version: 1, entry: value['entry'], states, parameters, transitions, ...(layout ? {layout} : {})};
    if (encoder.encode(JSON.stringify(graph, null, 2)).length > MAX_ANIMATOR_BYTES) {
        throw new Error('Animator graph exceeds 256 KiB.');
    }
    return graph;
}

export function readAnimatorDocument(value: unknown): AnimatorDocument {
    if (!record(value)) throw new Error('The editor returned an invalid Animator document.');
    return {asset: readAnimatorAsset(value['asset']), graph: readAnimatorGraph(value['graph'])};
}

export function readAnimatorCatalog(value: unknown): readonly AnimatorAsset[] {
    if (!Array.isArray(value) || value.length > 4096) throw new Error('Invalid Animator asset catalog.');
    const assets = value.map(readAnimatorAsset);
    if (new Set(assets.map(asset => asset.id)).size !== assets.length) throw new Error('Animator asset IDs repeat.');
    return assets.sort((a, b) => a.path.localeCompare(b.path));
}

export function readAnimatorValidation(value: unknown): AnimatorActorValidation {
    if (!record(value) || !validGameActionId(value['entry']) || !integer(value['states'], 1, 64) ||
        !integer(value['parameters'], 0, 64) || !sha(value['runtime_digest'])) {
        throw new Error('The editor returned invalid Animator validation data.');
    }
    const {entry, states, parameters, runtime_digest} = value;
    return {entry, states, parameters, runtime_digest};
}

function readAnimatorAsset(value: unknown): AnimatorAsset {
    if (!record(value) || !validInputActionsId(value['id']) || value['kind'] !== 'animator_graph' ||
        typeof value['path'] !== 'string' || !validAnimatorPath(value['path']) || !sha(value['fingerprint']) ||
        !integer(value['revision'], 1, Number.MAX_SAFE_INTEGER) || !integer(value['bytes'], 1, MAX_ANIMATOR_BYTES)) {
        throw new Error('The editor returned invalid Animator asset metadata.');
    }
    const {id, path, fingerprint, revision, bytes} = value;
    return {id, path, kind: 'animator_graph', fingerprint, revision, bytes};
}

function readState(value: unknown): AnimatorState {
    if (!record(value) || !only(value, ['id', 'name', 'clip', 'looping']) || !validGameActionId(value['id']) ||
        !label(value['name']) || (value['looping'] !== undefined && typeof value['looping'] !== 'boolean')) {
        throw diagnostic('State', value, 'needs a valid ID, display name, and Boolean loop flag.');
    }
    const clip = value['clip'];
    if (!record(clip)) throw diagnostic('State', value, 'requires a clip.');
    if (clip['kind'] === 'authored' && only(clip, ['kind', 'asset_id', 'expected_sha256']) &&
        validInputActionsId(clip['asset_id']) && sha(clip['expected_sha256'])) {
        return {id: value['id'], name: value['name'], clip: {kind: 'authored', asset_id: clip['asset_id'],
            expected_sha256: clip['expected_sha256']}, looping: value['looping'] === true};
    }
    if (clip['kind'] === 'imported' && only(clip, ['kind', 'source', 'clip_index']) &&
        validSource(clip['source']) && integer(clip['clip_index'], 0, 31)) {
        return {id: value['id'], name: value['name'], clip: {kind: 'imported', source: clip['source'],
            clip_index: clip['clip_index']}, looping: value['looping'] === true};
    }
    throw diagnostic('State', value, 'needs a valid imported or authored clip reference.');
}

function readParameter(value: unknown): AnimatorParameter {
    if (!record(value) || !only(value, ['id', 'name', 'source']) || !validGameActionId(value['id']) ||
        !label(value['name']) || !record(value['source'])) throw diagnostic('Parameter', value, 'is invalid.');
    const input = value['source'];
    const kind = input['kind'];
    let source: AnimatorParameterSource;
    if (kind === 'boolean' && only(input, ['kind', 'default']) &&
        (input['default'] === undefined || typeof input['default'] === 'boolean')) source = {kind, default: input['default'] === true};
    else if ((kind === 'grounded' || kind === 'jumped') && only(input, ['kind'])) source = {kind};
    else if (kind === 'moving' && only(input, ['kind', 'minimum_mm_per_second']) &&
        integer(input['minimum_mm_per_second'], 1, 10_000)) source = {kind, minimum_mm_per_second: input['minimum_mm_per_second']};
    else if (kind === 'action_accepted' && only(input, ['kind', 'action']) &&
        validGameActionId(input['action'])) source = {kind, action: input['action']};
    else throw diagnostic('Parameter', value, 'has an invalid source or speed threshold.');
    return {id: value['id'], name: value['name'], source};
}

function readTransition(value: unknown): AnimatorTransition {
    if (!record(value) || !only(value, ['id', 'from', 'to', 'condition', 'exit_after_ticks', 'blend_ticks']) ||
        !validGameActionId(value['id']) || typeof value['from'] !== 'string' || typeof value['to'] !== 'string' ||
        !record(value['condition'])) throw diagnostic('Transition', value, 'is invalid.');
    const exit = value['exit_after_ticks'];
    const blend = value['blend_ticks'] === undefined ? 0 : value['blend_ticks'];
    if (exit != null && !integer(exit, 0, 1_000_000) || !integer(blend, 0, 3600)) {
        throw diagnostic('Transition', value, 'exceeds its exit or blend duration limit.');
    }
    const test = value['condition'];
    const kind = test['kind'];
    let condition: AnimatorCondition;
    if (kind === 'always' && only(test, ['kind'])) condition = {kind};
    else if (kind === 'bool' && only(test, ['kind', 'parameter', 'value']) &&
        typeof test['parameter'] === 'string' && typeof test['value'] === 'boolean') {
        condition = {kind, parameter: test['parameter'], value: test['value']};
    } else if (kind === 'trigger' && only(test, ['kind', 'parameter']) &&
        typeof test['parameter'] === 'string') condition = {kind, parameter: test['parameter']};
    else throw diagnostic('Transition', value, 'has an invalid condition.');
    return {id: value['id'], from: value['from'], to: value['to'], condition, blend_ticks: blend,
        ...(typeof exit === 'number' ? {exit_after_ticks: exit} : {})};
}

function readLayout(value: unknown, ids: ReadonlySet<string>): AnimatorGraph['layout'] {
    if (value === undefined) return undefined;
    if (!record(value) || Object.entries(value).some(([id, position]) => !ids.has(id) || !record(position) ||
        !only(position, ['x', 'y']) || !integer(position['x'], -100_000, 100_000) ||
        !integer(position['y'], -100_000, 100_000))) {
        throw new Error('Animator layout addresses a missing state or exceeds the canvas bounds.');
    }
    return value as AnimatorGraph['layout'];
}

function visualChannel(value: unknown): value is VisualClipTarget {
    if (!record(value)) return false;
    if (value['kind'] === 'scene') return entityKey(value['entity_key']);
    if (value['kind'] !== 'prop' || !record(value['address'])) return false;
    const address = value['address'];
    const path = address['placement_path'];
    return Array.isArray(path) && path.length <= 8 && path.every(id => typeof id === 'string' && /^[A-Za-z0-9_-]{1,128}$/.test(id)) &&
        (address['kind'] === 'placement' || address['kind'] === 'object' && entityKey(address['source_entity']));
}

function validSource(value: unknown): value is AnimationSource {
    if (!record(value) || !validInputActionsId(value['asset_id']) ||
        !only(value, ['asset_id', 'expected_sha256', 'scene_index', 'mesh_node', 'skin_index', 'character'])) return false;
    return validAnimationSettings({source: value, playback: {kind: 'single_clip', clip_index: 0, looping: false},
        speed_numerator: 1, speed_denominator: 1});
}

function uniqueIds<T extends {readonly id: string}>(items: readonly T[], kind: string): Set<string> {
    const ids = new Set<string>();
    for (const item of items) {
        if (ids.has(item.id)) throw new Error(`Duplicate Animator ${kind} ID "${item.id}".`);
        ids.add(item.id);
    }
    return ids;
}

function collection(value: unknown, max: number, name: string): readonly unknown[] {
    if (value === undefined) return [];
    if (!Array.isArray(value) || value.length > max) throw new Error(`Animator allows up to ${max} ${name}.`);
    return value;
}

function diagnostic(kind: string, value: unknown, reason: string): Error {
    const id = record(value) && typeof value['id'] === 'string' ? ` "${value['id']}"` : '';
    return new Error(kind + id + ' ' + reason);
}
function record(value: unknown): value is Record<string, unknown> {
    return value !== null && typeof value === 'object' && !Array.isArray(value);
}
function only(value: Record<string, unknown>, keys: readonly string[]): boolean {
    return Object.keys(value).every(key => keys.includes(key));
}
function sha(value: unknown): value is string { return typeof value === 'string' && /^[a-f0-9]{64}$/.test(value); }
function label(value: unknown): value is string {
    return typeof value === 'string' && !!value.length && value.trim() === value && !/\p{Cc}/u.test(value) &&
        encoder.encode(value).length <= 96;
}
function entityKey(value: unknown): value is string { return typeof value === 'string' && !!value && !/\s/u.test(value); }
function integer(value: unknown, min: number, max: number): value is number {
    return typeof value === 'number' && Number.isSafeInteger(value) && value >= min && value <= max;
}
