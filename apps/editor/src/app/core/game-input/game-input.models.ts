export const MOVEMENT_AXES = [
    {field: 'negative_x', label: 'Move left'},
    {field: 'positive_x', label: 'Move right'},
    {field: 'negative_y', label: 'Move backward'},
    {field: 'positive_y', label: 'Move forward'},
] as const;

export type MovementAxis = (typeof MOVEMENT_AXES)[number]['field'];

export interface GameActionBinding {
    readonly id: string;
    readonly key: string;
    readonly animation_parameter?: string | null;
}

export interface GameInputProfile {
    readonly schema_version: 1;
    readonly movement: Readonly<Record<MovementAxis, string>>;
    readonly actions?: readonly GameActionBinding[];
}

export const DEFAULT_GAME_INPUT: GameInputProfile = {
    schema_version: 1,
    movement: {negative_x: 'a', positive_x: 'd', negative_y: 's', positive_y: 'w'},
};

export const MAX_GAME_ACTIONS = 24;

export const GAME_KEYS: readonly { key: string; label: string }[] = [
    ...Array.from('abcdefghijklmnopqrstuvwxyz', key => ({key, label: key.toUpperCase()})),
    ...Array.from({length: 10}, (_, number) => ({key: `digit${number}`, label: String(number)})),
    {key: 'arrow_up', label: 'Up arrow'},
    {key: 'arrow_down', label: 'Down arrow'},
    {key: 'arrow_left', label: 'Left arrow'},
    {key: 'arrow_right', label: 'Right arrow'},
    {key: 'space', label: 'Space'},
    {key: 'enter', label: 'Enter'},
    {key: 'shift_left', label: 'Left Shift'},
    {key: 'shift_right', label: 'Right Shift'},
    {key: 'control_left', label: 'Left Ctrl'},
    {key: 'control_right', label: 'Right Ctrl'},
    {key: 'alt_left', label: 'Left Alt'},
    {key: 'alt_right', label: 'Right Alt'},
    ...Array.from({length: 10}, (_, number) => ({key: `numpad${number}`, label: `Numpad ${number}`})),
];

/** Convert a physical browser code to the serialized Rust Key name. */
export function gameKeyFromCode(code: string): string {
    if (/^Key[A-Z]$/.test(code)) return code.slice(3).toLowerCase();
    return code.replace(/^Meta(?=Left|Right)/, 'Super')
        .replace(/([a-z0-9])([A-Z])/g, '$1_$2').toLowerCase();
}

export function validGameActionId(value: unknown): value is string {
    return typeof value === 'string' && /^[a-z][a-z0-9_]{0,31}$/.test(value);
}

export function validateGameInput(profile: GameInputProfile): string | null {
    const movementKeys = MOVEMENT_AXES.map(axis => profile.movement[axis.field]);
    if (movementKeys.some(key => !key || key === 'escape')) return 'Choose a movement key other than Escape.';
    const actions = profile.actions ?? [];
    if (actions.length > MAX_GAME_ACTIONS) return `Add up to ${MAX_GAME_ACTIONS} game actions.`;
    const ids = new Set<string>();
    for (const [index, action] of actions.entries()) {
        if (!validGameActionId(action.id)) {
            return `Action ${index + 1} needs a name such as jump or use_item.`;
        }
        if (ids.has(action.id)) return `The action name ${action.id} is already in use.`;
        ids.add(action.id);
        if (!action.key || action.key === 'escape') return `Choose a key other than Escape for ${action.id}.`;
        if (action.animation_parameter && !/^[A-Za-z_][A-Za-z0-9_]{0,31}$/.test(action.animation_parameter)) {
            return `The animation parameter for ${action.id} needs an identifier such as isJumping.`;
        }
    }
    return null;
}

export interface GameInputFrame {
    readonly movement: readonly [number, number];
    readonly actions: readonly string[];
    readonly animation: ReadonlyMap<string, boolean>;
}

/** Preview the same held-key semantics that the native game input frame uses. */
export function evaluateGameInput(profile: GameInputProfile, keys: ReadonlySet<string>): GameInputFrame {
    const {movement} = profile;
    const x = Number(keys.has(movement.positive_x)) - Number(keys.has(movement.negative_x));
    const y = Number(keys.has(movement.positive_y)) - Number(keys.has(movement.negative_y));
    const length = Math.hypot(x, y) || 1;
    const actions: string[] = [];
    const animation = new Map<string, boolean>();
    for (const action of profile.actions ?? []) {
        const held = keys.has(action.key);
        if (held && action.id) actions.push(action.id);
        const parameter = action.animation_parameter;
        if (parameter) animation.set(parameter, held || animation.get(parameter) === true);
    }
    return {movement: [x / length, y / length], actions, animation};
}
