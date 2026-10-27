import type {GraphDocument} from '../graph.models';

export const MOTION_SLOTS = [
    {id: 'idle', label: 'Idle', air: false, directional: false},
    {id: 'walk', label: 'Walk', air: false, directional: false},
    {id: 'run', label: 'Run', air: false, directional: false},
    {id: 'backward', label: 'Walk backward', air: false, directional: true},
    {id: 'left', label: 'Strafe left', air: false, directional: true},
    {id: 'right', label: 'Strafe right', air: false, directional: true},
    {id: 'runBackward', label: 'Run backward', air: false, directional: true},
    {id: 'runLeft', label: 'Run left', air: false, directional: true},
    {id: 'runRight', label: 'Run right', air: false, directional: true},
    {id: 'jump', label: 'Jump start', air: true, directional: false},
    {id: 'rise', label: 'Rising', air: true, directional: false},
    {id: 'fall', label: 'Falling', air: true, directional: false},
    {id: 'land', label: 'Landing', air: true, directional: false},
] as const;
export type MotionSlot = (typeof MOTION_SLOTS)[number]['id'];
export type StateMotion = 'grounded' | 'jump' | 'rise' | 'fall' | 'land' | 'reference';

export interface AnimationClipAsset {
    readonly id: string;
    readonly name: string;
    readonly duration: number;
    readonly skeletonId?: string;
}

export interface LocomotionValues {
    motions: Record<MotionSlot, string>;
    exposed: Record<MotionSlot, boolean>;
    blend: {
        mode: 'single' | 'speed1d' | 'direction2d';
        walkSpeed: number;
        runSpeed: number;
        synchronize: boolean;
    };
    air: { enabled: boolean };
    skeletonId: string;
    rootMotion: 'in_place' | 'extract';
    emitEvents: boolean;
    machine: GraphDocument;
}

export type TransitionParameter = 'grounded' | 'jump' | 'verticalSpeed' | 'stateTime';
export type TransitionOperator = 'true' | 'false' | 'greater' | 'less';

export interface TransitionCondition {
    parameter: TransitionParameter;
    operator: TransitionOperator;
    value: number;
}

export interface TransitionSettings {
    duration: number;
    enabled: boolean;
    useExitTime: boolean;
    exitTime: number;
    conditions: TransitionCondition[];
}

export const TRANSITION_PARAMETERS: readonly TransitionParameter[] = [
    'grounded',
    'jump',
    'verticalSpeed',
    'stateTime',
];
export const DEFAULT_TRANSITION: Readonly<TransitionSettings> = {
    duration: 0.15,
    enabled: true,
    useExitTime: false,
    exitTime: 1,
    conditions: [],
};

export function validTransition(settings: TransitionSettings): boolean {
    return (
        !!settings &&
        typeof settings.enabled === 'boolean' &&
        typeof settings.useExitTime === 'boolean' &&
        Number.isFinite(settings.duration) &&
        settings.duration >= 0 &&
        Number.isFinite(settings.exitTime) &&
        settings.exitTime >= 0 &&
        Array.isArray(settings.conditions) &&
        settings.conditions.every((condition) => {
            if (!condition) return false;
            const boolean = condition.parameter === 'grounded' || condition.parameter === 'jump';
            return (
                TRANSITION_PARAMETERS.includes(condition.parameter) &&
                Number.isFinite(condition.value) &&
                (boolean
                    ? ['true', 'false'].includes(condition.operator)
                    : ['greater', 'less'].includes(condition.operator))
            );
        })
    );
}

export function defaultLocomotion(): LocomotionValues {
    return {
        motions: Object.fromEntries(MOTION_SLOTS.map((slot) => [slot.id, ''])) as Record<
            MotionSlot,
            string
        >,
        exposed: Object.fromEntries(MOTION_SLOTS.map((slot) => [slot.id, false])) as Record<
            MotionSlot,
            boolean
        >,
        blend: {mode: 'speed1d', walkSpeed: 2, runSpeed: 5, synchronize: true},
        air: {enabled: false},
        skeletonId: '',
        rootMotion: 'in_place',
        emitEvents: false,
        machine: defaultLocomotionMachine(),
    };
}

export function defaultLocomotionMachine(): GraphDocument {
    const transition = (id: string, from: string, to: string, conditions: TransitionCondition[]) => ({
        id,
        kind: 'transition' as const,
        from: {node: from, port: 'next'},
        to: {node: to, port: 'enter'},
        settings: {...DEFAULT_TRANSITION, conditions},
    });
    return {
        version: 1,
        domain: 'animation',
        kind: 'stateMachine',
        nodes: [
            {id: 'entry', kind: 'animation.entry', position: {x: 20, y: 150}, values: {}},
            {
                id: 'grounded',
                kind: 'animation.state',
                position: {x: 260, y: 150},
                values: {name: 'Grounded', motion: 'grounded'},
            },
            {
                id: 'jump',
                kind: 'animation.state',
                position: {x: 520, y: 20},
                values: {name: 'Jump start', motion: 'jump'},
            },
            {
                id: 'air',
                kind: 'animation.state',
                position: {x: 520, y: 180},
                values: {name: 'In air', motion: 'fall'},
            },
            {
                id: 'land',
                kind: 'animation.state',
                position: {x: 260, y: 340},
                values: {name: 'Landing', motion: 'land'},
            },
        ],
        connections: [
            transition('start', 'entry', 'grounded', []),
            transition('jump-request', 'grounded', 'jump', [
                {parameter: 'jump', operator: 'true', value: 0},
            ]),
            transition('leave-ground', 'grounded', 'air', [
                {parameter: 'grounded', operator: 'false', value: 0},
            ]),
            transition('takeoff', 'jump', 'air', [
                {parameter: 'grounded', operator: 'false', value: 0},
            ]),
            transition('touchdown', 'air', 'land', [
                {parameter: 'grounded', operator: 'true', value: 0},
            ]),
            transition('recover', 'land', 'grounded', [
                {parameter: 'stateTime', operator: 'greater', value: 0.9},
            ]),
        ],
    };
}

export function slotVisible(
    values: LocomotionValues,
    slot: (typeof MOTION_SLOTS)[number],
): boolean {
    if (slot.air) return values.air.enabled;
    if (values.blend.mode === 'single') return slot.id === 'idle';
    return !slot.directional || values.blend.mode === 'direction2d';
}

/** Editor-side weight preview. Pose evaluation and asset sampling remain native responsibilities. */
export function locomotionWeights(
    values: LocomotionValues,
    x: number,
    forward: number,
): Readonly<Record<string, number>> {
    if (values.blend.mode === 'single') return {idle: 1};
    const speed = Math.hypot(x, forward);
    const walk = Math.max(0.01, values.blend.walkSpeed);
    const run = Math.max(walk + 0.01, values.blend.runSpeed);
    const moving = Math.min(1, speed / walk);
    const fast = Math.max(0, Math.min(1, (speed - walk) / (run - walk)));
    const result: Record<string, number> = {idle: 1 - moving};
    const directions = ['walk', 'right', 'backward', 'left'] as const;
    const runs = ['run', 'runRight', 'runBackward', 'runLeft'] as const;
    const direction =
        values.blend.mode === 'direction2d' ? (Math.atan2(x, forward) / (Math.PI / 2) + 4) % 4 : 0;
    const first = Math.floor(direction);
    const alpha = direction - first;
    for (const [index, weight] of [
        [first, 1 - alpha],
        [(first + 1) % 4, alpha],
    ]) {
        const walkSlot = directions[index];
        const runSlot = runs[index];
        result[walkSlot] = (result[walkSlot] ?? 0) + moving * (1 - fast) * weight;
        result[runSlot] = (result[runSlot] ?? 0) + fast * weight;
    }
    return Object.fromEntries(Object.entries(result).filter(([, weight]) => weight > 0.001));
}

export interface AnimationNodeValues {
    'animation.locomotion': LocomotionValues;
    'animation.state': { name: string; motion: StateMotion };
    'animation.entry': Record<string, never>;
    'animation.any': Record<string, never>;
    'animation.exit': Record<string, never>;
    'animation.reference': Record<string, never>;
    'animation.sample': { clip: string; time: number };
    'animation.mixer': { weightA: number; weightB: number; weightC: number };
    'animation.blend1d': { thresholdA: number; thresholdB: number; thresholdC: number };
    'animation.additive': { weight: number };
    'animation.layers': { weight: number; additive: boolean; mask: string };
    'animation.masked': { weight: number; mask: string };
    'animation.select': { index: number };
    'animation.subgraph': { assetId: string };
    'animation.events': Record<string, never>;
    'animation.debug': Record<string, never>;
    'parameter.float': { name: string; value: number };
    'parameter.int': { name: string; value: number };
    'parameter.bool': { name: string; value: boolean };
    'parameter.trigger': { name: string };
    'parameter.vector': { name: string; x: number; y: number; z: number };
    'logic.compare': { operator: 'greater' | 'less' | 'equal'; threshold: number };
    'logic.boolean': { operator: 'and' | 'or' | 'not' };
    'math.scalar': { operator: 'add' | 'subtract' | 'multiply' | 'divide'; value: number };
    'math.remap': {
        inputMin: number;
        inputMax: number;
        outputMin: number;
        outputMax: number;
        clamp: boolean;
    };
    'graph.poseInput': { name: string };
}
