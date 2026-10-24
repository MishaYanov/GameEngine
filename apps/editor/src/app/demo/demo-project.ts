import type {EditorSnapshot, ModelAsset, SceneDocument, SceneEntity, Vec3,} from '../core/editor/editor.models';

export const DEMO_ASSETS: readonly ModelAsset[] = [
    {name: 'Cube', path: 'models/cube.glb'},
    {name: 'Ground', path: 'models/ground.glb'},
    {name: 'Character', path: 'models/character.glb'},
    {name: 'Tree', path: 'models/tree.glb'},
];

export function demoEntity(key: string, name: string, model: string, position: Vec3): SceneEntity {
    return {
        key,
        name,
        model_path: model,
        renderable: true,
        parent: null,
        transform: {position, rotation: [0, 0, 0], scale: [1, 1, 1]},
    };
}

export function demoScenes(): Record<string, SceneDocument> {
    const base: SceneDocument = {
        schema_version: 3,
        bindings: {},
        camera: {position: [0, 3, 8], yaw: 0, pitch: -0.2, vertical_fov: 1, near: 0.1, far: 100},
        lights: [
            {
                key: 'sun',
                name: 'Sun',
                kind: 'directional',
                position: [0, 5, 0],
                direction: [-1, -1, 0],
                color: [1, 0.95, 0.85],
                intensity: 1.2,
                range: 100,
            },
        ],
        entities: [],
    };
    return {
        playground: {
            ...structuredClone(base),
            entities: [
                {
                    ...demoEntity('ground', 'Ground', 'models/ground.glb', [0, -1, 0]),
                    transform: {position: [0, -1, 0], rotation: [0, 0, 0], scale: [6, 0.2, 6]},
                },
                demoEntity('player', 'Player', 'models/character.glb', [-2, 0, 0]),
                {
                    ...demoEntity('crate', 'Supply crate', 'models/cube.glb', [2, 0, 1]),
                    collider: {kind: 'box', half_extents: [0.5, 0.5, 0.5], offset: [0, 0, 0]},
                },
                {...demoEntity('marker', 'Crate marker', 'models/cube.glb', [0, 1, 0]), parent: 'crate'},
            ],
            bindings: {player: 'player'},
        },
        lighting: {
            ...structuredClone(base),
            entities: [
                demoEntity('pedestal', 'Pedestal', 'models/cube.glb', [0, 0, 0]),
                demoEntity('tree', 'Tree', 'models/tree.glb', [2, 0, -1]),
            ],
        },
    };
}

export function demoProject(documents = demoScenes()): EditorSnapshot {
    const scenes = Object.keys(documents).map((id) => ({
        id,
        asset_path: `scenes/${id}.json`,
        loaded: true,
        dirty: false,
    }));
    return {
        revision: 0,
        scene_revision: 0,
        active_scene_id: 'playground',
        project: {
            schema_version: 2,
            name: 'Editor Playground',
            scenes,
            startup_scene_id: 'playground',
            fixed_hz: 60,
            max_catchup_steps: 8,
            behaviors: [],
        },
        project_dirty: false,
        scenes,
        document: documents['playground'],
        selection: ['player'],
        mode: 'edit',
        paused: false,
        dirty: false,
        can_undo: false,
        can_redo: false,
        transaction: null,
        snap: {enabled: false, translation: 0.25, rotation_radians: Math.PI / 12, scale: 0.1},
    };
}
