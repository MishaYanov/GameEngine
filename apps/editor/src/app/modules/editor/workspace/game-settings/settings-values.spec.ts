import {cameraDraft, followFromDraft} from './follow-camera';
import {courtyardScene, readCourtyardSettings, updateCourtyardScene} from './courtyard-settings';

describe('game settings values', () => {
    it('preserves other scene overrides and safely authors prototype-named scene IDs', () => {
        const original = readCourtyardSettings({schema_version: 1, scenes: {course: {
            kind: 'course', key_trigger: 'key', exit_trigger: 'exit', destination_scene: 'ending',
        }}})!;
        expect(courtyardScene(original, 'constructor')).toBeUndefined();
        const updated = updateCourtyardScene(original, '__proto__', {kind: 'finish', finish_trigger: 'goal'});
        expect(courtyardScene(updated, '__proto__')).toEqual({kind: 'finish', finish_trigger: 'goal'});
        expect(courtyardScene(updated, 'course')).toEqual(courtyardScene(original, 'course'));
        expect(courtyardScene(updateCourtyardScene(updated, '__proto__', null), '__proto__')).toBeUndefined();
        expect(courtyardScene(original, '__proto__')).toBeUndefined();
    });

    it('accepts legacy settings and rejects unsupported payloads without coercion', () => {
        expect(readCourtyardSettings(null)).toEqual({schema_version: 1, scenes: {}});
        for (const value of [
            [], {schema_version: 2, scenes: {}}, {schema_version: 1, scenes: {a: {kind: 'finish'}}},
            {schema_version: 1, scenes: {}, future: true},
            {schema_version: 1, scenes: {a: {kind: 'finish', finish_trigger: 'goal', future: true}}},
            {schema_version: 1, scenes: {a: {kind: 'course', key_trigger: 'key', exit_trigger: 'exit', destination_scene: 'ending', future: true}}},
        ]) {
            expect(readCourtyardSettings(value)).toBeNull();
        }
    });

    it('keeps the gate and checkpoint registry during ordinary scene settings edits', () => {
        const gate = {scene_id: 'court', lever: 'lever', gate: 'gate', action: 'interact', radius: 0.4};
        const checkpoint = {id: 'entry', scene_id: 'court', player: 'player', spawn: [0, 0, 0], activation_radius: 0.2};
        const payload = {
            schema_version: 1, scenes: {}, route_gate: gate, checkpoints: [checkpoint],
            project_save_id: 'c70545a2-181c-42d0-906e-87fb32547914',
        };
        const settings = readCourtyardSettings(payload)!;
        const edited = updateCourtyardScene(settings, 'finish', {kind: 'finish', finish_trigger: 'goal'});
        expect(edited).toEqual({...payload, scenes: {finish: {kind: 'finish', finish_trigger: 'goal'}}});
        expect(readCourtyardSettings({...payload, checkpoints: [{...checkpoint, spawn: [0, 0]}]})).toBeNull();
    });

    it('uses shared follow defaults and rejects incomplete, overflowing or vertical views', () => {
        const draft = {...cameraDraft(null, 'player'), mode: 'follow' as const};
        expect(followFromDraft(draft, ['player'])).toEqual({
            target: 'player', offset: [0, 2, 5], look_offset: [0, 0, 0], smoothing_seconds: 0.15,
        });
        expect(() => followFromDraft({...draft, smoothing: '1e-'}, ['player'])).toThrow('Smoothing');
        expect(() => followFromDraft({...draft, offset: {x: '0', y: '2', z: '0'}}, ['player'])).toThrow('viewing direction');
        expect(() => followFromDraft({...draft, offset: {x: '0', y: '2', z: '1e-50'}}, ['player'])).toThrow('32-bit');
        expect(() => followFromDraft(draft, [])).toThrow('target');
        expect(followFromDraft({...draft, mode: 'fixed'}, [])).toBeNull();
    });
});
