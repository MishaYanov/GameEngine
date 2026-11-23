import {signal} from '@angular/core';
import {TestBed} from '@angular/core/testing';
import {vi} from 'vitest';
import type {ClipAssetDocument, TimelineMarker} from '../../../../core/editor/authored-clip.models';
import {EditorService} from '../../../../core/editor/editor.service';
import {AnimationWorkspaceService} from '../animation/animation-workspace.service';
import {AudioMarkerFields} from './audio-marker-fields';

const assetId = '90000000-0000-4000-8000-000000000053';
const contact: TimelineMarker = {id: 'contact', label: 'Bell contact', at: 150};

function clip(markers: readonly TimelineMarker[]): ClipAssetDocument {
    return {
        workspace_generation: 7,
        asset: {asset_id: assetId, path: 'animations/bell.clip.json', fingerprint: 'a'.repeat(64), revision: 1},
        clip: {schema_version: 2, name: 'Bell', owner: {kind: 'scene', scene_id: 'yard'},
            duration_ticks: 500, tracks: [], markers},
    };
}

function setup(openAuthoredClip = vi.fn(async () => clip([contact]))) {
    const workspaceGeneration = signal(1);
    const authoredClipCatalogVersion = signal(0);
    const snapshot = signal({revision: 1, mode: 'edit'});
    const listAuthoredClips = vi.fn(async () => [clip([]).asset]);
    const revealAuthoredClip = vi.fn();
    TestBed.configureTestingModule({imports: [AudioMarkerFields], providers: [
        {provide: EditorService, useValue: {
            workspaceGeneration, authoredClipCatalogVersion, snapshot, openAuthoredClip, listAuthoredClips,
        }},
        {provide: AnimationWorkspaceService, useValue: {revealAuthoredClip}},
    ]});
    const fixture = TestBed.createComponent(AudioMarkerFields);
    fixture.componentRef.setInput('clipId', assetId);
    fixture.componentRef.setInput('markerId', contact.id);
    fixture.detectChanges();
    const root = fixture.nativeElement as HTMLElement;
    const marker = () => root.querySelector<HTMLInputElement>('editor-search-picker:nth-of-type(2) input');
    return {fixture, root, marker, authoredClipCatalogVersion, snapshot, listAuthoredClips, openAuthoredClip, revealAuthoredClip};
}

it('refreshes a selected clip after publication and keeps its missing marker visible for repair', async () => {
    const {fixture, root, marker, authoredClipCatalogVersion, snapshot, listAuthoredClips, openAuthoredClip, revealAuthoredClip} = setup();
    await vi.waitFor(() => {
        fixture.detectChanges();
        expect(marker()?.value).toBe(contact.label);
    });
    expect(openAuthoredClip).toHaveBeenCalledTimes(1);
    snapshot.set({revision: 2, mode: 'play'});
    await fixture.whenStable();
    expect(listAuthoredClips).toHaveBeenCalledTimes(1);
    expect(openAuthoredClip).toHaveBeenCalledTimes(1);

    openAuthoredClip.mockResolvedValue(clip([]));
    authoredClipCatalogVersion.update(version => version + 1);
    await vi.waitFor(() => {
        fixture.detectChanges();
        expect(root.textContent).toContain('The saved marker is unavailable.');
    });
    expect(marker()?.value).toBe(contact.id);
    expect(openAuthoredClip).toHaveBeenLastCalledWith(assetId, 1);
    root.querySelector<HTMLButtonElement>('button:not([aria-label])')?.click();
    expect(revealAuthoredClip).toHaveBeenCalledWith(assetId, contact.id);
});

it('discards the delayed reply for a previously published version of the same clip', async () => {
    let finishOld: (document: ClipAssetDocument) => void = () => {};
    const older = new Promise<ClipAssetDocument>(resolve => { finishOld = resolve; });
    const open = vi.fn(async () => clip([{...contact, label: 'Current contact'}])).mockReturnValueOnce(older);
    const {fixture, marker, authoredClipCatalogVersion} = setup(open);
    authoredClipCatalogVersion.update(version => version + 1);
    await vi.waitFor(() => {
        fixture.detectChanges();
        expect(marker()?.value).toBe('Current contact');
    });
    finishOld(clip([contact]));
    await fixture.whenStable();
    fixture.detectChanges();
    expect(marker()?.value).toBe('Current contact');
});
