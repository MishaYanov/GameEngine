import type {NativeBehaviorSpec} from '../../../../core/editor/editor-components';
import type {EntityKey, SceneDocument, SceneEntity} from '../../../../core/editor/editor.models';

export type SceneReferenceLocation =
    | {
    readonly kind: 'entity';
    readonly key: EntityKey;
    readonly componentId?: string;
    readonly section?: 'prefab' | 'hierarchy';
    readonly slot?: string;
}
    | { readonly kind: 'camera'; readonly mode: 'follow' | 'orbit' }
    | { readonly kind: 'binding'; readonly name: string };

export interface SceneReference {
    readonly id: string;
    readonly group: 'Camera' | 'Hierarchy' | 'Gameplay Roles' | 'Linked Prefabs' | 'Behaviors' | 'State Machines';
    readonly sourceName: string;
    readonly field: string;
    readonly location: SceneReferenceLocation;
    readonly targetKey: EntityKey;
    readonly targetComponentId?: string;
    readonly issue: 'missing-entity' | 'missing-component' | null;
}

type Reference = Omit<SceneReference, 'id' | 'issue'>;

/** Scan the active authored scene, including projected and legacy component data. */
export function collectSceneReferences(document: SceneDocument): readonly SceneReference[] {
    const entities = new Map(document.entities.map(entity => [entity.key, entity]));
    const results: SceneReference[] = [];
    const add = (reference: Reference): void => {
        const target = entities.get(reference.targetKey);
        const issue = !target ? 'missing-entity' : reference.targetComponentId !== undefined &&
        !hasMachine(target, reference.targetComponentId) ? 'missing-component' : null;
        results.push({
            ...reference,
            id: JSON.stringify([reference.location, reference.field]),
            issue,
        });
    };

    if (document.camera.follow) {
        add({
            group: 'Camera', sourceName: 'Game Camera', field: 'Follow target',
            location: {kind: 'camera', mode: 'follow'}, targetKey: document.camera.follow.target,
        });
    }
    if (document.camera.orbit) {
        add({
            group: 'Camera', sourceName: 'Game Camera', field: 'Orbit target',
            location: {kind: 'camera', mode: 'orbit'}, targetKey: document.camera.orbit.target,
        });
    }
    for (const [name, targetKey] of Object.entries(document.bindings)) {
        add({
            group: 'Gameplay Roles', sourceName: 'Scene', field: name,
            location: {kind: 'binding', name}, targetKey,
        });
    }
    for (const entity of document.entities) {
        if (entity.parent != null) {
            add({
                group: 'Hierarchy', sourceName: entity.name, field: 'Parent',
                location: {kind: 'entity', key: entity.key, section: 'hierarchy'}, targetKey: entity.parent,
            });
        }
        if (entity.linked_prefab) {
            for (const [slot, targetKey] of Object.entries(entity.linked_prefab.bindings)) {
                add({
                    group: 'Linked Prefabs', sourceName: entity.name, field: `Slot: ${slot}`,
                    location: {kind: 'entity', key: entity.key, section: 'prefab', slot}, targetKey,
                });
            }
        }
        for (const {id, behavior} of nativeBehaviors(entity)) {
            behaviorReferences(behavior, (field, targetKey, targetComponentId) => add({
                group: behavior.kind === 'state_machine' ? 'State Machines' : 'Behaviors',
                sourceName: entity.name,
                location: {kind: 'entity', key: entity.key, componentId: id},
                field,
                targetKey,
                targetComponentId,
            }));
        }
    }
    return results;
}

function nativeBehaviors(entity: SceneEntity): readonly {
    readonly id: string;
    readonly behavior: NativeBehaviorSpec
}[] {
    return entity.component_views?.flatMap(component => component.data.kind === 'native_behavior'
        ? [{id: component.id, behavior: component.data.behavior}] : []) ?? entity.native_behaviors ?? [];
}

function hasMachine(entity: SceneEntity, id: string): boolean {
    return entity.component_views?.some(component => component.id === id &&
            component.data.kind === 'native_behavior' && component.data.behavior.kind === 'state_machine') ??
        (entity.native_behaviors?.some(component => component.id === id && component.behavior.kind === 'state_machine') === true);
}

function behaviorReferences(
    behavior: NativeBehaviorSpec,
    add: (field: string, targetKey: EntityKey, targetComponentId?: string) => void,
): void {
    switch (behavior.kind) {
        case 'key_door':
            add('Actor', behavior.actor);
            add('Key trigger', behavior.key_trigger);
            add('Door', behavior.door);
            if (behavior.key_visual !== null) add('Key visual', behavior.key_visual);
            break;
        case 'collectible':
            add('Actor', behavior.actor);
            add('Trigger', behavior.trigger);
            if (behavior.visual !== null) add('Visual', behavior.visual);
            break;
        case 'timed_spawner':
            for (const [slot, target] of Object.entries(behavior.bindings)) add(`Spawn prefab slot: ${slot}`, target);
            break;
        case 'state_machine':
            for (const input of behavior.definition.inputs) {
                add(`Sensor input ${input.id}: Sensor`, input.sensor);
                add(`Sensor input ${input.id}: Actor`, input.actor);
            }
            for (const state of behavior.definition.states) {
                state.on_enter.forEach((action, index) => {
                    const prefix = `State ${state.id}, action ${index + 1}`;
                    if (action.kind === 'emit') add(`${prefix}: Signal target`, action.target.entity, action.target.component);
                    if (action.kind === 'set_visible' || action.kind === 'set_collision_enabled') {
                        add(`${prefix}: Target entity`, action.entity);
                    }
                });
                state.transitions.forEach(({when}, index) => {
                    if (when.kind === 'signal' && when.source) {
                        add(`State ${state.id}, transition ${index + 1}: Signal sender`, when.source.entity, when.source.component);
                    }
                });
            }
            break;
    }
}
