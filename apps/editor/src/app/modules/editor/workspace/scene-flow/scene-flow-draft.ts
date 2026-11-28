import type {ProjectData, SceneId} from '../../../../core/editor/editor.models';
import type {
    SceneRoute, SceneRouteActivation, SceneRoutePreload, SequenceAction, SequenceAsset, SequenceInstance, WorldRegion,
} from '../../../../core/editor/scene-flow.models';

export interface RouteDraft {
    id: string;
    from: SceneId;
    to: SceneId;
    activation: SceneRouteActivation['kind'];
    sensor: string;
    actor: string;
    action: string;
    completion: string;
    condition: string;
    preload: 'none' | SceneRoutePreload['kind'];
    distance: number;
    release: number;
    preloadSequence: string;
    arrival: string;
    arrivalActor: string;
    startSequence: string;
    priority: number;
}

export interface CueDraft {
    at: number;
    kind: SequenceAction['kind'];
    role: string;
    clip: number;
    subtitle: string;
    subtitleDuration: number;
}

export interface TimelineDraft {
    id: string;
    duration: number;
    cues: CueDraft[];
}

export interface InstanceDraft {
    id: string;
    scene: SceneId;
    asset: string;
    bindings: Record<string, string>;
    sensor: string;
    actor: string;
    once: boolean;
}

export interface RegionDraft {
    world: SceneId;
    scene: SceneId;
    minX: number;
    minY: number;
    minZ: number;
    maxX: number;
    maxY: number;
    maxZ: number;
    distance: number;
    release: number;
}

export interface FlowDraft {
    routes: RouteDraft[];
    timelines: TimelineDraft[];
    instances: InstanceDraft[];
    regions: RegionDraft[];
}

export type FlowFields = Pick<ProjectData, 'scene_routes' | 'sequences' | 'world_regions'>;

export function newRoute(from: SceneId, to: SceneId): RouteDraft {
    return {
        id: '', from, to, activation: 'enter_sensor', sensor: '', actor: '', action: 'interact',
        completion: '', condition: '', preload: 'none', distance: 12, release: 16, preloadSequence: '',
        arrival: '', arrivalActor: '', startSequence: '', priority: 0,
    };
}

export function newCue(at = 0): CueDraft {
    return {at, kind: 'camera_cut', role: '', clip: 0, subtitle: '', subtitleDuration: 1_000};
}

export function newInstance(scene: SceneId, asset: string): InstanceDraft {
    return {id: '', scene, asset, bindings: {}, sensor: '', actor: '', once: true};
}

export function newRegion(world: SceneId, scene: SceneId): RegionDraft {
    return {world, scene, minX: 0, minY: 0, minZ: 0, maxX: 20, maxY: 20, maxZ: 20, distance: 12, release: 16};
}

export function readFlow(fields: FlowFields): FlowDraft {
    return {
        routes: (fields.scene_routes ?? []).map(route => ({
            ...newRoute(route.from, route.to),
            id: route.id,
            activation: route.activate.kind,
            sensor: 'sensor' in route.activate ? route.activate.sensor : '',
            actor: 'actor' in route.activate ? route.activate.actor ?? '' : '',
            action: 'action' in route.activate ? route.activate.action : 'interact',
            completion: 'instance' in route.activate ? route.activate.instance : '',
            condition: route.condition?.key ?? '',
            preload: route.preload?.kind ?? 'none',
            distance: route.preload?.kind === 'approach' ? route.preload.distance : 12,
            release: route.preload?.kind === 'approach' ? route.preload.release_distance : 16,
            preloadSequence: route.preload?.kind === 'when_sequence_starts' ? route.preload.instance : '',
            arrival: route.arrival,
            arrivalActor: route.arrival_actor ?? '',
            startSequence: route.start_sequence ?? '',
            priority: route.priority ?? 0,
        })),
        timelines: (fields.sequences?.assets ?? []).map(asset => ({
            id: asset.id, duration: asset.duration_ms,
            cues: asset.cues.map(cue => ({
                ...newCue(cue.at_ms),
                kind: cue.action.kind,
                role: 'role' in cue.action ? cue.action.role : '',
                clip: 'clip_index' in cue.action ? cue.action.clip_index : 0,
                subtitle: 'text' in cue.action ? cue.action.text : '',
                subtitleDuration: 'duration_ms' in cue.action ? cue.action.duration_ms : 1_000,
            })),
        })),
        instances: (fields.sequences?.instances ?? []).map(instance => ({
            ...newInstance(instance.scene_id, instance.sequence_id),
            id: instance.id,
            bindings: {...instance.bindings},
            sensor: instance.trigger?.sensor ?? '',
            actor: instance.trigger?.actor ?? '',
            once: instance.once_per_entry ?? true,
        })),
        regions: (fields.world_regions ?? []).map(region => ({
            world: region.world,
            scene: region.scene,
            minX: region.bounds.min[0], minY: region.bounds.min[1], minZ: region.bounds.min[2],
            maxX: region.bounds.max[0], maxY: region.bounds.max[1], maxZ: region.bounds.max[2],
            distance: region.preload_distance,
            release: region.release_distance,
        })),
    };
}

export function flowFields(draft: FlowDraft): FlowFields {
    return {
        scene_routes: draft.routes.map(routeData),
        sequences: {
            assets: draft.timelines.map(timelineData),
            instances: draft.instances.map(instance => instanceData(instance, draft.timelines)),
        },
        world_regions: draft.regions.map(regionData),
    };
}

export function cueRoles(timeline: TimelineDraft): readonly string[] {
    return Array.from(new Set(timeline.cues
        .filter(cue => cue.kind !== 'subtitle' && !!cue.role.trim())
        .map(cue => cue.role.trim())));
}

export function sceneFlowError(draft: FlowDraft): string | null {
    const routes = new Set<string>();
    for (const route of draft.routes) {
        const name = route.id.trim();
        if (!/^[a-zA-Z0-9_-]{1,64}$/.test(name) || routes.has(name)) {
            return 'Enter unique route IDs using letters, digits, hyphens, or underscores.';
        }
        routes.add(name);
        if (!route.from || !route.to || route.from === route.to || !route.arrival.trim()) {
            return 'Each route needs distinct source and destination scenes and an arrival marker.';
        }
        const fromSensor = route.activation === 'enter_sensor' || route.activation === 'interact_in_sensor';
        if (fromSensor && !route.sensor.trim() || route.activation === 'interact_in_sensor' && !route.action.trim()) {
            return 'Select a source sensor and an action for interaction routes.';
        }
        if (route.activation === 'sequence_completed' && !route.completion) {
            return 'Select the sequence instance that activates this route.';
        }
        if (route.preload === 'approach' && (!fromSensor || !validDistances(route.distance, route.release))) {
            return 'Approach preloading requires a sensor and a release distance larger than the positive preload distance.';
        }
        if (route.preload === 'when_sequence_starts' && !route.preloadSequence) {
            return 'Select the sequence instance that starts preloading.';
        }
        if (!Number.isInteger(route.priority) || route.priority < -32768 || route.priority > 32767) {
            return 'Route priority must be an integer between -32768 and 32767.';
        }
    }

    const assets = new Map<string, TimelineDraft>();
    for (const timeline of draft.timelines) {
        if (!validKey(timeline.id) || assets.has(timeline.id.trim())) {
            return 'Enter unique Timeline IDs without spaces.';
        }
        assets.set(timeline.id.trim(), timeline);
        if (!Number.isInteger(timeline.duration) || timeline.duration < 1 || timeline.duration > 600_000 ||
            timeline.cues.length === 0 || timeline.cues.length > 256) {
            return 'Each Timeline needs 1–256 cues and a duration of 1–600,000 milliseconds.';
        }
        for (const cue of timeline.cues) {
            if (!Number.isInteger(cue.at) || cue.at < 0 || cue.at >= timeline.duration) {
                return 'Every cue time must be within its Timeline duration.';
            }
            if (cue.kind !== 'subtitle' && !validKey(cue.role)) return 'Enter a role ID for every camera, actor, and audio cue.';
            if (cue.kind === 'play_clip' && (!Number.isInteger(cue.clip) || cue.clip < 0 || cue.clip > 4_294_967_295)) {
                return 'Enter a valid imported clip index.';
            }
            if (cue.kind === 'subtitle' && (!cue.subtitle.trim() ||
                new TextEncoder().encode(cue.subtitle.trim()).length > 256 ||
                !Number.isInteger(cue.subtitleDuration) || cue.subtitleDuration < 1 ||
                cue.at + cue.subtitleDuration > timeline.duration)) {
                return 'Enter a subtitle of up to 256 bytes that ends within the Timeline duration.';
            }
        }
    }

    const instances = new Set<string>();
    for (const instance of draft.instances) {
        const identity = JSON.stringify([instance.scene, instance.id.trim()]);
        const asset = assets.get(instance.asset);
        if (!validKey(instance.id) || !instance.scene || !asset || instances.has(identity)) {
            return 'Every sequence instance needs a unique ID in its scene and an existing Timeline.';
        }
        instances.add(identity);
        if (cueRoles(asset).some(role => !validKey(Object.hasOwn(instance.bindings, role) ? instance.bindings[role] : ''))) {
            return 'Bind every Timeline role to a scene object.';
        }
        if (!!instance.sensor.trim() !== !!instance.actor.trim()) {
            return 'Specify both the sensor and the actor to start a Timeline on entry.';
        }
    }

    const regions = new Set<SceneId>();
    for (const region of draft.regions) {
        if (!region.world || !region.scene || region.world === region.scene || regions.has(region.scene)) {
            return 'Each region needs a unique scene and a different persistent world.';
        }
        regions.add(region.scene);
        if (![region.minX, region.minY, region.minZ, region.maxX, region.maxY, region.maxZ].every(Number.isFinite) ||
            region.minX >= region.maxX || region.minY >= region.maxY || region.minZ >= region.maxZ) {
            return 'Region bounds must have a finite minimum smaller than the maximum on all axes.';
        }
        if (!validDistances(region.distance, region.release)) return 'Region release distance must be larger than its positive preload distance.';
    }
    return null;
}

function routeData(route: RouteDraft): SceneRoute {
    const sensor = route.sensor.trim();
    const actor = optionalValue(route.actor);
    let activate: SceneRouteActivation;
    switch (route.activation) {
        case 'manual':
            activate = {kind: 'manual'};
            break;
        case 'enter_sensor':
            activate = {kind: 'enter_sensor', sensor, actor};
            break;
        case 'interact_in_sensor':
            activate = {kind: 'interact_in_sensor', sensor, actor, action: route.action.trim()};
            break;
        case 'sequence_completed':
            activate = {kind: 'sequence_completed', instance: route.completion};
            break;
    }
    let preload: SceneRoutePreload | undefined;
    if (route.preload === 'approach') {
        preload = {kind: 'approach', distance: route.distance, release_distance: route.release};
    } else if (route.preload === 'when_sequence_starts') {
        preload = {kind: 'when_sequence_starts', instance: route.preloadSequence};
    }
    const condition = optionalValue(route.condition);
    return {
        id: route.id.trim(), from: route.from, to: route.to, activate, preload,
        condition: condition ? {kind: 'gameplay_flag', key: condition} : undefined,
        arrival: route.arrival.trim(), arrival_actor: optionalValue(route.arrivalActor),
        start_sequence: optionalValue(route.startSequence), priority: route.priority || undefined,
    };
}

function timelineData(timeline: TimelineDraft): SequenceAsset {
    return {
        id: timeline.id.trim(), schema_version: 1, duration_ms: timeline.duration,
        cues: [...timeline.cues].sort((left, right) => left.at - right.at).map(cue => ({
            at_ms: cue.at,
            action: cue.kind === 'subtitle'
                ? {kind: 'subtitle', text: cue.subtitle.trim(), duration_ms: cue.subtitleDuration}
                : cue.kind === 'play_clip'
                    ? {kind: 'play_clip', role: cue.role.trim(), clip_index: cue.clip}
                    : {kind: cue.kind, role: cue.role.trim()},
        })),
    };
}

function instanceData(instance: InstanceDraft, timelines: readonly TimelineDraft[]): SequenceInstance {
    const asset = timelines.find(timeline => timeline.id.trim() === instance.asset);
    const roles = asset ? cueRoles(asset) : [];
    return {
        id: instance.id.trim(), scene_id: instance.scene, sequence_id: instance.asset,
        bindings: Object.fromEntries(roles.map(role => [role, (instance.bindings[role] ?? '').trim()])),
        trigger: instance.sensor.trim() && instance.actor.trim() ? {
            sensor: instance.sensor.trim(),
            actor: instance.actor.trim()
        } : undefined,
        once_per_entry: instance.once,
    };
}

function regionData(region: RegionDraft): WorldRegion {
    return {
        world: region.world, scene: region.scene,
        bounds: {min: [region.minX, region.minY, region.minZ], max: [region.maxX, region.maxY, region.maxZ]},
        preload_distance: region.distance, release_distance: region.release,
    };
}

function validKey(value: string): boolean {
    return !!value.trim() && new TextEncoder().encode(value.trim()).length <= 128 && !/[\s\p{Cc}]/u.test(value.trim());
}

function optionalValue(value: string): string | undefined {
    return value.trim() || undefined;
}

function validDistances(preload: number, release: number): boolean {
    return Number.isFinite(preload) && Number.isFinite(release) && preload > 0 && release > preload;
}
