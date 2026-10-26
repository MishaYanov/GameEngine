import {TestBed} from '@angular/core/testing';
import {GraphStore} from './graph.store';
import {createGraphNode, emptyGraph, GraphDocument, GraphNode} from './graph.models';

describe('Typed graph store', () => {
    function setup() {
        TestBed.configureTestingModule({providers: [GraphStore]});
        return TestBed.inject(GraphStore);
    }

    function shader(): GraphDocument {
        return {
            version: 1,
            domain: 'shader',
            nodes: [
                createGraphNode('shader.color', 'color', {x: 0, y: 0}),
                createGraphNode('shader.multiply', 'a', {x: 300, y: 0}),
                createGraphNode('shader.multiply', 'b', {x: 600, y: 0}),
                createGraphNode('shader.time', 'time', {x: 0, y: 300}),
            ],
            connections: [],
        };
    }

    it('keeps animation and shader node schemas separate and rejects invalid positions', () => {
        const store = setup();
        store.load(emptyGraph('animation'));
        expect(store.add('shader.color', {x: 0, y: 0})).toBeNull();
        expect(store.add('animation.clip', {x: Infinity, y: 0})).toBeNull();
        const id = store.add('animation.clip', {x: 10, y: 20})!;
        expect(store.nodeById().get(id)).toMatchObject({
            kind: 'animation.clip',
            values: {clip: 'Walk', speed: 1, loop: true},
        });
    });
    it('connects matching ports and replaces an input wire without duplicating it', () => {
        const store = setup();
        store.load(shader());
        expect(store.connect({node: 'color', port: 'color'}, {node: 'a', port: 'value'})).toBe(
            true,
        );
        store.connect({node: 'color', port: 'color'}, {node: 'a', port: 'value'});
        expect(store.document().connections).toHaveLength(1);
        store.connect({node: 'b', port: 'result'}, {node: 'a', port: 'value'});
        expect(store.document().connections).toHaveLength(1);
        expect(store.document().connections[0].from.node).toBe('b');
    });
    it('rejects wrong directions, incompatible types, self links and cycles', () => {
        const store = setup();
        store.load(shader());
        expect(store.connect({node: 'time', port: 'time'}, {node: 'a', port: 'value'})).toBe(false);
        expect(store.error()).toContain('float to vector3');
        expect(store.connect({node: 'a', port: 'factor'}, {node: 'b', port: 'value'})).toBe(false);
        expect(store.connect({node: 'a', port: 'result'}, {node: 'a', port: 'value'})).toBe(false);
        store.connect({node: 'a', port: 'result'}, {node: 'b', port: 'value'});
        expect(store.connect({node: 'b', port: 'result'}, {node: 'a', port: 'value'})).toBe(false);
        expect(store.error()).toContain('cycle');
        expect(store.document().connections).toHaveLength(1);
    });
    it('removes all attached wires when a node is deleted', () => {
        const store = setup();
        store.load(shader());
        store.connect({node: 'color', port: 'color'}, {node: 'a', port: 'value'});
        store.connect({node: 'a', port: 'result'}, {node: 'b', port: 'value'});
        store.select('a');
        store.remove('a');
        expect(store.document().connections).toEqual([]);
        expect(store.selected()).toBeNull();
    });
    it('edits values and positions without mutating the source document', () => {
        const store = setup();
        const original = shader();
        store.load(original);
        store.move('color', {x: 100, y: 200});
        store.setValues('color', {red: 0.7, green: 0.2, blue: 0.4});
        expect(original.nodes[0].position).toEqual({x: 0, y: 0});
        expect(store.nodeById().get('color')).toMatchObject({
            position: {x: 100, y: 200},
            values: {red: 0.7, green: 0.2, blue: 0.4},
        });
        expect(store.setValues('color', {value: 3})).toBe(false);
        expect(store.setValues('color', {red: NaN, green: 1, blue: 0})).toBe(false);
    });
    it('rejects invalid imports atomically and preserves the current graph', () => {
        const store = setup();
        store.load(shader());
        const current = store.document();
        expect(store.load({...shader(), nodes: [current.nodes[0], current.nodes[0]]})).toBe(false);
        expect(store.document()).toBe(current);
        expect(
            store.load({
                ...shader(),
                connections: [
                    {id: 'bad', from: {node: 'missing', port: 'value'}, to: {node: 'a', port: 'value'}},
                ],
            }),
        ).toBe(false);
        expect(store.document()).toBe(current);
    });

    function strictNodeTypes(): void {
        const invalid: GraphNode = {
            id: 'clip',
            kind: 'animation.clip',
            position: {x: 0, y: 0},
            // @ts-expect-error Animation clips require speed and loop fields.
            values: {clip: 'Walk'},
        };
        void invalid;
    }

    void strictNodeTypes;
});
