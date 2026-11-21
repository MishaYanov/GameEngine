export type CourtyardSceneSettings =
    | {readonly kind: 'course'; readonly key_trigger: string; readonly exit_trigger: string; readonly destination_scene: string; readonly exit_blocker?: string | null}
    | {readonly kind: 'finish'; readonly finish_trigger: string};

export interface CourtyardSettings {
    readonly schema_version: 1;
    readonly scenes: Readonly<Record<string, CourtyardSceneSettings>>;
    readonly route_gate?: {
        readonly scene_id: string;
        readonly lever: string;
        readonly gate: string;
        readonly action: string;
        readonly radius: number;
    } | null;
    readonly project_save_id?: string | null;
    readonly checkpoints?: readonly {
        readonly id: string;
        readonly scene_id: string;
        readonly player: string;
        readonly spawn: readonly [number, number, number];
        readonly activation_radius: number;
        readonly requires_key?: boolean;
        readonly requires_gate?: boolean;
    }[];
}

function record(value: unknown): value is Record<string, unknown> {
    return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function identifier(value: unknown): value is string {
    return typeof value === 'string' && value.length > 0 && new TextEncoder().encode(value).length <= 256 &&
        !/[\s\p{Cc}]/u.test(value);
}

function keys(value: Record<string, unknown>, allowed: readonly string[]): boolean {
    return Object.keys(value).every(key => allowed.includes(key));
}

function within(value: unknown, min: number, max: number): boolean {
    return typeof value === 'number' && Number.isFinite(value) && value >= min && value <= max;
}

function route(value: unknown): boolean {
    return value == null || (record(value) && keys(value, ['scene_id', 'lever', 'gate', 'action', 'radius']) &&
        identifier(value['scene_id']) && identifier(value['lever']) && identifier(value['gate']) &&
        typeof value['action'] === 'string' && /^[a-z][a-z0-9_]{0,31}$/.test(value['action']) &&
        typeof value['radius'] === 'number' && value['radius'] > 0 && value['radius'] <= 3);
}

function checkpoint(value: unknown): boolean {
    if (!record(value) || !keys(value, ['id', 'scene_id', 'player', 'spawn', 'activation_radius', 'requires_key', 'requires_gate'])) return false;
    const spawn: unknown = value['spawn'];
    return typeof value['id'] === 'string' && /^[A-Za-z0-9_-]{1,64}$/.test(value['id']) &&
        identifier(value['scene_id']) && identifier(value['player']) &&
        Array.isArray(spawn) && spawn.length === 3 && spawn.every(position => within(position, -1_000_000, 1_000_000)) &&
        within(value['activation_radius'], 0.01, 8) &&
        (value['requires_key'] === undefined || typeof value['requires_key'] === 'boolean') &&
        (value['requires_gate'] === undefined || typeof value['requires_gate'] === 'boolean');
}

export function readCourtyardSettings(value: unknown): CourtyardSettings | null {
    if (value === undefined || value === null) return {schema_version: 1, scenes: {}};
    if (!record(value) || !keys(value, ['schema_version', 'scenes', 'route_gate', 'project_save_id', 'checkpoints']) ||
        value['schema_version'] !== 1 || !record(value['scenes']) || !route(value['route_gate'])) return null;
    const identity = value['project_save_id'];
    if (identity != null && (typeof identity !== 'string' ||
        !/^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/.test(identity))) return null;
    const checkpoints = value['checkpoints'];
    if (checkpoints !== undefined && (!Array.isArray(checkpoints) || !checkpoints.every(checkpoint))) return null;
    const valid = Object.entries(value['scenes']).every(([scene, settings]) => identifier(scene) && record(settings) &&
        (settings['kind'] === 'finish' ? keys(settings, ['kind', 'finish_trigger']) && identifier(settings['finish_trigger'])
            : settings['kind'] === 'course' && keys(settings, ['kind', 'key_trigger', 'exit_trigger', 'destination_scene', 'exit_blocker']) &&
            identifier(settings['key_trigger']) && identifier(settings['exit_trigger']) &&
            identifier(settings['destination_scene']) && (settings['exit_blocker'] == null || identifier(settings['exit_blocker']))));
    return valid ? value as unknown as CourtyardSettings : null;
}

export function courtyardScene(settings: CourtyardSettings, sceneId: string): CourtyardSceneSettings | undefined {
    return Object.hasOwn(settings.scenes, sceneId) ? settings.scenes[sceneId] : undefined;
}

export function updateCourtyardScene(
    settings: CourtyardSettings, sceneId: string, value: CourtyardSceneSettings | null,
): CourtyardSettings {
    const scenes = {...settings.scenes};
    if (value === null) delete scenes[sceneId];
    else Object.defineProperty(scenes, sceneId, {value, enumerable: true, configurable: true, writable: true});
    return {...settings, scenes};
}
