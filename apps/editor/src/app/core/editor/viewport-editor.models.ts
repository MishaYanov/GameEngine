/** Editor-only wire contracts mirror bridge::viewport, never SceneDocument. */
export type ViewportTool = 'hand' | 'move' | 'rotate' | 'scale';
export type ViewportSpace = 'world' | 'local';
export type ViewportPivot = 'pivot' | 'center';
export type ViewportProjection = 'perspective' | 'orthographic';
export type ViewportPlane = 'xz' | 'xy' | 'yz';
export type ViewportView = 'scene' | 'game';
export type ViewportOrientation = 'free' | 'top' | 'bottom' | 'front' | 'back' | 'left' | 'right';
export type EditorWarmingMode = 'off' | 'references' | 'models';

export interface SceneWarmingResult {
    readonly scene_id: string;
    readonly scene_revision: number;
    readonly request_id: string;
    readonly mode: EditorWarmingMode;
    readonly mode_revision: number;
    readonly references_hit: boolean;
    readonly prepared_models: number;
    readonly adopted_models: number;
    readonly retained_bytes: number;
}

export interface ViewportSettings {
    readonly tool: ViewportTool;
    readonly space: ViewportSpace;
    readonly pivot: ViewportPivot;
    readonly grid: boolean;
    readonly colliders: boolean;
    readonly grid_spacing: number;
    readonly snap: boolean;
    readonly move_snap: number;
    readonly rotation_snap_degrees: number;
    readonly scale_snap: number;
    readonly projection: ViewportProjection;
    readonly plane: ViewportPlane;
    readonly view: ViewportView;
}

export interface ViewportEditorState {
    readonly settings: ViewportSettings;
    readonly camera_position: readonly [number, number, number];
    readonly camera_target: readonly [number, number, number];
    readonly camera_distance: number;
    readonly revision: number;
    readonly workspace_generation: number | null;
    readonly gesture: string | null;
    readonly placement: {readonly path: string; readonly position: readonly [number, number, number] | null} | null;
    readonly focused: boolean;
    readonly error: string | null;
    readonly collider_error?: string | null;
    readonly transform_preview?: ViewportTransformPreview | null;
    readonly game_status?: PlayStatus | null;
    readonly scene_warming_result?: SceneWarmingResult | null;
    readonly orbit_status?: {
        readonly blocker: string | null;
        readonly recovering: boolean;
        readonly target_lost: boolean
    } | null;
}

export interface PlayStatus {
    readonly play_session_id: string;
    readonly run_id: string;
    readonly scene_generation: number;
    readonly scene_id: string;
    readonly paused: boolean;
    readonly pending_scene_id: string | null;
    readonly gameplay: {
        readonly objective: string;
        readonly detail: string | null;
        readonly score: number | null;
        readonly completed: boolean;
    } | null;
}

export type ViewportAction =
    | {readonly type: 'set_tool'; readonly tool: ViewportTool}
    | {readonly type: 'set_space'; readonly space: ViewportSpace}
    | {readonly type: 'set_pivot'; readonly pivot: ViewportPivot}
    | {readonly type: 'set_grid'; readonly visible: boolean}
    | {readonly type: 'set_colliders'; readonly visible: boolean}
    | {readonly type: 'set_grid_spacing'; readonly spacing: number}
    | {readonly type: 'set_snap'; readonly enabled: boolean}
    | {readonly type: 'set_snap_increments'; readonly translation: number; readonly rotation_degrees: number; readonly scale: number}
    | {readonly type: 'set_projection'; readonly projection: ViewportProjection}
    | {readonly type: 'set_plane'; readonly plane: ViewportPlane}
    | {readonly type: 'set_view'; readonly view: ViewportView}
    | { readonly type: 'restart_run'; readonly run_id: string; readonly expected_revision: number }
    | {readonly type: 'orient'; readonly orientation: ViewportOrientation}
    | {readonly type: 'zoom'; readonly factor: number}
    | {readonly type: 'frame_selection'}
    | {readonly type: 'reset_view'}
    | {readonly type: 'cancel'}
    | {readonly type: 'begin_placement'; readonly path: string}
    | {readonly type: 'update_placement'; readonly normalized: readonly [number, number] | null}
    | {readonly type: 'commit_placement'}
    | {readonly type: 'cancel_placement'};

export interface ViewportDocumentChange {
    readonly workspace_generation: number;
    readonly revision: number;
}

export interface ViewportTransformPreview {
    readonly key: string;
    readonly transform: import('./editor.models').Transform;
}

export const DEFAULT_VIEWPORT_SETTINGS: ViewportSettings = {
    tool: 'move', space: 'world', pivot: 'pivot', grid: true, colliders: false, grid_spacing: 1,
    snap: false, move_snap: 1, rotation_snap_degrees: 15, scale_snap: 0.1,
    projection: 'perspective', plane: 'xz', view: 'scene',
};
