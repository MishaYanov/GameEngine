import type {GameInputProfile} from '../game-input/game-input.models';
import type {PhysicsLayers} from './physics-layers.models';
import type {ComponentRecord, ComponentTypeDescriptor, NativeBehaviorAttachment} from './editor-components';
import type {
    LinkedPrefabOverrideTarget,
    LinkedPrefabReferenceView,
    PrefabSourceDocument
} from './linked-prefabs.models';
import type {CharacterControllerAttachment} from './character-controller.models';
import type {AnimationAttachment} from './animation.models';
import type {MachineDescriptor} from './state-machine.models';
import type {ParticleEmitterComponent} from './particle-emitter.models';

export type {ParticleEmitterComponent, ParticleEmitterSettings} from './particle-emitter.models';

export type SceneId = string;

export interface SceneDescriptor {
    readonly id: SceneId;
    readonly asset_path: string;
}

export type BehaviorSpec =
    | {
    readonly kind: 'controlled_move';
    readonly entity: string;
    readonly speed: number;
    readonly space: 'local' | 'world';
}
    | { readonly kind: 'spin'; readonly entity: string; readonly angular_velocity: Vec3 }
    | { readonly kind: 'accept_action'; readonly entity: string; readonly action: string }
    | { readonly kind: 'shared'; readonly entity: string; readonly definition: string };

export type BehaviorSettings =
    | { readonly kind: 'controlled_move'; readonly speed: number; readonly space: 'local' | 'world' }
    | { readonly kind: 'spin'; readonly angular_velocity: Vec3 }
    | { readonly kind: 'accept_action'; readonly action: string };

export type SceneBindingTarget =
    | { readonly kind: 'assign'; readonly key: EntityKey }
    | { readonly kind: 'clear' };

export interface ProjectData {
    readonly schema_version: 3;
    readonly name: string;
    readonly scenes: readonly SceneDescriptor[];
    readonly startup_scene_id: SceneId;
    readonly fixed_hz: number;
    readonly max_catchup_steps: number;
    readonly behaviors: readonly { readonly scene_id: SceneId; readonly behavior: BehaviorSpec }[];
    readonly behavior_definitions: readonly { readonly id: string; readonly settings: BehaviorSettings }[];
    readonly input_profile?: GameInputProfile | null;
    readonly physics_layers?: PhysicsLayers | null;
    readonly gameplay_module?: string | null;
    readonly gameplay_settings?: unknown;
}

export interface SceneState extends SceneDescriptor {
    readonly loaded: boolean;
    readonly dirty: boolean;
}

/** Wire contracts mirror bridge::editor. Rust validates every incoming value. */
export type EntityKey = string;
export type Vec2 = readonly [number, number];
export type Vec3 = readonly [number, number, number];
export type Vec4 = readonly [number, number, number, number];
export type TransformSpace = 'local' | 'world';

export interface Transform {
    readonly position: Vec3;
    readonly rotation: Vec3;
    readonly scale: Vec3;
}

export interface Camera {
    readonly position: Vec3;
    readonly yaw: number;
    readonly pitch: number;
    readonly vertical_fov: number;
    readonly near: number;
    readonly far: number;
    readonly follow?: FollowCamera | null;
    readonly orbit?: OrbitCamera | null;
}

export interface OrbitCamera {
    readonly target: EntityKey;
    readonly pivot_offset: Vec3;
    readonly initial_yaw: number;
    readonly initial_pitch: number;
    readonly initial_distance: number;
    readonly min_distance: number;
    readonly max_distance: number;
    readonly min_pitch: number;
    readonly max_pitch: number;
    readonly follow_seconds: number;
    readonly recovery_seconds: number;
    readonly teleport_distance: number;
    readonly probe_radius: number;
    readonly obstruction_mask: number;
}

export interface FollowCamera {
    readonly target: EntityKey;
    readonly offset: Vec3;
    readonly look_offset: Vec3;
    readonly smoothing_seconds: number;
}

export interface CameraComponentSettings {
    readonly lens: Pick<Camera, 'vertical_fov' | 'near' | 'far'>;
    readonly rig:
        | {readonly mode: 'fixed'}
        | {readonly mode: 'follow'; readonly settings: FollowCamera}
        | {readonly mode: 'orbit'; readonly settings: OrbitCamera};
}

export interface MaterialOverride {
    readonly base_color_factor: Vec4;
}

export interface ColliderInteraction {
    readonly sensor: boolean;
    readonly layers: number;
    readonly mask: number;
}

export type Collider =
    | { readonly kind: 'box'; readonly half_extents: Vec3; readonly offset: Vec3; readonly interaction?: ColliderInteraction }
    | { readonly kind: 'sphere'; readonly radius: number; readonly offset: Vec3; readonly interaction?: ColliderInteraction };

export interface SceneEntity {
    readonly key: EntityKey;
    readonly name: string;
    readonly model_path: string;
    readonly asset_id?: string | null;
    readonly renderable: boolean;
    readonly collision_enabled?: boolean;
    readonly transform: Transform;
    readonly parent?: EntityKey | null;
    readonly material?: MaterialOverride | null;
    readonly collider?: Collider | null;
    readonly mesh_index?: number | null;
    readonly component_views?: readonly ComponentRecord[];
    readonly component_ids?: Readonly<Record<string, string>>;
    readonly native_behaviors?: readonly NativeBehaviorAttachment[];
    readonly character_controller?: CharacterControllerAttachment | null;
    readonly player_controller?: import('./player-controller.models').PlayerControllerAttachment | null;
    readonly animation?: AnimationAttachment | null;
    readonly animator?: import('./animator.models').AnimatorAttachment | null;
    readonly camera?: {readonly id: string; readonly enabled: boolean; readonly settings: CameraComponentSettings} | null;
    readonly sky?: import('./sky.models').SkyAttachment | null;
    readonly particle_emitter?: ParticleEmitterComponent | null;
    readonly linked_prefab?: LinkedPrefabReferenceView | null;
}

export interface Light {
    readonly key: EntityKey;
    readonly name: string;
    readonly kind: 'directional' | 'point';
    readonly position: Vec3;
    readonly direction: Vec3;
    readonly color: Vec3;
    readonly intensity: number;
    readonly range: number;
}

export type SceneSchemaVersion = 3 | 4 | 5 | 6 | 7 | 8;

export function isSceneSchemaVersion(value: unknown): value is SceneSchemaVersion {
    return value === 3 || value === 4 || value === 5 || value === 6 || value === 7 || value === 8;
}

export type FolderId = string;

export interface FolderRecord {
    readonly id: FolderId;
    readonly name: string;
    readonly parent?: FolderId | null;
}

export interface SceneOrganization {
    readonly folders: readonly FolderRecord[];
    readonly root_membership: Readonly<Record<EntityKey, FolderId>>;
}

export interface SceneDocument {
    readonly schema_version: SceneSchemaVersion;
    readonly bindings: Readonly<Record<string, EntityKey>>;
    readonly entities: readonly SceneEntity[];
    readonly camera: Camera;
    readonly default_game_camera?: EntityKey | null;
    readonly lights: readonly Light[];
    readonly organization?: SceneOrganization;
    readonly environment?: import('./sky.models').SceneEnvironment;
}

export interface CopiedPrefab {
    readonly schema_version: 1 | 2 | 3;
    readonly entities: readonly SceneEntity[];
    readonly gameplay?: unknown;
    readonly reference_slots?: Readonly<Record<string, string>>;
}

export type Prefab = CopiedPrefab | PrefabSourceDocument;

export interface EntitySpec {
    readonly name: string;
    readonly model_path: string;
    readonly asset_id?: string | null;
    readonly transform?: Transform;
    readonly parent?: EntityKey | null;
    readonly mesh_index?: number | null;
}

export interface TransformEdit {
    readonly key: EntityKey;
    readonly transform: Transform;
}

export interface SnapSettings {
    readonly enabled: boolean;
    readonly translation: number;
    readonly rotation_radians: number;
    readonly scale: number;
}

/** Intent has no dependency on key bindings, panels, tools, or mouse buttons. */
export type EditorCommand =
    | {
    readonly type: 'add_scene';
    readonly id: SceneId;
    readonly asset_path: string;
    readonly document: SceneDocument;
}
    | { readonly type: 'switch_scene'; readonly scene_id: SceneId }
    | { readonly type: 'remove_scene'; readonly scene_id: SceneId }
    | { readonly type: 'set_startup_scene'; readonly scene_id: SceneId }
    | { readonly type: 'change_scene_path'; readonly scene_id: SceneId; readonly asset_path: string }
    | { readonly type: 'set_project'; readonly project: ProjectData }
    | { readonly type: 'set_entity_input'; readonly entity: EntityKey; readonly movement: Vec3 }
    | {
    readonly type: 'pick_selection';
    readonly pixel: Vec2;
    readonly extent: Vec2;
    readonly additive: boolean;
}
    | { readonly type: 'select'; readonly keys: readonly EntityKey[] }
    | { readonly type: 'create_folder'; readonly name: string; readonly parent: FolderId | null }
    | { readonly type: 'rename_folder'; readonly id: FolderId; readonly name: string }
    | { readonly type: 'move_folder'; readonly id: FolderId; readonly parent: FolderId | null }
    | { readonly type: 'move_roots_to_folder'; readonly roots: readonly EntityKey[]; readonly folder: FolderId | null }
    | { readonly type: 'remove_folder'; readonly id: FolderId }
    | { readonly type: 'rename'; readonly key: EntityKey; readonly name: string }
    | { readonly type: 'spawn'; readonly entity: EntitySpec }
    | {
    readonly type: 'set_transforms';
    readonly edits: readonly TransformEdit[];
    readonly space: TransformSpace;
    readonly snap: boolean;
}
    | { readonly type: 'translate_selection'; readonly delta: Vec3; readonly snap: boolean }
    | {
    readonly type: 'rotate_selection';
    readonly radians: Vec3;
    readonly space: TransformSpace;
    readonly snap: boolean;
}
    | { readonly type: 'scale_selection'; readonly factor: Vec3; readonly snap: boolean }
    | { readonly type: 'duplicate_selection'; readonly offset: Vec3 }
    | { readonly type: 'delete_selection' }
    | {
    readonly type: 'reparent';
    readonly key: EntityKey;
    readonly parent: EntityKey | null;
    readonly keep_world: boolean;
}
    | {
    readonly type: 'reparent_objects';
    readonly keys: readonly EntityKey[];
    readonly parent: EntityKey | null;
    readonly keep_world: boolean;
}
    | {
    readonly type: 'set_material';
    readonly key: EntityKey;
    readonly material: MaterialOverride | null;
}
    | { readonly type: 'set_collider'; readonly key: EntityKey; readonly collider: Collider | null }
    | {
    readonly type: 'set_colliders';
    readonly updates: readonly { readonly key: EntityKey; readonly collider: Collider }[]
}
    | { readonly type: 'add_component'; readonly key: EntityKey; readonly component: ComponentRecord }
    | { readonly type: 'replace_component'; readonly key: EntityKey; readonly component: ComponentRecord }
    | { readonly type: 'remove_component'; readonly key: EntityKey; readonly component_id: string }
    | { readonly type: 'set_component_enabled'; readonly key: EntityKey; readonly component_id: string; readonly enabled: boolean }
    | { readonly type: 'revert_linked_prefab'; readonly root_key: EntityKey; readonly property?: LinkedPrefabOverrideTarget | null }
    | { readonly type: 'unpack_linked_prefab'; readonly root_key: EntityKey }
    | { readonly type: 'unpack_linked_prefab_outer'; readonly root_key: EntityKey }
    | { readonly type: 'duplicate_linked_prefab'; readonly root_key: EntityKey }
    | { readonly type: 'create_camera' }
    | { readonly type: 'create_sky' }
    | { readonly type: 'create_empty' }
    | { readonly type: 'create_light'; readonly light_kind: Light['kind'] }
    | { readonly type: 'create_particle_emitter' }
    | { readonly type: 'set_active_sky'; readonly key: EntityKey | null }
    | { readonly type: 'set_scene_environment'; readonly ambient_linear: Vec3; readonly sky_exposure_ev: number }
    | { readonly type: 'create_player' }
    | { readonly type: 'create_audio_source' }
    | { readonly type: 'set_local_player'; readonly key: EntityKey | null }
    | { readonly type: 'set_default_game_camera'; readonly key: EntityKey }
    | { readonly type: 'align_camera_to_scene_view'; readonly key: EntityKey; readonly position: Vec3; readonly yaw: number; readonly pitch: number }
    | { readonly type: 'upsert_light'; readonly light: Light }
    | { readonly type: 'remove_light'; readonly key: EntityKey }
    | { readonly type: 'set_camera'; readonly camera: Camera }
    | { readonly type: 'frame_selection' }
    | { readonly type: 'orbit_camera'; readonly yaw: number; readonly pitch: number }
    | { readonly type: 'pan_camera'; readonly right: number; readonly up: number }
    | { readonly type: 'dolly_camera'; readonly distance: number }
    | { readonly type: 'set_snap'; readonly settings: SnapSettings }
    | { readonly type: 'begin_transaction'; readonly label: string }
    | { readonly type: 'commit_transaction' }
    | { readonly type: 'cancel_transaction' }
    | { readonly type: 'undo' }
    | { readonly type: 'redo' }
    | { readonly type: 'start_play' }
    | { readonly type: 'stop_play' }
    | { readonly type: 'set_paused'; readonly paused: boolean }
    | { readonly type: 'set_play_input'; readonly movement: Vec3 }
    | PlayCameraInputCommand
    | {
    readonly type: 'set_scene_binding';
    readonly scene_id: SceneId;
    readonly name: string;
    readonly target: SceneBindingTarget;
};

export interface EditorCommandRequest {
    readonly expected_revision: number;
    readonly command: EditorCommand;
}

/** Owner values come from native viewport state and its matching Play status. */
export interface PlayCameraInputCommand {
    readonly type: 'set_play_camera_input';
    readonly workspace_generation: number;
    readonly run_id: string;
    readonly scene_generation: number;
    readonly yaw_delta: number;
    readonly pitch_delta: number;
    readonly log_distance_delta: number;
}

export interface EditorSnapshot {
    readonly play_session_id?: string | null;
    readonly active_scene_id: SceneId;
    readonly project: ProjectData;
    readonly project_dirty: boolean;
    readonly scenes: readonly SceneState[];
    readonly revision: number;
    readonly scene_revision: number;
    readonly document: SceneDocument;
    readonly selection: readonly EntityKey[];
    readonly mode: 'edit' | 'play';
    readonly paused: boolean;
    readonly dirty: boolean;
    readonly can_undo: boolean;
    readonly can_redo: boolean;
    readonly transaction: string | null;
    readonly snap: SnapSettings;
}

/** Authoritative UI state without copying the authored document or project. */
export type EditorState = Omit<EditorSnapshot, 'document' | 'project'>;

export interface WorldChange {
    readonly changed_entities: readonly EntityKey[];
    readonly removed_entities: readonly EntityKey[];
    readonly camera_changed: boolean;
    readonly lights_changed: boolean;
    readonly topology_changed: boolean;
    readonly full_resync: boolean;
}

export interface CommandReceipt {
    readonly revision: number;
    readonly scene_revision: number;
    readonly change_cursor: number;
    readonly changes: WorldChange;
    readonly state_changed: boolean;
    readonly project_changed: boolean;
    readonly organization_changed?: boolean;
    readonly active_scene_id: SceneId;
    readonly affected_scenes: readonly SceneId[];
}

export type EditorCommandUpdate =
    | {
    readonly kind: 'snapshot';
    readonly receipt: CommandReceipt;
    readonly snapshot: EditorSnapshot;
}
    | {
    readonly kind: 'delta';
    readonly receipt: CommandReceipt;
    readonly state: EditorState;
    readonly entities: readonly SceneEntity[];
    readonly camera: Camera | null;
    readonly lights: readonly Light[] | null;
    readonly organization?: SceneOrganization;
};

export interface CommandDescriptor {
    readonly name: EditorCommand['type'];
    readonly label: string;
    readonly category: string;
    readonly undoable: boolean;
    readonly available_in_play: boolean;
}

export interface EditorCapabilities {
    readonly input_action_assets?: boolean;
    readonly sky_panorama_assets?: boolean;
    readonly primitive_assets?: boolean;
    readonly animator_assets?: boolean;
    readonly controls_graph_assets?: boolean;
    readonly state_machines?: MachineDescriptor;
    readonly protocol_version: number;
    readonly scene_schema_version: number;
    readonly commands: readonly CommandDescriptor[];
    readonly max_selection: number;
    readonly max_history_entries: number;
    readonly history_budget_bytes: number;
    readonly embedded_viewport_supported: boolean;
    readonly scene_warming_supported?: boolean;
    readonly scene_verification_supported?: boolean;
    readonly incremental_commands?: boolean;
    readonly gameplay_modules?: readonly GameplayModuleDescriptor[];
    readonly component_types?: readonly ComponentTypeDescriptor[];
    readonly object_scripts?: readonly import('./object-scripts.models').ScriptDescriptor[];
}

export interface GameplayRoleDescriptor {
    readonly name: string;
    readonly label: string;
    readonly requirement_hint: string;
}

export interface GameplayModuleDescriptor {
    readonly id: string;
    readonly label: string;
    readonly description: string;
    readonly default_roles: readonly GameplayRoleDescriptor[];
    readonly scene_roles: Readonly<Record<SceneId, readonly GameplayRoleDescriptor[]>>;
}

export interface ModelAsset {
    readonly name: string;
    readonly path: string;
    readonly kind?: 'model' | 'prefab' | 'audio_clip' | 'sky_panorama';
    readonly asset_id?: string | null;
    readonly fingerprint?: string | null;
    readonly revision?: number | null;
    readonly source_path?: string | null;
    readonly mesh_count?: number | null;
}

export type PrimitiveKind = 'plane' | 'cube' | 'sphere' | 'capsule';

export interface AssetOperation {
    readonly operation_id: string;
    readonly asset: ModelAsset;
    readonly snapshot: EditorSnapshot;
}

export interface ProjectBuildResult {
    readonly path: string;
    readonly asset_count: number;
    readonly asset_bytes: number;
}

export interface ProjectBuildPreparation extends ProjectBuildResult {
    readonly build_id: number;
}

export interface BuildLaunchResult {
    readonly path: string;
    readonly process_id: number;
}

export interface CollisionContact {
    readonly a: EntityKey;
    readonly b: EntityKey;
    readonly normal: Vec3;
    readonly penetration: number;
}

export type CollisionDebugShape =
    | { readonly kind: 'box'; readonly entity: EntityKey; readonly min: Vec3; readonly max: Vec3 }
    | {
    readonly kind: 'sphere';
    readonly entity: EntityKey;
    readonly center: Vec3;
    readonly radius: number;
    readonly min: Vec3;
    readonly max: Vec3;
};

export interface CollisionSnapshot {
    readonly contacts: readonly CollisionContact[];
    readonly shapes: readonly CollisionDebugShape[];
}

/** CSS logical pixels relative to the Tauri webview's client area. */
export interface ViewportBounds {
    readonly x: number;
    readonly y: number;
    readonly width: number;
    readonly height: number;
    readonly visible: boolean;
}

export interface ViewportSceneLoad {
    readonly request_id?: string;
    readonly workspace_generation: number;
    readonly scene_id: SceneId;
    readonly scene_revision: number;
    readonly stage: 'preparing' | 'rendering' | 'presented';
    readonly completed?: number | null;
    readonly total?: number | null;
}

export interface ViewportStatus {
    readonly attached: boolean;
    readonly visible: boolean;
    readonly width: number;
    readonly height: number;
    readonly draw_calls: number;
    readonly instances: number;
    readonly triangles: number;
    readonly gpu_time_ms: number | null;
    readonly gpu_timestamps_supported: boolean;
    readonly error: string | null;
    readonly scene_load?: ViewportSceneLoad | null;
}

export interface ViewportModifiers {
    readonly shift: boolean;
    readonly control: boolean;
    readonly alt: boolean;
    readonly super_key: boolean;
}

export type ViewportInputKind =
    | 'pointer_move'
    | 'pointer_leave'
    | 'pointer_down'
    | 'pointer_up'
    | 'wheel_lines'
    | 'wheel_pixels'
    | 'key_down'
    | 'key_up'
    | 'focused'
    | 'blurred';

/** Raw native input; position and extent are physical pixels, not CSS pixels. */
export interface ViewportInput {
    readonly kind: ViewportInputKind;
    readonly position: Vec2 | null;
    readonly extent: Vec2;
    readonly button: string | null;
    readonly code: string | null;
    readonly repeat: boolean;
    readonly modifiers: ViewportModifiers;
    readonly delta: Vec2 | null;
}

/** Small factory for common intents. New controls can also construct the union. */
export class CommandBuilder {
    static select(keys: readonly EntityKey[]): EditorCommand {
        return {type: 'select', keys: [...keys]};
    }

    static pick(pixel: Vec2, extent: Vec2, additive = false): EditorCommand {
        return {type: 'pick_selection', pixel: [...pixel], extent: [...extent], additive};
    }

    static rename(key: EntityKey, name: string): EditorCommand {
        return {type: 'rename', key, name};
    }

    static spawn(entity: EntitySpec): EditorCommand {
        return {type: 'spawn', entity};
    }

    static transforms(
        edits: readonly TransformEdit[],
        space: TransformSpace = 'local',
        snap = false,
    ): EditorCommand {
        return {type: 'set_transforms', edits, space, snap};
    }

    static translate(delta: Vec3, snap = false): EditorCommand {
        return {type: 'translate_selection', delta: [...delta], snap};
    }

    static rotate(radians: Vec3, space: TransformSpace = 'world', snap = false): EditorCommand {
        return {type: 'rotate_selection', radians: [...radians], space, snap};
    }

    static scale(factor: Vec3, snap = false): EditorCommand {
        return {type: 'scale_selection', factor: [...factor], snap};
    }

    static duplicate(offset: Vec3 = [0, 0, 0]): EditorCommand {
        return {type: 'duplicate_selection', offset: [...offset]};
    }

    static deleteSelection(): EditorCommand {
        return {type: 'delete_selection'};
    }

    static reparent(key: EntityKey, parent: EntityKey | null, keepWorld = true): EditorCommand {
        return {type: 'reparent', key, parent, keep_world: keepWorld};
    }

    static material(key: EntityKey, material: MaterialOverride | null): EditorCommand {
        return {type: 'set_material', key, material};
    }

    static collider(key: EntityKey, collider: Collider | null): EditorCommand {
        return {type: 'set_collider', key, collider};
    }

    static light(light: Light): EditorCommand {
        return {type: 'upsert_light', light};
    }

    static removeLight(key: EntityKey): EditorCommand {
        return {type: 'remove_light', key};
    }

    static camera(camera: Camera): EditorCommand {
        return {type: 'set_camera', camera};
    }

    static frameSelection(): EditorCommand {
        return {type: 'frame_selection'};
    }

    static orbit(yaw: number, pitch: number): EditorCommand {
        return {type: 'orbit_camera', yaw, pitch};
    }

    static pan(right: number, up: number): EditorCommand {
        return {type: 'pan_camera', right, up};
    }

    static dolly(distance: number): EditorCommand {
        return {type: 'dolly_camera', distance};
    }

    static snapping(settings: SnapSettings): EditorCommand {
        return {type: 'set_snap', settings};
    }

    static begin(label: string): EditorCommand {
        return {type: 'begin_transaction', label};
    }

    static commit(): EditorCommand {
        return {type: 'commit_transaction'};
    }

    static cancel(): EditorCommand {
        return {type: 'cancel_transaction'};
    }

    static undo(): EditorCommand {
        return {type: 'undo'};
    }

    static redo(): EditorCommand {
        return {type: 'redo'};
    }

    static play(): EditorCommand {
        return {type: 'start_play'};
    }

    static stop(): EditorCommand {
        return {type: 'stop_play'};
    }

    static pause(paused: boolean): EditorCommand {
        return {type: 'set_paused', paused};
    }

    static playInput(movement: Vec3): EditorCommand {
        return {type: 'set_play_input', movement: [...movement]};
    }

    static playCameraInput(sample: Omit<PlayCameraInputCommand, 'type'>): PlayCameraInputCommand {
        if (![sample.workspace_generation, sample.scene_generation].every(value => Number.isSafeInteger(value) && value >= 0)) {
            throw new RangeError('Camera input generations must be nonnegative safe integers.');
        }
        if (![sample.yaw_delta, sample.pitch_delta, sample.log_distance_delta].every(Number.isFinite)) {
            throw new RangeError('Camera input deltas must be finite.');
        }
        return {...sample, type: 'set_play_camera_input'};
    }
}
