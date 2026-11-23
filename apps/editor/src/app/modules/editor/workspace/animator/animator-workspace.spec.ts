import {signal} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import {By} from '@angular/platform-browser';
import {vi} from 'vitest';
import {
    type AnimatorAsset, type AnimatorDocument, type AnimatorGraph, starterAnimator,
} from '../../../../core/editor/animator.models';
import {EditorService} from '../../../../core/editor/editor.service';
import {EditorGraph} from '../../../../shared/components/graph/graph';
import {WorkspaceViewService} from '../scene-viewport/workspace-view.service';
import {AnimatorAssets} from './animator-assets.service';
import {runtimeDefinition} from './animator-draft';
import {AnimatorWorkspace} from './animator-workspace';
import {AnimatorWorkspaceService} from './animator-workspace.service';

const asset: AnimatorAsset = {
    id: '12345678-1234-4234-8234-123456789012', kind: 'animator_graph', path: 'animations/knight.animator.json',
    bytes: 740, revision: 3, fingerprint: 'a'.repeat(64),
};
const alternate = {...asset, id: '12345678-1234-4234-8234-123456789013', path: 'animations/squire.animator.json'};
const scope = {workspaceGeneration: 4, nativeGeneration: 2, revision: 7};
const graph = starterAnimator({kind: 'imported', source: {
    asset_id: '12345678-1234-4234-8234-123456789014', expected_sha256: 'f'.repeat(64),
    scene_index: 0, mesh_node: 0, skin_index: 0,
}, clip_index: 0});

async function setup() {
    const open = vi.fn(async (assetId: string): Promise<AnimatorDocument> => ({
        asset: assetId === asset.id ? asset : alternate, graph,
    }));
    const save = vi.fn(async (_: string, updated: AnimatorGraph): Promise<AnimatorDocument> => ({
        asset: {...asset, fingerprint: 'b'.repeat(64), revision: 4}, graph: updated,
    }));
    const workspaceGeneration = signal(scope.workspaceGeneration);
    const current = vi.fn(() => workspaceGeneration() === scope.workspaceGeneration);
    const activeViewKind = signal('scene-viewport');
    const selectView = vi.fn((view: string) => activeViewKind.set(view));
    const accept = vi.fn();
    TestBed.configureTestingModule({
        imports: [AnimatorWorkspace],
        providers: [
            AnimatorWorkspaceService,
            {provide: EditorService, useValue: {
                openAnimator: open, saveAnimator: save, workspaceGeneration, playing: signal(false),
                snapshot: signal({active_scene_id: 'main'}),
            }},
            {provide: AnimatorAssets, useValue: {
                scope: () => scope, current, accept, assets: signal([asset, alternate]), pending: signal(false), error: signal(null),
            }},
            {provide: WorkspaceViewService, useValue: {selectView, activeViewKind}},
        ],
    });
    const fixture = TestBed.createComponent(AnimatorWorkspace);
    const workspace = TestBed.inject(AnimatorWorkspaceService);
    fixture.detectChanges();
    await workspace.open(asset.id);
    fixture.detectChanges();
    const element = fixture.nativeElement as HTMLElement;
    const button = (label: string) => [...element.querySelectorAll<HTMLButtonElement>('button')]
        .find(button => button.textContent?.trim() === label)!;
    const input = (label: string) => [...element.querySelectorAll<HTMLLabelElement>('label')]
        .find(candidate => candidate.textContent?.trim().startsWith(label))!.querySelector('input')!;
    const select = (label: string) => [...element.querySelectorAll<HTMLLabelElement>('label')]
        .find(candidate => candidate.textContent?.trim().startsWith(label))!.querySelector('select')!;
    return {fixture, workspace, element, input, select, button, open, save, accept, selectView, workspaceGeneration};
}

describe('Animator workspace', () => {
    it('adds and focuses a state from the keyboard, explains unreachable states, and saves a connected draft once', async () => {
        const {fixture, workspace, element, input, select, button, save, accept, selectView} = await setup();
        expect(selectView).toHaveBeenCalledWith('animation-controller');
        const stateName = input('New state name');
        stateName.value = 'Run';
        stateName.dispatchEvent(new KeyboardEvent('keydown', {key: 'Enter', bubbles: true}));
        fixture.detectChanges();
        await fixture.whenStable();
        expect(document.activeElement).toBe(element.querySelector('[data-state-id][aria-pressed="true"]'));
        expect(element.textContent).toContain('Run is unreachable from the entry state.');
        expect(button('Save Animator').disabled).toBe(true);
        expect(save).not.toHaveBeenCalled();

        select('From').value = graph.entry;
        select('To').value = workspace.graph()!.states[1].id;
        button('Add Transition').click();
        fixture.detectChanges();
        expect(button('Save Animator').disabled).toBe(false);
        expect(workspace.graph()?.transitions).toHaveLength(1);
        button('Save Animator').click();
        await fixture.whenStable();
        expect(save).toHaveBeenCalledWith(asset.path, workspace.graph(), {id: asset.id, fingerprint: asset.fingerprint}, scope);
        expect(accept).toHaveBeenCalledWith(expect.objectContaining({fingerprint: 'b'.repeat(64)}), scope);
        expect(workspace.dirty()).toBe(false);
    });

    it('orders transitions, validates typed conditions, retains rejected parameter edits and supports Undo and Redo', async () => {
        const {fixture, workspace, select, button, input} = await setup();
        input('Parameter name').value = 'On Ground';
        select('Source').value = 'grounded';
        select('Source').dispatchEvent(new Event('change'));
        fixture.detectChanges();
        button('Add Parameter').click();
        fixture.detectChanges();
        button('Add Transition').click();
        fixture.detectChanges();
        const first = workspace.graph()!.transitions[0];
        button('Add Transition').click();
        fixture.detectChanges();
        const second = workspace.graph()!.transitions[1];
        const parameter = workspace.graph()!.parameters[0];
        const condition = select('Condition');
        condition.value = parameter.id;
        condition.dispatchEvent(new Event('change'));
        fixture.detectChanges();
        expect(workspace.graph()!.transitions[1].condition).toEqual({kind: 'bool', parameter: parameter.id, value: true});

        button('Move Transition Earlier').click();
        fixture.detectChanges();
        expect(workspace.graph()!.transitions.map(edge => edge.id)).toEqual([second.id, first.id]);
        button('Remove').click();
        fixture.detectChanges();
        expect(workspace.error()).toContain('Choose another condition');
        expect(workspace.graph()!.parameters).toEqual([parameter]);
        button('Undo').click();
        fixture.detectChanges();
        expect(workspace.graph()!.transitions.map(edge => edge.id)).toEqual([first.id, second.id]);
        button('Redo').click();
        expect(workspace.graph()!.transitions.map(edge => edge.id)).toEqual([second.id, first.id]);
    });

    it('uses the canvas as a projection, rejects a stale wire and preserves runtime bytes through layout undo and save', async () => {
        const {fixture, workspace} = await setup();
        const canvas = fixture.debugElement.query(By.directive(EditorGraph)).componentInstance as EditorGraph;
        const before = runtimeDefinition(graph);
        const oldRevision = workspace.revision();
        canvas.canvasEdited.emit({kind: 'move_node', id: 'idle', position: {x: 135, y: -25}});
        fixture.detectChanges();
        expect(workspace.graph()?.layout?.['idle']).toEqual({x: 135, y: -25});
        expect(runtimeDefinition(workspace.graph()!)).toBe(before);
        canvas.canvasEdited.emit({kind: 'connect', from: {node: 'idle', port: 'next'},
            to: {node: 'idle', port: 'enter'}, revision: oldRevision});
        expect(workspace.error()).toContain('changed during the wire gesture');
        expect(workspace.graph()?.transitions).toHaveLength(0);
        workspace.undo();
        expect(workspace.graph()?.layout).toBeUndefined();
        workspace.redo();
        expect(await workspace.save()).toBe(true);
        expect(runtimeDefinition(workspace.graph()!)).toBe(before);
        expect(workspace.graph()?.layout?.['idle']).toEqual({x: 135, y: -25});
    });

    it('retains a failed save and asks before replacing a dirty Animator', async () => {
        const {workspace, open, save, selectView} = await setup();
        workspace.edit({kind: 'update_state', state: {...graph.states[0], name: 'Breathe'}});
        save.mockRejectedValueOnce(new Error('Animator bytes changed on disk.'));
        expect(await workspace.save()).toBe(false);
        expect(workspace.graph()?.states[0].name).toBe('Breathe');

        selectView('scene-viewport');
        selectView.mockClear();
        const canceled = workspace.open(alternate.id);
        expect(workspace.confirming()).toBe(true);
        expect(open).toHaveBeenCalledTimes(1);
        await workspace.settleNavigation('cancel');
        expect(await canceled).toBe(false);
        expect(selectView).not.toHaveBeenCalled();
        expect(workspace.dirty()).toBe(true);

        const approved = workspace.open(alternate.id);
        await workspace.settleNavigation('discard');
        expect(await approved).toBe(true);
        expect(workspace.document()?.asset.id).toBe(alternate.id);
        expect(selectView).toHaveBeenCalledExactlyOnceWith('animation-controller');
        expect(workspace.dirty()).toBe(false);
    });
});