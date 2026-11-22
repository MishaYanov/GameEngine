import {
    animatorChannelKey, type AnimatorBinding, type AnimatorGraph, sameAnimatorSource,
} from '../../../../core/editor/animator.models';
import type {ClipAssetDocument, VisualClipTarget} from '../../../../core/editor/authored-clip.models';
import type {SceneDocument, SceneEntity, SceneId} from '../../../../core/editor/editor.models';

export interface AnimatorChannel {
    readonly id: string;
    readonly channel: VisualClipTarget;
    readonly label: string;
    readonly automatic: boolean;
}
export type ActorGraph =
    | {readonly kind: 'imported'; readonly source: Extract<AnimatorBinding, {kind: 'imported'}>['source']}
    | {readonly kind: 'visual'; readonly channels: readonly AnimatorChannel[]};

export function animatorDescendants(actor: SceneEntity, document: SceneDocument | null): readonly SceneEntity[] {
    if (!document) return [];
    const {entities, bindings} = document;
    const children = new Map<string, SceneEntity[]>();
    const byKey = new Map(entities.map(entity => [entity.key, entity]));
    const blocked = new Set(Object.values(bindings));
    for (const entity of entities) {
        if (entity.collider || entity.character_controller || entity.player_controller || entity.camera || entity.animator ||
            entity.native_behaviors?.length || entity.component_views?.some(component =>
                component.data.kind !== 'mesh_renderer' && component.data.kind !== 'animation')) blocked.add(entity.key);
        if (!entity.parent) continue;
        const siblings = children.get(entity.parent);
        if (siblings) siblings.push(entity);
        else children.set(entity.parent, [entity]);
    }
    for (const key of blocked) {
        const parent = byKey.get(key)?.parent;
        if (parent) blocked.add(parent);
    }
    const pending = [...(children.get(actor.key) ?? [])];
    const seen = new Set([actor.key]);
    const targets: SceneEntity[] = [];
    for (let index = 0; index < pending.length; index++) {
        const entity = pending[index];
        if (seen.has(entity.key)) continue;
        seen.add(entity.key);
        if (entity.renderable && entity.model_path !== '__no_mesh__' && !blocked.has(entity.key)) targets.push(entity);
        pending.push(...(children.get(entity.key) ?? []));
    }
    return targets;
}

export async function reviewActorGraph(
    graph: AnimatorGraph, sceneId: SceneId, readClip: (id: string) => Promise<ClipAssetDocument>,
): Promise<ActorGraph> {
    const first = graph.states[0].clip;
    if (first.kind === 'imported') {
        const mismatch = graph.states.find(state => state.clip.kind !== 'imported' ||
            !sameAnimatorSource(state.clip.source, first.source));
        if (mismatch) throw new Error(`State "${mismatch.id}" uses a different rig or visual clip.`);
        return {kind: 'imported', source: first.source};
    }
    const channels = new Map<string, AnimatorChannel>();
    const reviewed = new Map<string, ClipAssetDocument>();
    for (const state of graph.states) {
        const clip = state.clip;
        if (clip.kind !== 'authored') throw new Error(`State "${state.id}" uses an imported rig in a visual graph.`);
        let document = reviewed.get(clip.asset_id);
        if (!document) {
            try {
                document = await readClip(clip.asset_id);
            } catch (error) {
                throw new Error(`State "${state.id}" clip could not be opened. ${error instanceof Error ? error.message : String(error)}`);
            }
            reviewed.set(clip.asset_id, document);
        }
        if (document.asset.asset_id !== clip.asset_id || document.asset.fingerprint !== clip.expected_sha256) {
            throw new Error(`State "${state.id}" clip changed. Review the clip in the Animation workspace.`);
        }
        for (const {target: channel} of document.clip.tracks) {
            const id = animatorChannelKey(channel);
            const automatic = document.clip.owner.kind === 'scene' && document.clip.owner.scene_id === sceneId && channel.kind === 'scene';
            const previous = channels.get(id);
            channels.set(id, {id, channel, label: channelLabel(channel), automatic: automatic && (previous?.automatic ?? true)});
        }
    }
    return {kind: 'visual', channels: [...channels.values()]};
}

function channelLabel(channel: VisualClipTarget): string {
    if (channel.kind === 'scene') return channel.entity_key;
    const {address} = channel;
    return [...address.placement_path, ...(address.kind === 'object' ? [address.source_entity] : [])]
        .join(' / ') || 'Prop placement';
}
