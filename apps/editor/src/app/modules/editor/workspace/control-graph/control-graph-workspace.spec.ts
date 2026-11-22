import {signal} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import {By} from '@angular/platform-browser';
import {vi} from 'vitest';
import {
    type ControlGraph, type ControlGraphAsset, type ControlGraphProbe, type ControlGraphOutput, readControlGraph,
} from '../../../../core/editor/controls-graph.models';
import {EditorService} from '../../../../core/editor/editor.service';
import {ViewportEditorService} from '../../../../core/editor/viewport-editor.service';
import {EditorGraph} from '../../../../shared/components/graph/graph';
import {WorkspaceViewService} from '../scene-viewport/workspace-view.service';
import {ControlGraphWorkspace} from './control-graph-workspace';
import {ControlGraphWorkspaceService} from './control-graph-workspace.service';

const graph: ControlGraph = {schema_version: 1, nodes: [
    {id: 'sample_jump', data: {kind: 'action', action: 'jump', value: 'button'}},
    {id: 'jump_output', data: {kind: 'jump', input: 'sample_jump'}},
]};
const asset: ControlGraphAsset = {id: '12345678-1234-4234-8234-123456789012', kind: 'controls_graph',
    path: 'controls/player.controls.json', revision: 1, bytes: 252, fingerprint: 'a'.repeat(64)};
const output: ControlGraphOutput = {intent: {
    movement: [0, 0], look: {delta: [0, 0], rate: [0, 0]},
    jump: {held: true, pressed: true, released: false}, attack: {held: false, pressed: false, released: false},
}, presentation: {}};
const result: ControlGraphProbe = {output, order: ['sample_jump', 'jump_output'], active_node_ids: ['sample_jump', 'jump_output']};
const actor = {key: 'player', name: 'Knight', player_controller: {}};

async function setup() {
    const playing = signal(false);
    const view = signal<string>('controls-graph');
    const snapshot = signal({mode: 'edit', revision: 7, selection: ['player'],
        document: {entities: [actor, {...actor, key: 'second', name: 'Second Knight'}]}});
    const workspaceGeneration = signal(1);
    const probe = vi.fn(async (): Promise<ControlGraphProbe> => result);
    const save = vi.fn(async (_: string, saved: ControlGraph) => ({asset, graph: saved}));
    TestBed.configureTestingModule({imports: [ControlGraphWorkspace], providers: [
        ControlGraphWorkspaceService,
        {provide: EditorService, useValue: {
            capabilities: signal({controls_graph_assets: true}), workspaceGeneration, snapshot, playing,
            listControlGraphs: vi.fn(async () => [asset]), openControlGraph: vi.fn(async () => ({asset, graph})),
            probeControlsGraph: probe, saveControlGraph: save,
        }},
        {provide: ViewportEditorService, useValue: {state: signal({workspace_generation: 2})}},
        {provide: WorkspaceViewService, useValue: {controlsGraphView: () => view() === 'controls-graph',
            selectView: (kind: string) => view.set(kind)}},
    ]});
    const fixture = TestBed.createComponent(ControlGraphWorkspace);
    const workspace = TestBed.inject(ControlGraphWorkspaceService);
    fixture.detectChanges();
    await fixture.whenStable();
    const element = fixture.nativeElement as HTMLElement;
    const button = (label: string) => [...element.querySelectorAll<HTMLButtonElement>('button')]
        .find(button => button.textContent?.trim() === label)!;
    const field = (label: string) => [...element.querySelectorAll<HTMLLabelElement>('label')]
        .find(candidate => candidate.textContent?.trim().startsWith(label))!;
    return {fixture, workspace, element, button, field, playing, snapshot, workspaceGeneration, probe, save, view};
}

describe('Controls Graph workspace', () => {
    it('authors a Jump through keyboard-accessible typed ports, simulates a semantic press and saves only explicitly', async () => {
        const {fixture, workspace, element, button, field, probe, save} = await setup();
        button('New Controls Graph').click();
        await fixture.whenStable();
        button('Add Node').focus();
        button('Add Node').click();
        await fixture.whenStable();
        const action = workspace.graph()!.nodes[0];
        expect(document.activeElement).toBe(element.querySelector('[data-control-node][aria-pressed="true"]'));

        field('Node type').querySelector('select')!.value = 'jump';
        button('Add Node').click();
        await fixture.whenStable();
        expect(button('Save Controls Graph').disabled).toBe(true);
        const input = field('Button input').querySelector('select')!;
        expect([...input.options].map(option => option.textContent?.trim())).toEqual(['Unconnected', 'Action · jump · button']);
        input.focus();
        input.value = action.id;
        input.dispatchEvent(new Event('change', {bubbles: true}));
        await fixture.whenStable();
        expect(workspace.graph()!.nodes[1].data).toMatchObject({kind: 'jump', input: action.id});
        expect(save).not.toHaveBeenCalled();

        field('Held').querySelector('input')!.click();
        field('Pressed').querySelector('input')!.click();
        button('Simulate Sample').click();
        await fixture.whenStable();
        expect(probe).toHaveBeenCalledWith(workspace.graph(), [{
            action: 'jump', kind: 'button', held: true, pressed: true, released: false,
        }], null, 'player', {workspaceGeneration: 1, nativeGeneration: 2, revision: 7});
        expect(element.querySelector('[aria-label="Sample intent"]')?.textContent).toContain('Pressed, Held');
        expect(save).not.toHaveBeenCalled();

        button('Save Controls Graph').click();
        await fixture.whenStable();
        expect(save).toHaveBeenCalledTimes(1);
        expect(workspace.asset()?.id).toBe(asset.id);
        expect(workspace.dirty()).toBe(false);
    });

    it('rejects wrong-typed and stale canvas wires and keeps the accepted connection available for Undo', async () => {
        const {fixture, workspace, button} = await setup();
        await workspace.open(asset.id);
        workspace.edit({kind: 'add', node: {id: 'move_input', data: {kind: 'action', action: 'move', value: 'vector2'}}});
        fixture.detectChanges();
        const canvas = fixture.debugElement.query(By.directive(EditorGraph)).componentInstance as EditorGraph;
        const revision = workspace.revision();
        canvas.canvasEdited.emit({kind: 'connect', from: {node: 'move_input', port: 'result'},
            to: {node: 'jump_output', port: 'input'}, revision});
        expect(workspace.error()).toContain('vector2 to button');
        const accepted = workspace.graph();
        canvas.canvasEdited.emit({kind: 'connect', from: {node: 'sample_jump', port: 'result'},
            to: {node: 'jump_output', port: 'input'}, revision: revision - 1});
        expect(workspace.error()).toContain('changed while wiring');
        expect(workspace.graph()).toBe(accepted);
        fixture.detectChanges();
        button('Undo').click();
        expect(workspace.graph()).toEqual(graph);
    });

    it('drops stale probes after actor selection, entering Play, and reopening the authoring view', async () => {
        const {fixture, workspace, probe, snapshot, playing, view} = await setup();
        await workspace.open(asset.id);
        fixture.detectChanges();
        const samples = [{action: 'jump', kind: 'button', pressed: true, held: true, released: false}] as const;
        const releases: ((value: ControlGraphProbe) => void)[] = [];
        probe.mockImplementation(() => new Promise(resolve => releases.push(resolve)));
        const old = workspace.probe(samples);
        snapshot.update(state => ({...state, selection: ['second']}));
        fixture.detectChanges();
        const next = workspace.probe(samples);
        releases[0](result);
        await old;
        expect(workspace.route()).toBeNull();
        expect(workspace.probing()).toBe(true);
        releases[1](result);
        await next;
        expect(workspace.route()).toEqual(result);
        expect(probe.mock.calls).toHaveLength(2);
        playing.set(true);
        view.set('game-preview');
        expect(workspace.route()).toBeNull();
        fixture.detectChanges();
        playing.set(false);
        view.set('controls-graph');
        fixture.detectChanges();
        expect(workspace.route()).toBeNull();
        expect(workspace.graph()).toEqual(graph);
    });

    it('rejects missing ports and cycles in saved graph responses', () => {
        expect(() => readControlGraph({...graph, nodes: [graph.nodes[1]]})).toThrow('compatible Button input');
        const cycle: ControlGraph = {schema_version: 1, nodes: [
            {id: 'a', data: {kind: 'not', input: 'b'}},
            {id: 'b', data: {kind: 'not', input: 'a'}},
            {id: 'output', data: {kind: 'animator_bool', input: 'a', parameter: 'aim'}},
        ]};
        expect(() => readControlGraph(cycle)).toThrow('acyclic');
    });
});