import {type InputActionMap, readInputActionsAsset, readInputActionsDocument, starterInputActions} from './input-actions.models';

it('round-trips ordered conditional layouts and v1 maps', () => {
    const asset = readInputActionsAsset({
        id: '08d61bdf-e1ce-4fd6-a971-4d662ac1b808',
        path: 'controls.input-actions.json',
        kind: 'input_actions',
        revision: 2,
        fingerprint: 'a'.repeat(64),
        bytes: 1024,
    });
    const defaults = starterInputActions();
    expect(readInputActionsDocument({asset, map: defaults}).map).toEqual(defaults);
    const map: InputActionMap = {...defaults, schema_version: 2, layouts: [
        {id: 'airborne', label: 'Airborne', when: {kind: 'character_grounded', value: false}, overrides: [
            {action_id: 'jump', bindings: []},
        ]},
        {id: 'world', label: 'World Scene', when: {kind: 'scene_is', scene_id: 'lobby'}, overrides: []},
    ]};
    expect(readInputActionsDocument({asset, map: JSON.parse(JSON.stringify(map))}).map).toEqual(map);
    expect(() => readInputActionsDocument({asset, map: {...map, schema_version: 1}})).toThrow(/schema 2/);
    expect(() => readInputActionsDocument({asset, map: {...map, layouts: [
        {...map.layouts![1], when: {kind: 'scene_is', scene_id: 'bad scene'}},
    ]}})).toThrow(/invalid Input Actions map/);
});
