import {Component, effect, inject, viewChild} from '@angular/core';
import {AnimationWorkspaceService} from '../animation/animation-workspace.service';
import {ComponentEditor} from '../components/component-editor';
import {ComponentWorkspaceSession} from '../components/component-workspace-session';
import {AudioSourceInspector} from './audio-source-inspector';

@Component({
    selector: 'editor-audio-workspace',
    imports: [AudioSourceInspector, ComponentEditor],
    template: `
        <section aria-label="Audio Source" class="audio-workspace">
            <header>
                <h2>{{ session.description() }}</h2>
                @if (session.target(); as target) {
                    @if (target.record.data.kind === 'audio_source') {
                        <editor-audio-source-inspector [componentId]="target.componentId"
                            [settings]="target.record.data.settings" [enabled]="target.record.enabled"
                            [scope]="session.audioScope()" [clips]="session.models()"/>
                        @if (target.record.data.settings.event; as event) {
                            @if (event.kind === 'animation_marker') {
                                <button type="button" [disabled]="session.pending()"
                                    (click)="openMarker(event.clip, event.marker)">Open Saved Animation Marker</button>
                            }
                        }
                    }
                } @else {
                    <p role="status">Select the original object or reopen Audio from another component.</p>
                }
            </header>
            <editor-component-editor [focusedComponent]="session.record()" [ownerKey]="session.ownerKey()"
                [editable]="session.editable()" [pending]="session.pending()" [error]="session.error()"
                [acknowledged]="session.acknowledged()" [contextKey]="session.context()" [models]="session.models()"
                [assetGeneration]="session.target()?.subject?.scope?.workspaceGeneration ?? null"
                (actionRequested)="session.apply($event)"/>
        </section>
    `,
    styles: `
        :host { display: block; height: 100%; overflow: auto; }
        .audio-workspace { max-width: 44rem; margin-inline: auto; padding: 1rem; }
        h2 { margin-block: 0 .75rem; }
    `,
})
export class AudioWorkspace {
    protected readonly session = inject(ComponentWorkspaceSession);
    private readonly animation = inject(AnimationWorkspaceService);
    private readonly editor = viewChild(ComponentEditor);

    constructor() {
        effect(onCleanup => {
            onCleanup(this.session.connectForm(this.editor()));
        });
    }

    protected async openMarker(clip: string, marker: string): Promise<void> {
        const resolve = () => {
            const target = this.session.target();
            if (target?.record.data.kind !== 'audio_source') return null;
            const event = target.record.data.settings.event;
            return event?.kind === 'animation_marker' && event.clip === clip && event.marker === marker
                ? {subject: target.subject, componentId: target.componentId, asset: {id: clip}}
                : null;
        };
        const target = resolve();
        if (target) await this.animation.openScoped({...target, library: 'authored', markerId: marker}, resolve);
    }
}
