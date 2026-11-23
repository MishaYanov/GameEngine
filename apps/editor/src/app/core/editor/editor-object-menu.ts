export const OBJECT_MENU_GROUPS = [
    [
        {id: 'browse_props', label: 'Props…'},
        {id: 'browse_models', label: 'Models…'},
    ],
    [
        {id: 'create_empty', label: 'Empty Object', capability: 'create_empty'},
        {id: 'create_camera', label: 'Camera', capability: 'create_camera'},
        {id: 'create_player', label: 'Player Controller', capability: 'create_player'},
        {id: 'create_audio_source', label: 'Audio Source', capability: 'create_audio_source'},
        {id: 'create_sky', label: 'Sky', capability: 'create_sky'},
        {id: 'create_particle_emitter', label: 'Particle Emitter', capability: 'create_particle_emitter'},
    ],
    [
        {id: 'create_point_light', label: 'Point Light', capability: 'create_light'},
        {id: 'create_directional_light', label: 'Directional Light', capability: 'create_light'},
        {id: 'edit_lights', label: 'Edit Lights…', capability: 'upsert_light'},
    ],
] as const;

export type ObjectMenuAction = (typeof OBJECT_MENU_GROUPS)[number][number]['id'];
