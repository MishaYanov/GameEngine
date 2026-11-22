import type {GameplayModuleDescriptor, GameplayRoleDescriptor, SceneId} from './editor.models';

function record(value: unknown): value is Record<string, unknown> {
    return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function visibleText(value: unknown): value is string {
    return typeof value === 'string' && value.trim().length > 0 && !/\p{Cc}/u.test(value);
}

/** Cinder Oath route settings can give the intro and arena custom scene IDs. */
export function rolesForScene(
    module: GameplayModuleDescriptor,
    sceneId: SceneId,
    settings: unknown,
): readonly GameplayRoleDescriptor[] {
    let roleScene = sceneId;
    if (module.id === 'cinder_oath' && record(settings)) {
        const intro = settings['intro_scene'];
        const arena = settings['arena_scene'];
        if (visibleText(intro) && visibleText(arena) && intro !== arena) {
            if (sceneId === intro) roleScene = 'intro';
            else if (sceneId === arena) roleScene = 'arena';
            else return module.default_roles;
        }
    }
    return Object.hasOwn(module.scene_roles, roleScene) ? module.scene_roles[roleScene] : module.default_roles;
}

function roles(value: unknown): value is readonly GameplayRoleDescriptor[] {
    return Array.isArray(value) && value.every((role: unknown) =>
        record(role) &&
        visibleText(role['name']) && new TextEncoder().encode(role['name']).length <= 1024 &&
        visibleText(role['label']) && visibleText(role['requirement_hint']),
    ) && new Set(value.map(role => role.name)).size === value.length;
}

/** Keep backend metadata within the same identifier contracts as authored commands. */
export function validGameplayModules(value: unknown): value is readonly GameplayModuleDescriptor[] {
    if (!Array.isArray(value)) return false;
    const valid = value.every((module: unknown) => {
        if (!record(module) || typeof module['id'] !== 'string' ||
            !/^[a-z][a-z0-9_]{0,31}$/.test(module['id']) ||
            !visibleText(module['label']) || !visibleText(module['description']) ||
            !roles(module['default_roles']) || !record(module['scene_roles'])) return false;
        return Object.entries(module['scene_roles']).every(([sceneId, sceneRoles]) =>
            sceneId.length > 0 && new TextEncoder().encode(sceneId).length <= 256 &&
            !/[\s\p{Cc}]/u.test(sceneId) && roles(sceneRoles),
        );
    });
    return valid && new Set(value.map(module => module.id)).size === value.length;
}
