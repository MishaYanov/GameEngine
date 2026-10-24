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
}

export interface MaterialOverride {
    readonly base_color_factor: Vec4;
}

export type Collider =
    | { readonly kind: 'box'; readonly half_extents: Vec3; readonly offset: Vec3 }
    | { readonly kind: 'sphere'; readonly radius: number; readonly offset: Vec3 };

export interface SceneEntity {
    readonly key: EntityKey;
    readonly name: string;
    readonly model_path: string;
    readonly renderable: boolean;
    readonly transform: Transform;
    readonly parent?: EntityKey | null;
    readonly material?: MaterialOverride | null;
    readonly collider?: Collider | null;
    readonly mesh_index?: number | null;
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

export interface SceneDocument {
    readonly schema_version: 3;
    readonly bindings: Readonly<Record<string, EntityKey>>;
    readonly entities: readonly SceneEntity[];
    readonly camera: Camera;
    readonly lights: readonly Light[];
}

export interface Prefab {
    readonly schema_version: 1;
    readonly entities: readonly SceneEntity[];
}

export interface EntitySpec {
    readonly name: string;
    readonly model_path: string;
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
    readonly type: 'pick_selection';
    readonly pixel: Vec2;
    readonly extent: Vec2;
    readonly additive: boolean;
}
    | { readonly type: 'select'; readonly keys: readonly EntityKey[] }
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
    readonly type: 'set_material';
    readonly key: EntityKey;
    readonly material: MaterialOverride | null;
}
    | { readonly type: 'set_collider'; readonly key: EntityKey; readonly collider: Collider | null }
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
    | { readonly type: 'set_play_input'; readonly movement: Vec3 };

export interface EditorCommandRequest {
    readonly expected_revision: number;
    readonly command: EditorCommand;
}

export interface EditorSnapshot {
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

export interface CommandDescriptor {
    readonly name: EditorCommand['type'];
    readonly label: string;
    readonly category: string;
    readonly undoable: boolean;
    readonly available_in_play: boolean;
}

export interface EditorCapabilities {
    readonly protocol_version: number;
    readonly scene_schema_version: number;
    readonly commands: readonly CommandDescriptor[];
    readonly max_selection: number;
    readonly max_history_entries: number;
    readonly history_budget_bytes: number;
    readonly embedded_viewport_supported: boolean;
}

export interface ModelAsset {
    readonly name: string;
    readonly path: string;
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
}
