import {TestBed} from '@angular/core/testing';
import {defaultTransform, ObjectSubject, ScriptAsset, Subject, TransformSubject,} from './subject.models';
import {SubjectStore} from './subject.store';

const playerScript: ScriptAsset = {
    assetId: 'player',
    path: 'scripts/player.rs',
    className: 'PlayerController',
    properties: [
        {name: 'speed', type: 'number', defaultValue: 5},
        {name: 'active', type: 'boolean', defaultValue: true},
    ],
};
const subjects = (): Subject[] => [
    {id: 'object', kind: 'object', data: {name: 'Object', visible: true}},
    {id: 'transform', kind: 'transform', data: defaultTransform()},
    {id: 'script', kind: 'script', script: null, data: {enabled: true, properties: {}}},
];

describe('SubjectStore', () => {
    let store: InstanceType<typeof SubjectStore>;
    beforeEach(() => {
        TestBed.configureTestingModule({providers: [SubjectStore]});
        store = TestBed.inject(SubjectStore);
        store.setScriptAssets([playerScript]);
        store.setSubjects(subjects());
    });

    it('retains strict discriminated types for every subject', () => {
        expectTypeOf<Extract<Subject, { kind: 'object' }>>().toEqualTypeOf<ObjectSubject>();
        expectTypeOf<Extract<Subject, { kind: 'transform' }>>().toEqualTypeOf<TransformSubject>();
        if (false) {
            // @ts-expect-error Object subjects cannot contain Transform data.
            store.updateSubject({id: 'object', kind: 'object', data: defaultTransform()});
        }
    });

    it('copies caller-owned data and rejects duplicate ids atomically', () => {
        const input = subjects();
        store.setSubjects(input);
        const object = input[0];
        if (object.kind === 'object') object.data.name = 'Changed elsewhere';
        expect(store.subjects()[0].data).toEqual({name: 'Object', visible: true});
        expect(() => store.setSubjects([input[0], input[0]])).toThrow('unique');
        expect(store.subjects()).toHaveLength(3);
    });

    it('rejects mismatched kinds and non-finite transforms without changing state', () => {
        expect(() =>
            store.updateSubject({id: 'object', kind: 'transform', data: defaultTransform()}),
        ).toThrow('type');
        const invalid = defaultTransform();
        invalid.position.x = Number.NaN;
        expect(() =>
            store.updateSubject({id: 'transform', kind: 'transform', data: invalid}),
        ).toThrow('finite');
        expect(store.subjects()[1].data).toEqual(defaultTransform());
    });

    it('attaches a script reference and its declared default properties', () => {
        store.attachScript('script', 'player');
        expect(store.subjects()[2]).toEqual({
            id: 'script',
            kind: 'script',
            script: {assetId: 'player', path: 'scripts/player.rs', className: 'PlayerController'},
            data: {enabled: true, properties: {speed: 5, active: true}},
        });
        expect(() => store.attachScript('object', 'player')).toThrow('not found');
        expect(() => store.attachScript('script', 'unknown')).toThrow('not found');
    });

    it('validates script values against their definitions and preserves edits on reattach', () => {
        store.attachScript('script', 'player');
        store.updateSubject({
            id: 'script',
            kind: 'script',
            data: {enabled: false, properties: {speed: 8, active: true}},
        });
        store.attachScript('script', 'player');
        expect(store.subjects()[2].data).toEqual({
            enabled: false,
            properties: {speed: 8, active: true},
        });
        expect(() =>
            store.updateSubject({
                id: 'script',
                kind: 'script',
                data: {enabled: true, properties: {speed: 'fast', active: true}},
            }),
        ).toThrow('definition');
        expect(() =>
            store.updateSubject({
                id: 'script',
                kind: 'script',
                data: {enabled: true, properties: {speed: 5, active: true, undeclared: 1}},
            }),
        ).toThrow('definition');
    });

    it('detaches scripts and removes their exposed values', () => {
        store.attachScript('script', 'player');
        store.detachScript('script');
        expect(store.subjects()[2]).toEqual({
            id: 'script',
            kind: 'script',
            script: null,
            data: {enabled: true, properties: {}},
        });
        store.removeSubject('script');
        expect(store.subjects()).toHaveLength(2);
    });

    it('reconciles exposed properties when a script definition changes', () => {
        store.attachScript('script', 'player');
        store.updateSubject({
            id: 'script',
            kind: 'script',
            data: {enabled: true, properties: {speed: 8, active: true}},
        });
        store.setScriptAssets([
            {
                ...playerScript,
                properties: [
                    {name: 'speed', type: 'number', defaultValue: 5},
                    {name: 'active', type: 'string', defaultValue: 'yes'},
                    {name: 'jump', type: 'number', defaultValue: 2},
                ],
            },
        ]);
        expect(store.subjects()[2].data).toEqual({
            enabled: true,
            properties: {speed: 8, active: 'yes', jump: 2},
        });
    });

    it('rejects invalid script declarations without replacing the catalog', () => {
        expect(() =>
            store.setScriptAssets([
                {
                    ...playerScript,
                    properties: [{name: 'speed', type: 'number', defaultValue: Number.NaN}],
                },
            ]),
        ).toThrow('definition');
        expect(() =>
            store.setScriptAssets([
                {...playerScript, properties: [playerScript.properties[0], playerScript.properties[0]]},
            ]),
        ).toThrow('unique');
        expect(store.scriptAssets()).toEqual([playerScript]);
    });
});
