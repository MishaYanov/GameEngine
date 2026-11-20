export type CourtyardSceneSettings =
    | {readonly kind: 'course'; readonly key_trigger: string; readonly exit_trigger: string; readonly destination_scene: string; readonly exit_blocker?: string | null}
    | {readonly kind: 'finish'; readonly finish_trigger: string};

export interface CourtyardSettings {
    readonly schema_version: 1;
    readonly scenes: Readonly<Record<string, CourtyardSceneSettings>>;
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

export function readCourtyardSettings(value: unknown): CourtyardSettings | null {
    if (value === undefined || value === null) return {schema_version: 1, scenes: {}};
    if (!record(value) || !keys(value, ['schema_version', 'scenes']) || value['schema_version'] !== 1 || !record(value['scenes'])) return null;
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
