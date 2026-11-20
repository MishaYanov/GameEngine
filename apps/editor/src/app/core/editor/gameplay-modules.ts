import type {GameplayModuleDescriptor, GameplayRoleDescriptor} from './editor.models';

function record(value: unknown): value is Record<string, unknown> {
    return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function visibleText(value: unknown): value is string {
    return typeof value === 'string' && value.trim().length > 0 && !/\p{Cc}/u.test(value);
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
