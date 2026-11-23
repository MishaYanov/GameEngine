import {scriptDraft, scriptSettings, servesScriptReference} from './script-component-draft';
import type {ScriptDescriptor} from '../../../../core/editor/object-scripts.models';

const door: ScriptDescriptor = {
    id: 'project.open_door_on_trigger', label: 'Open Door on Trigger', config_version: 1,
    fields: [{kind: 'number', key: 'angle', label: 'Angle', min: 0, max: 180, default: 90}],
    references: [{key: 'sensor', label: 'Trigger sensor', requires: 'trigger_collider'}],
};

const sensor = {
    key: 'sensor', name: 'Sensor', collider: {
        kind: 'box' as const, half_extents: [1, 1, 1] as const, offset: [0, 0, 0] as const,
        interaction: {sensor: true, layers: 1, mask: 4_294_967_295},
    },
};

describe('Script inspector draft', () => {
    it('generates defaults and only accepts compatible references and field bounds', () => {
        const draft = {...scriptDraft(), id: door.id, references: {sensor: {entity: 'sensor'}}};
        const targets = [sensor, {key: 'floor', name: 'Floor'}];
        expect(servesScriptReference(targets[1], 'trigger_collider')).toBe(false);
        expect(scriptSettings(draft, [door], targets)).toEqual({
            script_id: door.id, config_version: 1, properties: {angle: 90},
            references: {sensor: {entity: 'sensor'}},
        });
        expect(() => scriptSettings({...draft, properties: {angle: '181'}}, [door], targets)).toThrow('Angle');
        expect(() => scriptSettings({...draft, references: {sensor: {entity: 'floor'}}}, [door], targets)).toThrow('Trigger sensor');
    });

    it('preserves an unavailable script attachment while it is repaired', () => {
        const original = {script_id: 'project.old_script', config_version: 4, properties: {message: 'saved'}};
        expect(scriptSettings(scriptDraft(original), [door], [], original)).toBe(original);
    });

    it('only upgrades a stale configuration after an explicit reset', () => {
        const previous = {script_id: door.id, config_version: 2};
        const draft = {...scriptDraft(previous), references: {sensor: {entity: 'sensor'}}};
        expect(() => scriptSettings(draft, [door], [sensor], previous)).toThrow('unsupported configuration');
        expect(scriptSettings({...draft, resetConfig: true}, [door], [sensor], previous).config_version).toBe(1);
    });

    it('saves only an explicitly declared Prop slot', () => {
        const draft = {...scriptDraft(), id: door.id, references: {sensor: {slot: 'Exit'}}};
        expect(() => scriptSettings(draft, [door], [])).toThrow('Trigger sensor');
        expect(scriptSettings(draft, [door], [], undefined, ['Exit']).references).toEqual(draft.references);
    });
});
