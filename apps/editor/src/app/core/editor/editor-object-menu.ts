import type {EditorCommand} from './editor.models';

type CreateAction =
    | {readonly kind: 'browse'; readonly asset: 'model' | 'prefab'}
    | {readonly kind: 'scene-settings'}
    | {readonly kind: 'create'; readonly command: EditorCommand; readonly allowFolder: boolean};

interface CreateEntry {
    readonly id: string;
    readonly label: string;
    readonly capability?: EditorCommand['type'];
    readonly action: CreateAction;
}

export const OBJECT_MENU_GROUPS = [
    [
        {id: 'create_empty', label: 'Empty Object', capability: 'create_empty',
            action: {kind: 'create', command: {type: 'create_empty'}, allowFolder: true}},
        {id: 'create_camera', label: 'Camera', capability: 'create_camera',
            action: {kind: 'create', command: {type: 'create_camera'}, allowFolder: true}},
        {id: 'create_player', label: 'Player Controller', capability: 'create_player',
            action: {kind: 'create', command: {type: 'create_player'}, allowFolder: true}},
        {id: 'create_audio_source', label: 'Audio Source', capability: 'create_audio_source',
            action: {kind: 'create', command: {type: 'create_audio_source'}, allowFolder: true}},
        {id: 'create_sky', label: 'Sky', capability: 'create_sky',
            action: {kind: 'create', command: {type: 'create_sky'}, allowFolder: true}},
        {id: 'create_particle_emitter', label: 'Particle Emitter', capability: 'create_particle_emitter',
            action: {kind: 'create', command: {type: 'create_particle_emitter'}, allowFolder: true}},
    ],
    [
        {id: 'browse_props', label: 'Props…', action: {kind: 'browse', asset: 'prefab'}},
        {id: 'browse_models', label: 'Models…', action: {kind: 'browse', asset: 'model'}},
    ],
    [
        {id: 'create_point_light', label: 'Point Light', capability: 'create_light',
            action: {kind: 'create', command: {type: 'create_light', light_kind: 'point'}, allowFolder: false}},
        {id: 'create_directional_light', label: 'Directional Light', capability: 'create_light',
            action: {kind: 'create', command: {type: 'create_light', light_kind: 'directional'}, allowFolder: false}},
        {id: 'edit_lights', label: 'Edit Lights…', capability: 'upsert_light', action: {kind: 'scene-settings'}},
    ],
] as const satisfies readonly (readonly CreateEntry[])[];

export type ObjectMenuEntry = (typeof OBJECT_MENU_GROUPS)[number][number];
export type ObjectMenuAction = ObjectMenuEntry['id'];
const entries: readonly ObjectMenuEntry[] = OBJECT_MENU_GROUPS.flatMap<ObjectMenuEntry>(group => group);

export function availableObjectMenuActions(commands: ReadonlySet<string>): readonly ObjectMenuAction[] {
    return entries.filter(item => !('capability' in item) || commands.has(item.capability)).map(item => item.id);
}

export function objectMenuEntry(id: ObjectMenuAction): ObjectMenuEntry | undefined {
    return entries.find(entry => entry.id === id);
}
