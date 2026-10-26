import {TestBed} from '@angular/core/testing';
import {vi} from 'vitest';
import axe from 'axe-core';
import {EditorGraph} from './graph';
import {exampleGraph} from './graph.design';
import {createGraphNode} from './graph.models';

describe('Graph editor', () => {
    const nativeStyle = window.getComputedStyle.bind(window);
    afterEach(() => vi.restoreAllMocks());

    async function setup() {
        vi.spyOn(window, 'getComputedStyle').mockImplementation((element) => nativeStyle(element));
        const fixture = TestBed.createComponent(EditorGraph);
        fixture.componentRef.setInput('graph', exampleGraph('shader'));
        fixture.detectChanges();
        await fixture.whenStable();
        fixture.detectChanges();
        const element: HTMLElement = fixture.nativeElement;
        const button = (name: string) =>
            element.querySelector<HTMLButtonElement>(`button[aria-label="${name}"]`)!;
        return {fixture, element, button, component: fixture.componentInstance};
    }

    it('renders nodes and editable Signal Forms parameters', async () => {
        const {fixture, element, component} = await setup();
        expect(element.querySelectorAll('editor-graph-node')).toHaveLength(4);
        const input = element.querySelector<HTMLInputElement>('[aria-label="Color Red"]')!;
        input.value = '0.8';
        input.dispatchEvent(new Event('input'));
        fixture.detectChanges();
        await fixture.whenStable();
        expect(component.graph().nodes.find((node) => node.id === 'color')?.values).toMatchObject({
            red: 0.8,
        });
    });
    it('creates and validates connections using keyboard-accessible port buttons', async () => {
        const {fixture, button, component, element} = await setup();
        button('color Color output, vector3').click();
        button('material Roughness input, float').click();
        fixture.detectChanges();
        expect(element.querySelector('[role="alert"]')?.textContent).toContain('vector3 to float');
        button('time Time output, float').click();
        button('material Roughness input, float').click();
        fixture.detectChanges();
        await fixture.whenStable();
        expect(component.graph().connections).toHaveLength(4);
    });
    it('moves a focused node by keyboard and deletes its connections with it', async () => {
        const {fixture, button, component, element} = await setup();
        const handle = button('Move Color color');
        handle.dispatchEvent(new KeyboardEvent('keydown', {key: 'ArrowRight', bubbles: true}));
        fixture.detectChanges();
        await fixture.whenStable();
        expect(component.graph().nodes.find((node) => node.id === 'color')?.position.x).toBe(50);
        handle.dispatchEvent(new KeyboardEvent('keydown', {key: 'Delete', bubbles: true}));
        fixture.detectChanges();
        await fixture.whenStable();
        expect(element.querySelectorAll('editor-graph-node')).toHaveLength(3);
        expect(component.graph().connections).toHaveLength(2);
    });
    it('adds palette nodes and replaces the document when the domain changes', async () => {
        const {fixture, element, component} = await setup();
        [...element.querySelectorAll('button')]
            .find((button) => button.textContent?.includes('Add node'))!
            .click();
        fixture.detectChanges();
        await fixture.whenStable();
        expect(component.graph().nodes).toHaveLength(5);
        fixture.componentRef.setInput('graph', exampleGraph('animation'));
        fixture.detectChanges();
        await fixture.whenStable();
        expect(component.graph().domain).toBe('animation');
        expect(element.textContent).toContain('Animation clip');
    });
    it('has accessible graph, node, port and control labels', async () => {
        const {element} = await setup();
        expect(
            (await axe.run(element, {rules: {'color-contrast': {enabled: false}}})).violations.map(
                (v) => ({id: v.id, targets: v.nodes.map((node) => node.target)}),
            ),
        ).toEqual([]);
    });

    it('rebuilds port anchors when an imported node changes kind while keeping its ID', async () => {
        const {fixture, component, element} = await setup();
        const original = exampleGraph('shader');
        fixture.componentRef.setInput('graph', {
            ...original,
            nodes: original.nodes.map((node) =>
                node.id === 'time' ? createGraphNode('value', 'time', node.position) : node,
            ),
            connections: original.connections.map((edge) =>
                edge.from.node === 'time' ? {...edge, from: {...edge.from, port: 'value'}} : edge,
            ),
        });
        fixture.detectChanges();
        await fixture.whenStable();
        fixture.detectChanges();
        expect(component.graph().nodes.find((node) => node.id === 'time')?.kind).toBe('value');
        expect(element.querySelectorAll('.wire-hit')).toHaveLength(3);
        expect(element.querySelector('[aria-label="time Value output, float"]')).not.toBeNull();
    });
});
