import {signal} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import {vi} from 'vitest';
import type {AnimationReview, AnimationSource} from '../../../../core/editor/animation.models';
import {starterAnimator, type AnimatorAsset} from '../../../../core/editor/animator.models';
import type {EditorCommand} from '../../../../core/editor/editor.models';
import {SceneEditingService} from '../scene-editing.service';
import {AnimatorAssets} from './animator-assets.service';
import {AnimatorInspector} from './animator-inspector';

const source: AnimationSource = {
    asset_id: '12345678-1234-4234-8234-123456789014', expected_sha256: 'a'.repeat(64),
    scene_index: 0, mesh_node: 3, skin_index: 0, character: {joints: {0: 'hip', 1: 'knee', 2: 'foot'}, clips: ['idle']},
};
const asset: AnimatorAsset = {
    id: '12345678-1234-4234-8234-123456789012', kind: 'animator_graph', path: 'animations/leg.animator.json',
    bytes: 264, revision: 1, fingerprint: 'b'.repeat(64),
};
const scope = {workspaceGeneration: 4, nativeGeneration: 2, revision: 7};
const review: AnimationReview = {
    workspace_generation: scope.nativeGeneration, revision: scope.revision,
    metadata: {
        source, mesh_index: 0, nodes: 4, joints: 3, clips: [{index: 0, name: 'Idle', duration_seconds: 1}],
        joint_metadata: [
            {source_node: 0, name: 'Hip', stable_id: 'hip'},
            {source_node: 1, name: 'Knee', stable_id: 'knee', parent_source_node: 0},
            {source_node: 2, name: 'Foot', stable_id: 'foot', parent_source_node: 1},
        ]
    },
};

describe('Animator Foot IK settings', () => {
    it('offers the eligible foot and applies its stable ID and distances with the Animator attachment', async () => {
        const actor = {
            key: 'character',
            name: 'Character',
            model_path: 'character.glb',
            asset_id: source.asset_id,
            renderable: true
        };
        const execute = vi.fn(async (_command: EditorCommand) => {
        });
        const inspectAnimation = vi.fn(async () => review);
        const openAnimator = vi.fn(async () => ({
            asset,
            graph: starterAnimator({kind: 'imported', source, clip_index: 0})
        }));
        const snapshot = signal({mode: 'edit', active_scene_id: 'main', revision: scope.revision});
        const library = {
            supported: signal(true), scope: () => scope, current: () => true, accept: vi.fn(),
            assets: signal([asset]), error: signal(null), pending: signal(false),
            editor: {
                snapshot,
                execute,
                inspectAnimation,
                openAnimator,
                workspaceGeneration: signal(scope.workspaceGeneration),
                document: () => ({entities: [actor]}),
                validateAnimatorActor: vi.fn(async () => null)
            },
        };
        TestBed.configureTestingModule({
            imports: [AnimatorInspector],
            providers: [{
                provide: SceneEditingService, useValue: {
                    withCommittedEdit: async (action: () => Promise<void>) => {
                        await action();
                        return true;
                    },
                }
            }],
        }).overrideComponent(AnimatorInspector, {set: {providers: [{provide: AnimatorAssets, useValue: library}]}});
        const fixture = TestBed.createComponent(AnimatorInspector);
        fixture.componentRef.setInput('actor', actor);
        fixture.componentRef.setInput('editable', true);
        await fixture.whenStable();
        const root = fixture.nativeElement as HTMLElement;
        const choose = (select: HTMLSelectElement, value: string) => {
            select.value = value;
            select.dispatchEvent(new Event('change', {bubbles: true}));
        };
        const assetPicker = root.querySelector<HTMLSelectElement>('select')!;
        choose(assetPicker, asset.id);
        await fixture.whenStable();
        expect(inspectAnimation).toHaveBeenCalledWith(source.asset_id, scope.workspaceGeneration, scope.revision);

        const jointPicker = root.querySelector<HTMLSelectElement>('fieldset select')!;
        expect([...jointPicker.options].map(item => [item.text, item.value])).toEqual([['Off', ''], ['Foot', 'foot']]);
        choose(jointPicker, 'foot');
        await fixture.whenStable();
        const reach = root.querySelector<HTMLInputElement>('fieldset input')!;
        const attach = [...root.querySelectorAll<HTMLButtonElement>('button')].find(button => button.textContent?.trim() === 'Attach Animator')!;
        reach.value = '0';
        reach.dispatchEvent(new Event('input'));
        await fixture.whenStable();
        expect(attach.disabled).toBe(true);
        reach.value = '275';
        reach.dispatchEvent(new Event('input'));
        await fixture.whenStable();
        attach.click();
        await fixture.whenStable();
        expect(execute).toHaveBeenCalledWith(expect.objectContaining({
            type: 'add_component', key: actor.key,
            component: expect.objectContaining({
                data: {
                    kind: 'animator', settings: {
                        asset_id: asset.id, binding: {kind: 'imported', source},
                        foot_ik: {foot_joint: 'foot', reach_mm: 275, sole_offset_mm: 100},
                    }
                }
            }),
        }), scope.workspaceGeneration, scope.revision);
    });
});
