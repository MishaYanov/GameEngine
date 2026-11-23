import {signal} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import {describe, expect, it, vi} from 'vitest';
import {EditorService} from '../../../../core/editor/editor.service';
import {validSceneEnvironment, type SceneEnvironment} from '../../../../core/editor/sky.models';
import {SceneEnvironmentInspector} from './scene-environment-inspector';

describe('Scene Environment', () => {
    it('keeps active Sky, ambient and finite sky exposure independently validated', () => {
        expect(validSceneEnvironment({active_sky: 'sky', ambient_linear: [0.1, 0.3, 0.7], sky_exposure_ev: -8})).toBe(true);
        expect(validSceneEnvironment({ambient_linear: [0, 0, 1.01]})).toBe(false);
        expect(validSceneEnvironment({sky_exposure_ev: 8.01})).toBe(false);
        expect(validSceneEnvironment({sky_exposure_ev: Infinity})).toBe(false);
    });

    it('applies separately edited ambient and sky exposure without replacing the active Sky', async () => {
        const environment = signal<SceneEnvironment>({active_sky: 'sky-object'});
        const revision = signal(16);
        const execute = vi.fn(async (command: {ambient_linear: readonly [number, number, number]; sky_exposure_ev: number}) => {
            environment.update(current => ({...current, ambient_linear: command.ambient_linear, sky_exposure_ev: command.sky_exposure_ev}));
            revision.update(value => value + 1);
        });
        TestBed.configureTestingModule({imports: [SceneEnvironmentInspector], providers: [
            {provide: EditorService, useValue: {
                workspaceGeneration: () => 8, execute,
                snapshot: () => ({active_scene_id: 'outside', revision: revision(), document: {environment: environment()}, mode: 'edit'}),
            }},
        ]});
        const fixture = TestBed.createComponent(SceneEnvironmentInspector);
        fixture.componentRef.setInput('editable', true);
        fixture.detectChanges();
        const root = fixture.nativeElement as HTMLElement;
        const field = (text: string) => Array.from(root.querySelectorAll('label'))
            .find(label => label.textContent?.includes(text))?.querySelector('input');
        const enter = (text: string, value: string) => {
            const input = field(text);
            if (!input) throw new Error(`Missing environment field ${text}`);
            input.value = value;
            input.dispatchEvent(new Event('input', {bubbles: true}));
            fixture.detectChanges();
        };
        const button = root.querySelector<HTMLButtonElement>('button');
        expect(field('Ambient red')?.value).toBe('0.25');
        expect(field('Sky Exposure')?.value).toBe('0');
        enter('Sky Exposure', '9');
        expect(button?.disabled).toBe(true);
        expect(root.textContent).toContain('Sky exposure must be between');
        enter('Sky Exposure', '1.5');
        button?.click();
        await vi.waitFor(() => expect(execute).toHaveBeenLastCalledWith({
            type: 'set_scene_environment', ambient_linear: [0.25, 0.25, 0.25], sky_exposure_ev: 1.5,
        }, 8, 16));
        await fixture.whenStable();
        enter('Ambient red', '0.10');
        button?.click();
        await vi.waitFor(() => expect(execute).toHaveBeenLastCalledWith({
            type: 'set_scene_environment', ambient_linear: [0.1, 0.25, 0.25], sky_exposure_ev: 1.5,
        }, 8, 17));
        expect(environment().active_sky).toBe('sky-object');
    });
});
