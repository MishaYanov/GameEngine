import type {ViewportAction, ViewportSettings} from './viewport-editor.models';

export type WorkspaceAction =
    | 'save' | 'saveAll' | 'undo' | 'redo' | 'play' | 'stop'
    | 'selectAll' | 'duplicate' | 'deleteSelection' | 'frameSelection';

type EditorActionDefinition =
    | { readonly target: 'workspace'; readonly label: string; readonly command: WorkspaceAction }
    | {
    readonly target: 'viewport';
    readonly label: string;
    readonly command: ViewportAction | ((settings: ViewportSettings) => ViewportAction);
};

/** Editor intents are independent of the key, pane, or input device that invokes them. */
export const EDITOR_ACTIONS = {
    'scene.tool.hand': {target: 'viewport', label: 'Hand', command: {type: 'set_tool', tool: 'hand'}},
    'scene.tool.move': {target: 'viewport', label: 'Move', command: {type: 'set_tool', tool: 'move'}},
    'scene.tool.rotate': {target: 'viewport', label: 'Rotate', command: {type: 'set_tool', tool: 'rotate'}},
    'scene.tool.scale': {target: 'viewport', label: 'Scale', command: {type: 'set_tool', tool: 'scale'}},
    'scene.frame': {target: 'viewport', label: 'Frame selection', command: {type: 'frame_selection'}},
    'scene.zoom-in': {target: 'viewport', label: 'Zoom in', command: {type: 'zoom', factor: 0.87}},
    'scene.zoom-out': {target: 'viewport', label: 'Zoom out', command: {type: 'zoom', factor: 1 / 0.87}},
    'scene.front': {target: 'viewport', label: 'Front view', command: {type: 'orient', orientation: 'front'}},
    'scene.back': {target: 'viewport', label: 'Back view', command: {type: 'orient', orientation: 'back'}},
    'scene.right': {target: 'viewport', label: 'Right view', command: {type: 'orient', orientation: 'right'}},
    'scene.left': {target: 'viewport', label: 'Left view', command: {type: 'orient', orientation: 'left'}},
    'scene.top': {target: 'viewport', label: 'Top view', command: {type: 'orient', orientation: 'top'}},
    'scene.bottom': {target: 'viewport', label: 'Bottom view', command: {type: 'orient', orientation: 'bottom'}},
    'scene.projection': {
        target: 'viewport', label: 'Toggle perspective / orthographic',
        command: (settings: ViewportSettings): ViewportAction => ({
            type: 'set_projection', projection: settings.projection === 'perspective' ? 'orthographic' : 'perspective',
        }),
    },
    'scene.cancel': {target: 'viewport', label: 'Cancel viewport gesture', command: {type: 'cancel'}},
    'document.save': {target: 'workspace', label: 'Save Scene', command: 'save'},
    'document.undo': {target: 'workspace', label: 'Undo', command: 'undo'},
    'document.redo': {target: 'workspace', label: 'Redo', command: 'redo'},
    'selection.all': {target: 'workspace', label: 'Select all', command: 'selectAll'},
    'selection.duplicate': {target: 'workspace', label: 'Duplicate selection', command: 'duplicate'},
    'selection.delete': {target: 'workspace', label: 'Delete selection', command: 'deleteSelection'},
} as const satisfies Readonly<Record<string, EditorActionDefinition>>;

export type EditorActionId = keyof typeof EDITOR_ACTIONS;

export const VIEWPORT_TOOL_ACTION_IDS = [
    'scene.tool.hand', 'scene.tool.move', 'scene.tool.rotate', 'scene.tool.scale',
] as const satisfies readonly EditorActionId[];
