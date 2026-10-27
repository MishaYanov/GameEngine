import type {EditorCommandUpdate, EditorSnapshot, SceneEntity} from './editor.models';

function record(value: unknown): value is Record<string, unknown> {
    return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function strings(value: unknown): value is string[] {
    return Array.isArray(value) && value.every((item: unknown) => typeof item === 'string');
}

function integer(value: unknown): value is number {
    return typeof value === 'number' && Number.isSafeInteger(value) && value >= 0;
}

function vector(value: unknown, length: number): boolean {
    return Array.isArray(value) && value.length === length && value.every(Number.isFinite);
}

function camera(value: unknown): boolean {
    return (
        record(value) &&
        vector(value['position'], 3) &&
        ['yaw', 'pitch', 'vertical_fov', 'near', 'far'].every((key) => Number.isFinite(value[key]))
    );
}

function entity(value: unknown): value is SceneEntity {
    if (!record(value) || !record(value['transform'])) return false;
    const transform = value['transform'];
    return (
        typeof value['key'] === 'string' &&
        typeof value['name'] === 'string' &&
        typeof value['model_path'] === 'string' &&
        typeof value['renderable'] === 'boolean' &&
        ['position', 'rotation', 'scale'].every((key) => vector(transform[key], 3))
    );
}

function light(value: unknown): boolean {
    return (
        record(value) &&
        typeof value['key'] === 'string' &&
        typeof value['name'] === 'string' &&
        (value['kind'] === 'point' || value['kind'] === 'directional') &&
        ['position', 'direction', 'color'].every((key) => vector(value[key], 3)) &&
        Number.isFinite(value['intensity']) &&
        Number.isFinite(value['range'])
    );
}

function validDocument(value: unknown): boolean {
    return (
        record(value) &&
        value['schema_version'] === 3 &&
        record(value['bindings']) &&
        Object.values(value['bindings']).every((key) => typeof key === 'string') &&
        camera(value['camera']) &&
        Array.isArray(value['lights']) &&
        value['lights'].every(light) &&
        Array.isArray(value['entities']) &&
        value['entities'].every(entity) &&
        new Set(value['entities'].map((item) => item.key)).size === value['entities'].length
    );
}

function project(value: unknown): boolean {
    return (
        record(value) &&
        value['schema_version'] === 2 &&
        typeof value['name'] === 'string' &&
        typeof value['startup_scene_id'] === 'string' &&
        Number.isFinite(value['fixed_hz']) &&
        integer(value['max_catchup_steps']) &&
        Array.isArray(value['behaviors']) &&
        Array.isArray(value['scenes']) &&
        value['scenes'].every(
            (scene: unknown) =>
                record(scene) && typeof scene['id'] === 'string' && typeof scene['asset_path'] === 'string',
        )
    );
}

function state(value: unknown): boolean {
    if (!record(value) || !record(value['snap'])) return false;
    const snap = value['snap'];
    return (
        integer(value['revision']) &&
        integer(value['scene_revision']) &&
        typeof value['active_scene_id'] === 'string' &&
        strings(value['selection']) &&
        (value['mode'] === 'edit' || value['mode'] === 'play') &&
        ['project_dirty', 'paused', 'dirty', 'can_undo', 'can_redo'].every(
            (key) => typeof value[key] === 'boolean',
        ) &&
        (value['transaction'] === null || typeof value['transaction'] === 'string') &&
        Array.isArray(value['scenes']) &&
        value['scenes'].every(
            (scene: unknown) =>
                record(scene) &&
                typeof scene['id'] === 'string' &&
                typeof scene['asset_path'] === 'string' &&
                typeof scene['loaded'] === 'boolean' &&
                typeof scene['dirty'] === 'boolean',
        ) &&
        typeof snap['enabled'] === 'boolean' &&
        ['translation', 'rotation_radians', 'scale'].every((key) => Number.isFinite(snap[key]))
    );
}

function invalid(): never {
    throw new Error('The editor returned an inconsistent incremental update; refresh is required.');
}

/** Validate the whole update before publishing any new state. Failed replies never replay edits. */
export function applyEditorUpdate(previous: EditorSnapshot, value: unknown): EditorSnapshot {
    if (!record(value) || !record(value['receipt'])) return invalid();
    const receipt = value['receipt'];
    const changes = receipt['changes'];
    if (
        !integer(receipt['revision']) ||
        receipt['revision'] < previous.revision ||
        !integer(receipt['scene_revision']) ||
        !integer(receipt['change_cursor']) ||
        typeof receipt['active_scene_id'] !== 'string' ||
        !strings(receipt['affected_scenes']) ||
        typeof receipt['state_changed'] !== 'boolean' ||
        typeof receipt['project_changed'] !== 'boolean' ||
        !record(changes) ||
        !strings(changes['changed_entities']) ||
        !strings(changes['removed_entities']) ||
        !['camera_changed', 'lights_changed', 'topology_changed', 'full_resync'].every(
            (key) => typeof changes[key] === 'boolean',
        )
    )
        return invalid();
    const next = value['kind'] === 'snapshot' ? value['snapshot'] : value['state'];
    if (
        !state(next) ||
        !record(next) ||
        next['revision'] !== receipt['revision'] ||
        next['scene_revision'] !== receipt['scene_revision'] ||
        next['active_scene_id'] !== receipt['active_scene_id']
    )
        return invalid();
    if (value['kind'] === 'snapshot') {
        if (!validDocument(next['document']) || !project(next['project'])) return invalid();
        return (value as unknown as Extract<EditorCommandUpdate, { kind: 'snapshot' }>).snapshot;
    }
    if (
        value['kind'] !== 'delta' ||
        receipt['active_scene_id'] !== previous.active_scene_id ||
        receipt['scene_revision'] < previous.scene_revision ||
        changes['full_resync'] ||
        changes['topology_changed'] ||
        receipt['project_changed'] ||
        changes['removed_entities'].length > 0 ||
        !Array.isArray(value['entities']) ||
        !value['entities'].every(entity) ||
        (changes['camera_changed'] ? !camera(value['camera']) : value['camera'] !== null) ||
        (changes['lights_changed']
            ? !Array.isArray(value['lights']) || !value['lights'].every(light)
            : value['lights'] !== null)
    )
        return invalid();
    const update = value as unknown as Extract<EditorCommandUpdate, { kind: 'delta' }>;
    const replacements = new Map(update.entities.map((item) => [item.key, item]));
    const changed = new Set(update.receipt.changes.changed_entities);
    if (
        replacements.size !== update.entities.length ||
        changed.size !== update.receipt.changes.changed_entities.length ||
        changed.size !== replacements.size ||
        [...changed].some((key) => !replacements.has(key))
    )
        return invalid();
    let replaced = 0;
    const entities =
        replacements.size === 0
            ? previous.document.entities
            : previous.document.entities.map((item) => {
                const replacement = replacements.get(item.key);
                if (replacement) replaced++;
                return replacement ?? item;
            });
    if (replaced !== replacements.size) return invalid();
    const document =
        replacements.size === 0 && update.camera === null && update.lights === null
            ? previous.document
            : {
                ...previous.document,
                entities,
                camera: update.camera ?? previous.document.camera,
                lights: update.lights ?? previous.document.lights,
            };
    // Assign state fields explicitly so unexpected wire keys cannot overwrite document/project.
    return {
        active_scene_id: update.state.active_scene_id,
        project_dirty: update.state.project_dirty,
        scenes: update.state.scenes,
        revision: update.state.revision,
        scene_revision: update.state.scene_revision,
        selection: update.state.selection,
        mode: update.state.mode,
        paused: update.state.paused,
        dirty: update.state.dirty,
        can_undo: update.state.can_undo,
        can_redo: update.state.can_redo,
        transaction: update.state.transaction,
        snap: update.state.snap,
        document,
        project: previous.project,
    };
}
