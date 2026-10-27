import {TestBed} from '@angular/core/testing';
import {GraphStore} from '../graph.store';
import {createGraphNode, GraphDocument, graphDocumentError, graphPorts, validGraphValues,} from '../graph.models';
import {defaultLocomotion, defaultLocomotionMachine, locomotionWeights} from './animation.models';

describe('Animation graph structure', () => {
    it('rejects malformed serialized nodes and endpoints without throwing', () => {
        const machine = defaultLocomotionMachine();
        for (const document of [
            {...machine, nodes: [null]},
            {...machine, connections: [null]},
            {
                ...machine,
                connections: [{id: 'broken', from: null, to: {node: 'air', port: 'enter'}}],
            },
        ]) {
            expect(graphDocumentError(document as unknown as GraphDocument)).not.toBeNull();
        }
    });

    function store() {
        TestBed.configureTestingModule({providers: [GraphStore]});
        return TestBed.inject(GraphStore);
    }

    it('permits transition loops and multiple incoming transitions without replacing them', () => {
        const graph = store();
        expect(graph.load(defaultLocomotionMachine())).toBe(true);
        expect(graph.connect({node: 'land', port: 'next'}, {node: 'jump', port: 'enter'})).toBe(
            true,
        );
        expect(
            graph.connect({node: 'grounded', port: 'next'}, {node: 'grounded', port: 'enter'}),
        ).toBe(true);
        expect(graph.document().connections).toHaveLength(8);
        expect(graphDocumentError(graph.document())).toBeNull();
        expect(graph.add('animation.clip', {x: 0, y: 0})).toBeNull();
        expect(graph.add('animation.entry', {x: 0, y: 0})).toBeNull();
        expect(graph.connect({node: 'entry', port: 'next'}, {node: 'jump', port: 'enter'})).toBe(
            false,
        );
    });
    it('rejects malformed transitions atomically', () => {
        const graph = store();
        const machine = defaultLocomotionMachine();
        graph.load(machine);
        const previous = graph.document();
        const edge = machine.connections[0];
        expect(graph.load({...machine, connections: [{...edge, kind: 'data'}]})).toBe(false);
        expect(graph.document()).toBe(previous);
        if (edge.kind !== 'transition') throw new Error('Expected transition');
        expect(graph.setTransition(edge.id, {...edge.settings, duration: -1})).toBe(false);
        expect(
            graph.setTransition(edge.id, {
                ...edge.settings,
                conditions: [{parameter: 'grounded', operator: 'greater', value: 0}],
            }),
        ).toBe(false);
        expect(graph.setTransition(edge.id, {...edge.settings, duration: 0.35})).toBe(true);
        expect(graph.document().connections[0]).toMatchObject({settings: {duration: 0.35}});
    });
    it('clones composite defaults deeply and accepts optional empty motions', () => {
        const first = createGraphNode('animation.locomotion', 'a', {x: 0, y: 0});
        const second = createGraphNode('animation.locomotion', 'b', {x: 0, y: 0});
        first.values.motions.idle = 'clip:idle';
        expect(first.values.machine.nodes[0]).not.toBe(second.values.machine.nodes[0]);
        expect(second.values.motions.idle).toBe('');
        expect(first.values.machine).not.toBe(second.values.machine);
        expect(validGraphValues(first.kind, second.values)).toBe(true);
        expect(
            validGraphValues(first.kind, {
                ...first.values,
                blend: {...first.values.blend, walkSpeed: 5, runSpeed: 2},
            }),
        ).toBe(false);
        expect(
            validGraphValues('animation.blend1d', {thresholdA: 0, thresholdB: 0, thresholdC: 5}),
        ).toBe(false);
        expect(validGraphValues('parameter.int', {name: 'Mode', value: 0.5})).toBe(false);
    });
    it('exposes typed optional ports and prevents orphaning connected Pose inputs', () => {
        const graph = store();
        const node = createGraphNode('animation.locomotion', 'loco', {x: 0, y: 0});
        expect(
            graph.load({
                version: 1,
                domain: 'animation',
                nodes: [node, createGraphNode('animation.reference', 'source', {x: 0, y: 0})],
                connections: [],
            }),
        ).toBe(true);
        const values = {
            ...node.values,
            exposed: {...node.values.exposed, idle: true},
            air: {enabled: true},
            rootMotion: 'extract' as const,
            emitEvents: true,
        };
        expect(graph.setValues(node.id, values)).toBe(true);
        const updated = graph.nodeById().get(node.id)!;
        expect(graphPorts(updated, 'input').map((port) => port.id)).toContain('jump');
        expect(graphPorts(updated, 'output').map((port) => port.id)).toEqual([
            'pose',
            'rootMotion',
            'events',
        ]);
        expect(
            graph.connect({node: 'source', port: 'pose'}, {node: 'loco', port: 'motion.idle'}),
        ).toBe(true);
        expect(
            graph.setValues(node.id, {...values, exposed: {...values.exposed, idle: false}}),
        ).toBe(false);
        expect(graph.document().connections).toHaveLength(1);
        graph.disconnect(graph.document().connections[0].id);
        expect(
            graph.setValues(node.id, {...values, exposed: {...values.exposed, idle: false}}),
        ).toBe(true);
        const serialized = JSON.parse(JSON.stringify(graph.document()));
        expect(graphDocumentError(serialized)).toBeNull();
    });
    it('previews normalized speed and directional weights at the thresholds', () => {
        const values = defaultLocomotion();
        expect(locomotionWeights(values, 0, 0)).toEqual({idle: 1});
        expect(locomotionWeights(values, 0, 2)).toEqual({walk: 1});
        expect(locomotionWeights(values, 0, 5)).toEqual({run: 1});
        expect(locomotionWeights(values, 0, 3.5)).toEqual({walk: 0.5, run: 0.5});
        values.blend.mode = 'direction2d';
        expect(locomotionWeights(values, 2, 0)).toEqual({right: 1});
        expect(locomotionWeights(values, 0, -5)).toEqual({runBackward: 1});
        const diagonal = locomotionWeights(values, Math.SQRT2, Math.SQRT2);
        expect(diagonal['walk']).toBeCloseTo(0.5);
        expect(diagonal['right']).toBeCloseTo(0.5);
        expect(Object.values(diagonal).reduce((sum, weight) => sum + weight, 0)).toBeCloseTo(1);
        values.blend.mode = 'single';
        expect(locomotionWeights(values, 100, 50)).toEqual({idle: 1});
    });
});
