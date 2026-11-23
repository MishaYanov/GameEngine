import {Component, computed, DestroyRef, effect, inject, input, output, signal, untracked} from '@angular/core';
import type {AudioAuditionScope, AudioSourceSettings} from '../../../../core/editor/audio-source.models';
import type {ModelAsset} from '../../../../core/editor/editor.models';
import {AudioAuditionService} from './audio-audition.service';

@Component({
    selector: 'editor-audio-source-inspector',
    template: `
        @if (clip(); as selected) {
            <p>{{ selected.path }}</p>
        } @else {
            <p role="status" tabindex="0">{{ settings().clip ? 'Audio Clip unavailable.' : 'No Audio Clip assigned.' }}</p>
            @if (assignable()) {
                <button type="button" (click)="assign.emit()">Assign Audio Clip…</button>
            }
        }
        @if (scope()) {
            <button type="button" [disabled]="!enabled() || !clip() || playing() || settings().gain === 0"
                (click)="play()">Preview Audio</button>
            <button type="button" [disabled]="!playing()" (click)="stop()">Stop Audio</button>
            @if (settings().gain === 0 && clip()) {
                <p role="status">Audio Source gain is zero.</p>
            }
        }
        @if (error(); as message) { <p role="alert">{{ message }}</p> }
    `,
    styles: ':host { display: block; }',
})
export class AudioSourceInspector {
    readonly componentId = input.required<string>();
    readonly settings = input.required<AudioSourceSettings>();
    readonly enabled = input.required<boolean>();
    readonly clips = input<readonly ModelAsset[]>([]);
    readonly scope = input<AudioAuditionScope | null>(null);
    readonly assignable = input(false);
    readonly assign = output<void>();
    private readonly audition = inject(AudioAuditionService);
    private readonly owner = Symbol('Audio Source Inspector');
    protected readonly error = signal<string | null>(null);
    protected readonly playing = computed(() => this.audition.isPlaying(this.owner));
    protected readonly clip = computed(() => {
        const id = this.settings().clip?.asset;
        return id ? this.clips().find(asset => asset.kind === 'audio_clip' && asset.asset_id === id) : undefined;
    });
    private readonly context = computed(() => JSON.stringify([this.scope(), this.componentId(), this.enabled(),
        this.clip()?.asset_id, this.settings().gain]));

    constructor() {
        effect(() => {
            this.context();
            untracked(() => this.stop());
            this.error.set(null);
        });
        inject(DestroyRef).onDestroy(() => this.stop());
    }

    protected async play(): Promise<void> {
        const scope = this.scope();
        const gain = this.settings().gain;
        if (!scope || !this.enabled() || !this.clip() || gain === 0 || this.playing()) return;
        const context = this.context();
        this.error.set(null);
        const active = () => this.context() === context;
        try {
            await this.audition.play(this.owner, scope, this.componentId(), gain, active);
        } catch (error: unknown) {
            if (active()) this.error.set(error instanceof Error ? error.message : String(error));
        }
    }

    protected stop(): void {
        this.audition.stop(this.owner);
    }
}
