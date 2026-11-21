import {parseF32} from '../../../../core/editor/authoring-number';
import {
    validInteractionReach,
    type ComponentRecord,
    type InteractableSettings,
} from '../../../../core/editor/editor-components';
import type {EditorSnapshot, SceneEntity} from '../../../../core/editor/editor.models';
import type {GameInputProfile} from '../../../../core/game-input/game-input.models';
import {normalizeColliderInteraction} from '../collider/collider';
import type {CourtyardRouteGate} from './courtyard-settings';

export interface RouteGateDraft {
    enabled: boolean;
    lever: string;
    gate: string;
    action: string;
    radius: string;
}

type LeverComponent = ComponentRecord & {
    readonly data: { readonly kind: 'interactable'; readonly settings: InteractableSettings };
};

export function leverComponent(lever: SceneEntity | undefined): LeverComponent | undefined {
    return lever?.component_views?.find(
        (component): component is LeverComponent => component.data.kind === 'interactable',
    );
}

export function routeActions(profile: GameInputProfile | null | undefined): readonly string[] {
    if (!profile) return [];
    const actions = profile.actions ?? [];
    const movement = Object.values(profile.movement);
    return actions.filter(action => !movement.includes(action.key) &&
        !actions.some(other => other.id !== action.id && other.key === action.key)).map(action => action.id);
}

export function routeGateCandidate(
    draft: RouteGateDraft,
    snapshot: EditorSnapshot,
    current: CourtyardRouteGate | null | undefined,
    reserved: readonly (string | undefined)[],
): CourtyardRouteGate {
    const lever = snapshot.document.entities.find(entity => entity.key === draft.lever);
    const gate = snapshot.document.entities.find(entity => entity.key === draft.gate);
    if (!lever || !gate || lever === gate) throw new Error('Choose separate existing lever and gate entities.');
    if (reserved.includes(lever.key) || reserved.includes(gate.key))
        throw new Error('The lever and gate must be separate from the course roles, sensors, and exit blocker.');

    const {bindings} = snapshot.document;
    const player = snapshot.document.entities.find(entity => entity.key === bindings['player']);
    if (!player?.collider || player.collision_enabled === false || !gate.collider)
        throw new Error('Assign a Player collider and choose a gate with a blocking collider.');
    const actor = normalizeColliderInteraction(player.collider.interaction);
    const obstacle = normalizeColliderInteraction(gate.collider.interaction);
    if (obstacle.sensor || (actor.mask & obstacle.layers) === 0 || (obstacle.mask & actor.layers) === 0)
        throw new Error('Choose a blocking gate collider whose filters accept Player.');
    if ((gate.collision_enabled !== false) !== gate.renderable)
        throw new Error('The gate visibility and collider enabled state must agree.');

    const component = leverComponent(lever);
    const action = component?.data.settings.action ?? draft.action;
    if (!routeActions(snapshot.project.input_profile).includes(action))
        throw new Error('Bind the selected action to a dedicated key in Game Input first.');
    const radius = component
        ? current?.lever === lever.key ? current.radius : component.data.settings.reach
        : parseF32(draft.radius);
    if (!validInteractionReach(radius)) throw new Error('Fallback reach must be positive and at most 3 metres.');
    return {scene_id: snapshot.active_scene_id, lever: lever.key, gate: gate.key, action, radius};
}
