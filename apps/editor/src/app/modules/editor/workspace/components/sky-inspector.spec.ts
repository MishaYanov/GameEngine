import {signal} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import {describe, expect, it, vi} from 'vitest';
import type {SceneDocument, SceneEntity} from '../../../../core/editor/editor.models';
import {validComponentTypes} from '../../../../core/editor/editor-components';
import {EditorService} from '../../../../core/editor/editor.service';
import {validSceneEnvironment, validSkySettings} from '../../../../core/editor/sky.models';
import {SceneEditingService} from '../scene-editing.service';
import {SceneComponentInspector} from './scene-component-inspector';
import {skyComponentDraft, skyComponentFromDraft} from './sky-component-draft';

describe('Sky Inspector', () => {
    it('preserves both linear color channels and rejects invalid yaw or numeric drafts', () => {
        const sky = {zenith: [0.1, 0.3, 0.5], horizon: [0.2, 0.4, 0.6], yaw_degrees: -360} as const;
        expect(skyComponentFromDraft(skyComponentDraft(sky))).toEqual(sky);
        expect(validSkySettings({...sky, horizon: [0, 0.04, NaN]})).toBe(false);
        expect(validSkySettings({...sky, yaw_degrees: 361})).toBe(false);
        expect(validSceneEnvironment({active_sky: 'sky-key'})).toBe(true);
        expect(validSceneEnvironment({active_sky: 'invalid key'})).toBe(false);
        expect(validComponentTypes([{kind: 'sky', type_id: 'engine.sky', label: 'Sky',
            description: 'Background', max_per_object: 1}])).toBe(true);
        expect(() => skyComponentFromDraft({...skyComponentDraft(sky), yawDegrees: '0.1e-99'})).toThrow('Yaw');
        expect(() => skyComponentFromDraft({
            ...skyComponentDraft(sky), horizon: {red: '1.1', green: '0.1', blue: '0.2'},
        })).toThrow('Horizon color');
    });

    it('sets and clears the enabled Sky through the owned scene revision', async () => {
        const sky: SceneEntity = {
            key: 'clouds', name: 'Clouds', model_path: '', renderable: false,
            transform: {position: [0, 0, 0], rotation: [0, 0, 0], scale: [1, 1, 1]},
            sky: {id: 'sky-component', enabled: true, settings: skyComponentFromDraft(skyComponentDraft())},
        };
        const document = signal<Pick<SceneDocument, 'entities' | 'environment'>>({entities: [sky]});
        const capabilities = signal({
            commands: [{name: 'set_active_sky'}],
            component_types: [{kind: 'sky', type_id: 'engine.sky', label: 'Sky', description: 'Background', max_per_object: 1}],
        });
        const revision = signal(10);
        const execute = vi.fn(async (command: {type: string; key: string | null}) => {
            document.update(value => ({...value, environment: {active_sky: command.key}}));
            revision.update(value => value + 1);
        });
        TestBed.configureTestingModule({imports: [SceneComponentInspector], providers: [
            {provide: EditorService, useValue: {
                capabilities, workspaceGeneration: signal(3), document, modelAssets: () => [],
                snapshot: () => ({
                    active_scene_id: 'exterior', revision: revision(), mode: 'edit', document: document(),
                    selection: [sky.key], project: {},
                }),
                execute,
            }},
            {provide: SceneEditingService, useValue: {withCommittedEdit: async (work: () => Promise<void>) => {
                await work();
                return true;
            }}},
        ]});
        const fixture = TestBed.createComponent(SceneComponentInspector);
        fixture.componentRef.setInput('entity', sky);
        fixture.componentRef.setInput('editable', true);
        fixture.detectChanges();
        const button = (name: string) => Array.from(
            (fixture.nativeElement as HTMLElement).querySelectorAll('button'),
        ).find(item => item.textContent?.trim() === name);
        button('Set as Scene Sky')?.click();
        await vi.waitFor(() => expect(execute).toHaveBeenLastCalledWith({type: 'set_active_sky', key: sky.key}, 3, 10));
        await vi.waitFor(() => {
            fixture.detectChanges();
            expect(button('Clear Scene Sky')?.disabled).toBe(false);
        });
        button('Clear Scene Sky')?.click();
        await vi.waitFor(() => expect(execute).toHaveBeenLastCalledWith({type: 'set_active_sky', key: null}, 3, 11));
        const disabled = {...sky, sky: {id: 'sky-component', enabled: false, settings: skyComponentFromDraft(skyComponentDraft())}};
        document.set({entities: [disabled]});
        fixture.componentRef.setInput('entity', disabled);
        fixture.detectChanges();
        expect(button('Set as Scene Sky')?.disabled).toBe(true);
        const child = {...sky, parent: 'gate'};
        document.set({entities: [child]});
        fixture.componentRef.setInput('entity', child);
        fixture.detectChanges();
        expect(button('Set as Scene Sky')?.disabled).toBe(true);
        expect((fixture.nativeElement as HTMLElement).textContent).toContain('Choose a Sky on an independent root object.');
        expect(execute).toHaveBeenCalledTimes(2);
    });
});
