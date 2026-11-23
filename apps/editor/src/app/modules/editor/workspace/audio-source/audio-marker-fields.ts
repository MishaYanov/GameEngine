import {Component, computed, effect, inject, input, output, signal, untracked} from '@angular/core';
import {EditorService} from '../../../../core/editor/editor.service';
import type {ClipAssetIndex, TimelineMarker} from '../../../../core/editor/authored-clip.models';
import {AnimationWorkspaceService} from '../animation/animation-workspace.service';
import {EditorSearchPicker} from '../../../../shared/components/search-picker/search-picker';

@Component({
    selector: 'editor-audio-marker-fields',
    imports: [EditorSearchPicker],
    template: `
        <editor-search-picker label="Animation Clip" [value]="clipId() || null" [options]="clipOptions()"
            (picked)="clipChange.emit($event)" [clearable]="true"/>
        <editor-search-picker label="Timeline marker" [value]="markerId() || null" [options]="markerOptions()"
            [disabled]="!clipId() || loading()" (picked)="markerChange.emit($event)"/>
        @if (error(); as message) { <p role="alert">{{ message }}</p> }
        @if (clipId() && !loading() && markerId() && !markers().some(hasSelectedMarker)) {
            <p role="status">The saved marker is unavailable.</p>
        }
        @if (animation) { <button type="button" (click)="animation.openAuthored()">Open Animation</button> }
    `,
})
export class AudioMarkerFields {
    readonly clipId = input.required<string>();
    readonly markerId = input.required<string>();
    readonly clipChange = output<string>();
    readonly markerChange = output<string>();
    protected readonly animation = inject(AnimationWorkspaceService, {optional: true});
    private readonly editor = inject(EditorService);
    private readonly clips = signal<readonly ClipAssetIndex[]>([]);
    protected readonly markers = signal<readonly TimelineMarker[]>([]);
    protected readonly error = signal<string | null>(null);
    protected readonly loading = signal(false);
    protected readonly hasSelectedMarker = (marker: TimelineMarker) => marker.id === this.markerId();
    protected readonly clipOptions = computed(() => this.clips().map(clip => ({id: clip.asset_id, label: clip.path})));
    protected readonly markerOptions = computed(() => this.markers().map(marker => ({
        id: marker.id, label: marker.label, description: `${marker.at} ms`,
    })));

    constructor() {
        effect(onCleanup => {
            const generation = this.editor.workspaceGeneration();
            const catalog = this.editor.authoredClipCatalogVersion();
            let active = true;
            onCleanup(() => { active = false; });
            const owns = () => active && this.editor.workspaceGeneration() === generation &&
                this.editor.authoredClipCatalogVersion() === catalog;
            untracked(() => {
                this.clips.set([]);
                void this.editor.listAuthoredClips(generation).then(clips => {
                    if (owns()) this.clips.set(clips);
                }).catch(() => {
                    if (owns()) this.error.set('Animation Clips could not be listed.');
                });
            });
        });
        effect(onCleanup => {
            const id = this.clipId();
            const generation = this.editor.workspaceGeneration();
            const catalog = this.editor.authoredClipCatalogVersion();
            let active = true;
            onCleanup(() => { active = false; });
            const owns = () => active && this.clipId() === id && this.editor.workspaceGeneration() === generation &&
                this.editor.authoredClipCatalogVersion() === catalog;
            untracked(() => {
                this.markers.set([]);
                this.loading.set(!!id);
                this.error.set(null);
                if (!id) return;
                void this.editor.openAuthoredClip(id, generation).then(document => {
                    if (owns() && document.asset.asset_id === id) this.markers.set(document.clip.markers ?? []);
                }).catch(() => {
                    if (owns()) this.error.set('The saved Animation Clip is unavailable.');
                }).finally(() => {
                    if (owns()) this.loading.set(false);
                });
            });
        });
    }
}
