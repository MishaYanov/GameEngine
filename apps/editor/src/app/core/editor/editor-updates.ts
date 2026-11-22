import {
    type EditorCommandUpdate, type EditorSnapshot, type SceneEntity, isSceneSchemaVersion,
} from './editor.models';
import {validComponentViews} from './editor-components';

const maxSceneFolders = 1_024;
const maxFolderDepth = 32;
const maxFolderNameBytes = 128;
const utf8 = new TextEncoder();

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

function orbitCamera(value: unknown): boolean {
    return record(value) && typeof value['target'] === 'string' &&
        value['target'].length > 0 && vector(value['pivot_offset'], 3) &&
        ['initial_yaw', 'initial_pitch', 'initial_distance', 'min_distance', 'max_distance',
            'min_pitch', 'max_pitch', 'follow_seconds', 'recovery_seconds', 'teleport_distance',
            'probe_radius'].every(key => Number.isFinite(value[key])) &&
        integer(value['obstruction_mask']) && value['obstruction_mask'] > 0 &&
        value['obstruction_mask'] <= 0xffff_ffff;
}

function camera(value: unknown, schema: unknown): boolean {
    const follow = record(value) ? value['follow'] : undefined;
    const validFollow = follow == null || (record(follow) && typeof follow['target'] === 'string' &&
        vector(follow['offset'], 3) && vector(follow['look_offset'], 3) &&
        typeof follow['smoothing_seconds'] === 'number' && Number.isFinite(follow['smoothing_seconds']) &&
        follow['smoothing_seconds'] >= 0 && follow['smoothing_seconds'] <= 10);
    return (
        record(value) &&
        validFollow &&
        (schema === 5 || schema === 6
            ? (value['orbit'] == null || (follow == null && orbitCamera(value['orbit'])))
            : !('orbit' in value)) &&
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
        (value['component_views'] === undefined || validComponentViews(value['component_views'])) &&
        (value['linked_prefab'] == null || linkedPrefab(value['linked_prefab'])) &&
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

function linkedPrefab(value: unknown): boolean {
    if (!record(value) || !record(value['source']) || !record(value['bindings'])) return false;
    return typeof value['source']['source_id'] === 'string' && typeof value['source']['path'] === 'string' &&
        Object.values(value['bindings']).every(key => typeof key === 'string');
}

function folderId(value: unknown): value is string {
    return typeof value === 'string' && /^[\w-]{1,128}$/.test(value);
}

function validOrganization(value: unknown, entities: readonly SceneEntity[]): boolean {
    if (!record(value) || !Array.isArray(value['folders']) || value['folders'].length > maxSceneFolders ||
        !record(value['root_membership'])) {
        return false;
    }
    const parents = new Map<string, string | null>();
    for (const folder of value['folders']) {
        if (!record(folder)) return false;
        const id = folder['id'];
        const name = folder['name'];
        const parent = folder['parent'];
        if (!folderId(id) || parents.has(id) || typeof name !== 'string' || !name ||
            name !== name.trim() || /\p{Cc}/u.test(name) || utf8.encode(name).length > maxFolderNameBytes ||
            (parent != null && !folderId(parent))) {
            return false;
        }
        parents.set(id, parent ?? null);
    }
    for (const parent of parents.values()) {
        let next = parent;
        let depth = 1;
        while (next !== null) {
            const ancestor = parents.get(next);
            if (ancestor === undefined || ++depth > maxFolderDepth) return false;
            next = ancestor;
        }
    }
    const roots = new Set(entities.filter(item => item.parent == null).map(item => item.key));
    return Object.entries(value['root_membership']).every(
        ([root, folder]) => roots.has(root) && folderId(folder) && parents.has(folder),
    );
}

function validDocument(value: unknown): boolean {
    return (
        record(value) &&
        isSceneSchemaVersion(value['schema_version']) &&
        record(value['bindings']) &&
        Object.values(value['bindings']).every((key) => typeof key === 'string') &&
        camera(value['camera'], value['schema_version']) &&
        Array.isArray(value['lights']) &&
        value['lights'].every(light) &&
        Array.isArray(value['entities']) &&
        value['entities'].every(entity) &&
        new Set(value['entities'].map((item) => item.key)).size === value['entities'].length &&
        (value['organization'] === undefined ||
            (value['schema_version'] === 6 && validOrganization(value['organization'], value['entities'])))
    );
}

function project(value: unknown): boolean {
    if (!record(value)) return false;
    const gameplayModule = value['gameplay_module'];
    return (
        value['schema_version'] === 3 &&
        typeof value['name'] === 'string' &&
        typeof value['startup_scene_id'] === 'string' &&
        Number.isFinite(value['fixed_hz']) &&
        integer(value['max_catchup_steps']) &&
        (gameplayModule === undefined || gameplayModule === null || typeof gameplayModule === 'string') &&
        Array.isArray(value['behaviors']) &&
        Array.isArray(value['behavior_definitions']) &&
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
    const playSessionId = value['play_session_id'];
    return (
        integer(value['revision']) &&
        integer(value['scene_revision']) &&
        typeof value['active_scene_id'] === 'string' &&
        strings(value['selection']) &&
        (value['mode'] === 'edit' || value['mode'] === 'play') &&
        (playSessionId === undefined || playSessionId === null ||
            (value['mode'] === 'play' && typeof playSessionId === 'string' && playSessionId.length > 0)) &&
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
        (receipt['organization_changed'] !== undefined && typeof receipt['organization_changed'] !== 'boolean') ||
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
        ((value['organization'] !== undefined) !== (receipt['organization_changed'] === true)) ||
        !Array.isArray(value['entities']) ||
        !value['entities'].every(entity) ||
        (changes['camera_changed'] ? !camera(value['camera'], previous.document.schema_version) : value['camera'] !== null) ||
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
    if (update.organization !== undefined &&
        (previous.document.schema_version !== 6 || !validOrganization(update.organization, entities))) {
        return invalid();
    }
    const document =
        replacements.size === 0 && update.camera === null && update.lights === null && update.organization === undefined
            ? previous.document
            : {
                ...previous.document,
                entities,
                camera: update.camera ?? previous.document.camera,
                lights: update.lights ?? previous.document.lights,
                ...(update.organization === undefined ? {} : {organization: update.organization}),
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
        play_session_id: update.state.play_session_id ?? null,
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
