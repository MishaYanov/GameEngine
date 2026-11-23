import {DEFAULT_GAME_INPUT, GAME_KEYS, MOVEMENT_AXES, type MovementAxis, validGameActionId} from './game-input.models';

export type InputActionValue = 'button' | 'vector2' | 'delta2';
export const FACE_BUTTONS = ['a', 'b', 'x', 'y'] as const;
export type FaceButton = (typeof FACE_BUTTONS)[number];
export type InputActionKeys = Readonly<Record<MovementAxis, string>>;

export type InputActionBinding =
    | {readonly kind: 'key'; readonly key: string}
    | {readonly kind: 'gamepad_button'; readonly button: FaceButton}
    | {readonly kind: 'keyboard_vector' | 'keyboard_look'; readonly keys: InputActionKeys}
    | {readonly kind: 'gamepad_left_stick' | 'gamepad_right_stick' | 'pointer_delta'};

export interface NamedInputAction {
    readonly id: string;
    readonly label: string;
    readonly value: InputActionValue;
    readonly bindings: readonly InputActionBinding[];
}

export type InputLayoutCondition =
    | {readonly kind: 'character_grounded'; readonly value: boolean}
    | {readonly kind: 'scene_is'; readonly scene_id: string};

export interface InputBindingOverride {
    readonly action_id: string;
    readonly bindings: readonly InputActionBinding[];
}

export interface ConditionalInputLayout {
    readonly id: string;
    readonly label: string;
    readonly when: InputLayoutCondition;
    readonly overrides: readonly InputBindingOverride[];
}

export interface InputLayoutFacts {
    readonly scene: string | null;
    readonly grounded: boolean | null;
}

export function layoutMatches(condition: InputLayoutCondition, facts: InputLayoutFacts): boolean | null {
    if (condition.kind === 'scene_is') return facts.scene === null ? null : facts.scene === condition.scene_id;
    return facts.grounded === null ? null : facts.grounded === condition.value;
}

export function firstMatchingLayout(
    layouts: readonly ConditionalInputLayout[], facts: InputLayoutFacts,
): ConditionalInputLayout | null {
    return layouts.find(layout => layoutMatches(layout.when, facts) === true) ?? null;
}

export interface InputActionMap {
    readonly schema_version: 1 | 2;
    readonly actions: readonly NamedInputAction[];
    readonly layouts?: readonly ConditionalInputLayout[];
}

export interface InputActionsAsset {
    readonly id: string;
    readonly path: string;
    readonly kind: 'input_actions';
    readonly revision: number;
    readonly fingerprint: string;
    readonly bytes: number;
}

export interface InputActionsDocument {
    readonly asset: InputActionsAsset;
    readonly map: InputActionMap;
}

export type InputActionsCapture = Pick<InputActionsAsset, 'id' | 'fingerprint'>;
export const INPUT_ACTIONS_SUFFIX = '.input-actions.json';
export const MAX_INPUT_ACTIONS = 64;
export const MAX_ACTION_BINDINGS = 8;
export const MAX_CONDITIONAL_INPUT_LAYOUTS = 16;
const MAX_BYTES = 64 * 1024;
const encoder = new TextEncoder();
export const ACTION_VALUES = [
    {value: 'button', label: 'Button'}, {value: 'vector2', label: 'Movement vector'}, {value: 'delta2', label: 'Look delta'},
] as const;
export const ACTION_BINDINGS = [
    {kind: 'key', value: 'button', label: 'Keyboard key'},
    {kind: 'gamepad_button', value: 'button', label: 'Gamepad button'},
    {kind: 'keyboard_vector', value: 'vector2', label: 'Keyboard movement'},
    {kind: 'gamepad_left_stick', value: 'vector2', label: 'Gamepad left stick'},
    {kind: 'keyboard_look', value: 'delta2', label: 'Keyboard look'},
    {kind: 'gamepad_right_stick', value: 'delta2', label: 'Gamepad right stick'},
    {kind: 'pointer_delta', value: 'delta2', label: 'Pointer movement'},
] as const;
export const INPUT_ACTION_KEYS = [
    ...GAME_KEYS,
    ...['tab', 'backspace', 'delete', 'insert', 'home', 'end', 'page_up', 'page_down', 'minus',
        'equal', 'comma', 'period', 'slash', 'backslash', 'backquote', 'semicolon', 'quote',
        'bracket_left', 'bracket_right', ...Array.from({length: 24}, (_, i) => 'f' + (i + 1))]
        .map(key => ({key, label: key.replaceAll('_', ' ')})),
];

export function starterInputActions(): InputActionMap {
    return {schema_version: 1, actions: [
        {id: 'move', label: 'Move', value: 'vector2', bindings: [
            {kind: 'keyboard_vector', keys: {...DEFAULT_GAME_INPUT.movement}}, {kind: 'gamepad_left_stick'},
        ]},
        {id: 'look', label: 'Look', value: 'delta2', bindings: [{kind: 'pointer_delta'}, {kind: 'gamepad_right_stick'}]},
        {id: 'jump', label: 'Jump', value: 'button', bindings: [{kind: 'key', key: 'space'}, {kind: 'gamepad_button', button: 'a'}]},
        {id: 'attack', label: 'Attack', value: 'button', bindings: [{kind: 'key', key: 'f'}, {kind: 'gamepad_button', button: 'x'}]},
    ]};
}

export function validInputActionsId(value: unknown): value is string {
    return typeof value === 'string' &&
        /^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/.test(value) &&
        value !== '00000000-0000-0000-0000-000000000000';
}

export function validInputActionsPath(path: string): boolean {
    return path.toLowerCase().endsWith(INPUT_ACTIONS_SUFFIX) && encoder.encode(path).length <= 4096 &&
        !/[:\\\p{Cc}]/u.test(path) && path.split('/').every(part => !!part && part !== '.' && part !== '..');
}

export function effectiveLayoutActions(
    map: InputActionMap, layout: ConditionalInputLayout | null,
): readonly NamedInputAction[] {
    if (!layout) return map.actions;
    const overrides = new Map(layout.overrides.map(item => [item.action_id, item.bindings]));
    return map.actions.map(action => overrides.has(action.id)
        ? {...action, bindings: overrides.get(action.id)!} : action);
}

export function validateInputActions(map: InputActionMap, scenes?: ReadonlySet<string>): string | null {
    if (!map.actions.length || map.actions.length > MAX_INPUT_ACTIONS) {
        return 'Add between 1 and 64 input actions.';
    }
    const defaultError = validateActions(map.actions);
    if (defaultError) return defaultError;
    const actions = new Set(map.actions.map(action => action.id));
    const layouts = map.layouts ?? [];
    if (map.schema_version === 1 && layouts.length) return 'Conditional layouts require Input Actions schema 2.';
    if (layouts.length > MAX_CONDITIONAL_INPUT_LAYOUTS) return 'Add up to 16 conditional layouts.';
    const layoutIds = new Set<string>();
    for (const layout of layouts) {
        if (!validGameActionId(layout.id) || layout.id === 'default' || layoutIds.has(layout.id)) {
            return 'Each conditional layout needs a unique ID.';
        }
        layoutIds.add(layout.id);
        if (!validLabel(layout.label)) return `Enter a display name of 1–96 bytes for layout ${layout.id}.`;
        if (layout.when.kind === 'scene_is' &&
            (!sceneId(layout.when.scene_id) || scenes && !scenes.has(layout.when.scene_id))) {
            return `Choose an available scene for ${layout.label}.`;
        }
        if (layout.overrides.length > MAX_INPUT_ACTIONS) return `Layout ${layout.label} has too many action overrides.`;
        const overrides = new Set<string>();
        for (const entry of layout.overrides) {
            if (!actions.has(entry.action_id) || overrides.has(entry.action_id) || entry.bindings.length > MAX_ACTION_BINDINGS) {
                return `Layout ${layout.label} has invalid or repeated action overrides.`;
            }
            overrides.add(entry.action_id);
        }
        const problem = validateActions(effectiveLayoutActions(map, layout));
        if (problem) return `Layout ${layout.label}. ${problem}`;
    }
    return encoder.encode(JSON.stringify(map, null, 2)).length > MAX_BYTES ? 'Input Actions must fit within 64 KiB.' : null;
}

function validateActions(actions: readonly NamedInputAction[]): string | null {
    const ids = new Set<string>();
    const controls = new Map<string, string>();
    for (const [index, action] of actions.entries()) {
        if (!validGameActionId(action.id) || ids.has(action.id)) {
            return `Action ${index + 1} needs a unique ID such as jump or use_item.`;
        }
        ids.add(action.id);
        if (!validLabel(action.label)) return `Enter a display name of 1–96 bytes for ${action.id}.`;
        const expected = action.id === 'move' ? 'vector2' : action.id === 'look' ? 'delta2'
            : ['jump', 'attack', 'interact'].includes(action.id) ? 'button' : action.value;
        if (expected !== action.value) return `${action.id} needs the ${expected} value type.`;
        if (action.bindings.length > MAX_ACTION_BINDINGS) return `Add up to eight bindings for ${action.id}.`;
        for (const binding of action.bindings) {
            if (ACTION_BINDINGS.find(item => item.kind === binding.kind)?.value !== action.value) {
                return `${action.id} has a binding with the wrong value type.`;
            }
            for (const control of bindingControls(binding)) {
                if (control === 'key:escape') return `${action.id} uses a reserved control.`;
                const previous = controls.get(control);
                if (previous) return `${previous} and ${action.id} use a conflicting control.`;
                controls.set(control, action.id);
            }
        }
    }
    return null;
}

export function readInputActionsAsset(value: unknown): InputActionsAsset {
    if (!record(value) || !validInputActionsId(value['id']) || typeof value['path'] !== 'string' ||
        !validInputActionsPath(value['path']) || value['kind'] !== 'input_actions' ||
        !integer(value['revision'], 1, Number.MAX_SAFE_INTEGER) || !integer(value['bytes'], 1, MAX_BYTES) ||
        typeof value['fingerprint'] !== 'string' || !/^[a-f0-9]{64}$/.test(value['fingerprint'])) {
        throw new Error('The editor returned invalid Input Actions asset metadata.');
    }
    const {id, path, revision, fingerprint, bytes} = value;
    return {id, path, kind: 'input_actions', revision, fingerprint, bytes};
}

export function readInputActionsCatalog(value: unknown): readonly InputActionsAsset[] {
    if (!Array.isArray(value) || value.length > 4096) throw new Error('The editor returned an invalid Input Actions catalog.');
    const assets = value.map(readInputActionsAsset);
    if (new Set(assets.map(asset => asset.id)).size !== assets.length) throw new Error('Input Actions asset IDs repeat.');
    return assets.sort((a, b) => a.path.localeCompare(b.path));
}

export function readInputActionsDocument(value: unknown): InputActionsDocument {
    if (!record(value) || !mapShape(value['map'])) throw new Error('The editor returned an invalid Input Actions map.');
    const error = validateInputActions(value['map']);
    if (error) throw new Error(error);
    return {asset: readInputActionsAsset(value['asset']), map: value['map']};
}

function bindingControls(binding: InputActionBinding): readonly string[] {
    if (binding.kind === 'key') return [`key:${binding.key}`];
    if (binding.kind === 'gamepad_button') return [`pad:${binding.button}`];
    if ('keys' in binding) return MOVEMENT_AXES.map(({field}) => `key:${binding.keys[field]}`);
    return [binding.kind];
}

function record(value: unknown): value is Record<string, unknown> {
    return value !== null && typeof value === 'object' && !Array.isArray(value);
}

function integer(value: unknown, min: number, max: number): value is number {
    return typeof value === 'number' && Number.isSafeInteger(value) && value >= min && value <= max;
}

function key(value: unknown): value is string {
    return typeof value === 'string' && /^[a-z][a-z0-9_]{0,47}$/.test(value);
}

function validLabel(label: string): boolean {
    return !!label && label.trim() === label && !/\p{Cc}/u.test(label) && encoder.encode(label).length <= 96;
}

function sceneId(value: unknown): value is string {
    return typeof value === 'string' && !!value && encoder.encode(value).length <= 256 && !/[\p{White_Space}\p{Cc}]/u.test(value);
}

function bindingShape(value: unknown): boolean {
    if (!record(value)) return false;
    switch (value['kind']) {
        case 'key': return Object.keys(value).length === 2 && key(value['key']);
        case 'gamepad_button': return Object.keys(value).length === 2 && FACE_BUTTONS.some(button => button === value['button']);
        case 'keyboard_vector':
        case 'keyboard_look': {
            const keys = value['keys'];
            return Object.keys(value).length === 2 && record(keys) && Object.keys(keys).length === 4 &&
                MOVEMENT_AXES.every(({field}) => key(keys[field]));
        }
        case 'gamepad_left_stick':
        case 'gamepad_right_stick':
        case 'pointer_delta': return Object.keys(value).length === 1;
        default: return false;
    }
}

function layoutShape(value: unknown): boolean {
    if (!record(value) || Object.keys(value).length !== 4 ||
        typeof value['id'] !== 'string' || typeof value['label'] !== 'string') return false;
    const condition = value['when'];
    if (!record(condition) || Object.keys(condition).length !== 2 ||
        !(condition['kind'] === 'character_grounded' && typeof condition['value'] === 'boolean' ||
            condition['kind'] === 'scene_is' && sceneId(condition['scene_id']))) return false;
    const overrides = value['overrides'];
    return Array.isArray(overrides) && overrides.length <= MAX_INPUT_ACTIONS && overrides.every(item =>
        record(item) && Object.keys(item).length === 2 && typeof item['action_id'] === 'string' &&
        Array.isArray(item['bindings']) && item['bindings'].length <= MAX_ACTION_BINDINGS && item['bindings'].every(bindingShape));
}

function mapShape(value: unknown): value is InputActionMap {
    if (!record(value) || Object.keys(value).some(key => !['schema_version', 'actions', 'layouts'].includes(key)) ||
        (value['schema_version'] !== 1 && value['schema_version'] !== 2) || !Array.isArray(value['actions']) ||
        value['actions'].length > MAX_INPUT_ACTIONS) return false;
    if ('layouts' in value && (!Array.isArray(value['layouts']) ||
        value['layouts'].length > MAX_CONDITIONAL_INPUT_LAYOUTS || !value['layouts'].every(layoutShape))) return false;
    return value['actions'].every(action => record(action) && Object.keys(action).length === 4 && typeof action['id'] === 'string' &&
        typeof action['label'] === 'string' && ACTION_VALUES.some(item => item.value === action['value']) &&
        Array.isArray(action['bindings']) && action['bindings'].length <= MAX_ACTION_BINDINGS &&
        action['bindings'].every(bindingShape));
}
