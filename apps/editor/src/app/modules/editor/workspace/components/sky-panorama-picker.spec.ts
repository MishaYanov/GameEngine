import {signal} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import {beforeEach, describe, expect, it, vi} from 'vitest';
import {EditorService} from '../../../../core/editor/editor.service';
import {readSkyPanoramas, validPanoramaPath, type SkyPanoramaAsset} from '../../../../core/editor/sky.models';
import {ViewportEditorService} from '../../../../core/editor/viewport-editor.service';
import {skyComponentDraft, skyComponentFromDraft} from './sky-component-draft';
import {SkyPanoramaPicker} from './sky-panorama-picker';

const sunset: SkyPanoramaAsset = {
    id: '48eeb497-2ca5-4a5e-b0c9-9662b23b4c28', path: 'sky/sunset.png', width: 1024, height: 512,
};

describe('Sky panorama picker', () => {
    const generation = signal(1);
    const snapshot = signal({active_scene_id: 'outside', revision: 4, mode: 'edit', transaction: null});
    const list = vi.fn<() => Promise<readonly SkyPanoramaAsset[]>>();
    const choose = vi.fn(async () => 'C:\\Pictures\\sunset.png');
    const publish = vi.fn(async () => ({asset: {asset_id: sunset.id}}));

    beforeEach(() => {
        generation.set(1);
        list.mockReset().mockResolvedValue([]);
        choose.mockClear();
        publish.mockClear();
        TestBed.configureTestingModule({imports: [SkyPanoramaPicker], providers: [
            {provide: EditorService, useValue: {
                nativeAvailable: true, workspaceGeneration: generation, snapshot, busy: () => false,
                listSkyPanoramas: list, chooseExternalSkyPng: choose, importSkyPanorama: publish,
            }},
            {provide: ViewportEditorService, useValue: {state: () => ({workspace_generation: 7})}},
        ]});
    });

    it('preserves panorama identity and yaw and rejects malformed catalog entries', () => {
        const draft = {...skyComponentDraft(), panorama: sunset.id, yawDegrees: '-183.5'};
        expect(skyComponentFromDraft(draft)).toMatchObject({
            yaw_degrees: -183.5, background: {kind: 'panorama', image: sunset.id},
        });
        expect(readSkyPanoramas([sunset])).toEqual([sunset]);
        expect(() => readSkyPanoramas([sunset, sunset])).toThrow('catalog');
        expect(() => readSkyPanoramas([{...sunset, height: 511}])).toThrow('catalog');
        for (const path of ['../sky.png', '/outside.png', 'C:/sky.png', 'sky/a.jpg']) {
            expect(validPanoramaPath(path)).toBe(false);
        }
    });

    it('imports a reviewed PNG and stages its ID for Sky component Apply', async () => {
        const fixture = TestBed.createComponent(SkyPanoramaPicker);
        const selected = vi.fn();
        fixture.componentInstance.picked.subscribe(selected);
        fixture.componentRef.setInput('editable', true);
        list.mockResolvedValueOnce([]).mockResolvedValueOnce([sunset]);
        fixture.detectChanges();
        const root = fixture.nativeElement as HTMLElement;
        const button = (name: string) => Array.from(root.querySelectorAll('button')).find(item =>
            item.textContent?.trim() === name);
        button('Import Panorama…')?.click();
        await vi.waitFor(() => {
            fixture.detectChanges();
            expect(button('Import and Assign')?.disabled).toBe(false);
        });
        button('Import and Assign')?.click();
        await vi.waitFor(() => expect(selected).toHaveBeenCalledWith(sunset.id));
        expect(choose).toHaveBeenCalledTimes(1);
        expect(publish).toHaveBeenCalledWith('C:\\Pictures\\sunset.png', 'sunset.png', {
            workspaceGeneration: 1, nativeGeneration: 7, revision: 4,
        });
    });

    it('shows the gradient fallback for an unavailable PNG and drops stale catalog reads', async () => {
        const fixture = TestBed.createComponent(SkyPanoramaPicker);
        let resolvePrevious: (assets: readonly SkyPanoramaAsset[]) => void = () => {};
        list.mockImplementationOnce(() => new Promise(resolve => { resolvePrevious = resolve; }));
        fixture.componentRef.setInput('review', true);
        fixture.componentRef.setInput('image', sunset.id);
        fixture.detectChanges();
        generation.set(2);
        fixture.detectChanges();
        await vi.waitFor(() => {
            fixture.detectChanges();
            expect((fixture.nativeElement as HTMLElement).textContent).toContain('The Sky shows its gradient.');
        });
        resolvePrevious([sunset]);
        await fixture.whenStable();
        fixture.detectChanges();
        expect((fixture.nativeElement as HTMLElement).textContent).not.toContain(sunset.path);
    });
});
