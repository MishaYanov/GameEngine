import {TestBed} from '@angular/core/testing';
import {vi} from 'vitest';
import axe from 'axe-core';
import {EditorGraph} from './graph';
import {EXAMPLE_CLIPS, exampleGraph} from '../../../../../projects/design-guide/src/previews/graph.design';
import {createGraphNode, GraphDomain} from './graph.models';
import {GraphStore} from './graph.store';
import {By} from '@angular/platform-browser';

describe('Graph editor', () => {
    const nativeStyle = window.getComputedStyle.bind(window);
    afterEach(() => vi.restoreAllMocks());

    async function setup(domain: GraphDomain = 'shader') {
        vi.spyOn(window, 'getComputedStyle').mockImplementation((element) => nativeStyle(element));
        const fixture = TestBed.createComponent(EditorGraph);
        fixture.componentRef.setInput('graph', exampleGraph(domain));
        fixture.componentRef.setInput('clipAssets', EXAMPLE_CLIPS);
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

    it('edits locomotion with Signal Forms and preserves the clip fallback behind a connected Pose', async () => {
        const {fixture, element, component} = await setup('animation');
        const store = fixture.debugElement.injector.get(GraphStore);
        store.select('locomotion');
        fixture.detectChanges();
        await fixture.whenStable();
        const mode = element.querySelector<HTMLSelectElement>('[aria-label="Blend Mode"]')!;
        mode.value = 'direction2d';
        mode.dispatchEvent(new Event('input'));
        mode.dispatchEvent(new Event('change'));
        fixture.detectChanges();
        await fixture.whenStable();
        expect(element.querySelector('[aria-label="Strafe left clip"]')).not.toBeNull();
        const expose = element.querySelector<HTMLInputElement>(
            '[aria-label="Expose Idle Pose input"]',
        )!;
        const clip = element.querySelector<HTMLSelectElement>('[aria-label="Idle clip"]')!;
        clip.value = 'clip:run';
        clip.dispatchEvent(new Event('input'));
        clip.dispatchEvent(new Event('change'));
        fixture.detectChanges();
        await fixture.whenStable();
        const assigned = component.graph().nodes.find((node) => node.id === 'locomotion');
        expect(assigned?.kind === 'animation.locomotion' && assigned.values.motions.idle).toBe(
            'clip:run',
        );
        clip.value = 'clip:idle';
        clip.dispatchEvent(new Event('input'));
        clip.dispatchEvent(new Event('change'));
        fixture.detectChanges();
        await fixture.whenStable();
        expose.checked = true;
        expose.dispatchEvent(new Event('input'));
        expose.dispatchEvent(new Event('change'));
        fixture.detectChanges();
        await fixture.whenStable();
        expect(element.querySelector('[aria-label="locomotion Idle input, pose"]')).not.toBeNull();
        const reference = store.add('animation.reference', {x: 0, y: 0})!;
        store.connect({node: reference, port: 'pose'}, {node: 'locomotion', port: 'motion.idle'});
        store.select('locomotion');
        fixture.detectChanges();
        await fixture.whenStable();
        expect(element.querySelector<HTMLSelectElement>('[aria-label="Idle clip"]')!.disabled).toBe(
            true,
        );
        expect(
            element.querySelector<HTMLInputElement>('[aria-label="Expose Idle Pose input"]')!.disabled,
        ).toBe(true);
        const wire = store.document().connections.find((edge) => edge.to.port === 'motion.idle')!;
        store.disconnect(wire.id);
        fixture.detectChanges();
        await fixture.whenStable();
        expect(element.querySelector<HTMLSelectElement>('[aria-label="Idle clip"]')!.disabled).toBe(
            false,
        );
        const node = component.graph().nodes.find((node) => node.id === 'locomotion');
        expect(node?.kind === 'animation.locomotion' && node.values.motions.idle).toBe('clip:idle');
        expect(
            (await axe.run(element, {rules: {'color-contrast': {enabled: false}}})).violations.map(
                (v) => ({
                    id: v.id,
                    nodes: v.nodes.map((node) => ({target: node.target, summary: node.failureSummary})),
                }),
            ),
        ).toEqual([]);
    });

    it('edits nested transition conditions without losing the nested selection or parent data', async () => {
        const prototype = HTMLDialogElement.prototype;
        const show = Object.getOwnPropertyDescriptor(prototype, 'showModal');
        const close = Object.getOwnPropertyDescriptor(prototype, 'close');
        Object.defineProperty(prototype, 'showModal', {
            configurable: true,
            value: function (this: HTMLDialogElement) {
                this.open = true;
            },
        });
        Object.defineProperty(prototype, 'close', {
            configurable: true,
            value: function (this: HTMLDialogElement) {
                this.open = false;
                this.dispatchEvent(new Event('close'));
            },
        });
        try {
            const {fixture, element, component} = await setup('animation');
            [...element.querySelectorAll('button')]
                .find((button) => button.textContent?.trim() === 'Open state machine')!
                .click();
            fixture.detectChanges();
            await fixture.whenStable();
            fixture.detectChanges();
            const nested = fixture.debugElement.queryAll(By.directive(EditorGraph))[0];
            expect(nested).toBeDefined();
            const graph = nested.componentInstance as EditorGraph;
            const store = nested.injector.get(GraphStore);
            expect(graph.graph().kind).toBe('stateMachine');
            store.selectConnection('jump-request');
            fixture.detectChanges();
            await fixture.whenStable();
            const duration = element.querySelector<HTMLInputElement>(
                '[aria-label="Transition duration"]',
            )!;
            duration.value = '.35';
            duration.dispatchEvent(new Event('input'));
            fixture.detectChanges();
            await fixture.whenStable();
            fixture.detectChanges();
            expect(store.selectedConnection()).toBe('jump-request');
            expect(graph.graph().connections.find((edge) => edge.id === 'jump-request')).toMatchObject({
                settings: {
                    duration: 0.35,
                    conditions: [{parameter: 'jump', operator: 'true', value: 0}],
                },
            });
            const parent = component.graph().nodes.find((node) => node.id === 'locomotion');
            expect(
                parent?.kind === 'animation.locomotion' &&
                parent.values.machine.connections.find((edge) => edge.id === 'jump-request'),
            ).toMatchObject({settings: {duration: 0.35}});
            expect(parent?.kind === 'animation.locomotion' && parent.values.motions.walk).toBe(
                'clip:walk',
            );
            [...element.querySelectorAll('button')]
                .find((button) => button.textContent?.trim() === 'Add condition')!
                .click();
            fixture.detectChanges();
            await fixture.whenStable();
            const parameter = element.querySelector<HTMLSelectElement>(
                '[aria-label="Condition 2 parameter"]',
            )!;
            parameter.value = 'verticalSpeed';
            parameter.dispatchEvent(new Event('input'));
            parameter.dispatchEvent(new Event('change'));
            fixture.detectChanges();
            await fixture.whenStable();
            expect(element.querySelector('[aria-label="Condition 2 parameter"]')).toBe(parameter);
            expect(
                element.querySelector<HTMLSelectElement>('[aria-label="Condition 2 operator"]')!.value,
            ).toBe('greater');
            store.select('grounded');
            fixture.detectChanges();
            await fixture.whenStable();
            const name = element.querySelector<HTMLInputElement>('[aria-label="State Name"]')!;
            name.value = 'On ground';
            name.dispatchEvent(new Event('input'));
            fixture.detectChanges();
            await fixture.whenStable();
            expect(element.querySelector('[aria-label="Move On ground grounded"]')).not.toBeNull();
            element
                .querySelector<HTMLButtonElement>('[aria-label="Close Locomotion / State machine"]')!
                .click();
            fixture.detectChanges();
            await fixture.whenStable();
            expect(element.querySelector('dialog')).toBeNull();
        } finally {
            if (show) Object.defineProperty(prototype, 'showModal', show);
            else Reflect.deleteProperty(prototype, 'showModal');
            if (close) Object.defineProperty(prototype, 'close', close);
            else Reflect.deleteProperty(prototype, 'close');
        }
    });
});
