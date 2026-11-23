import {Component, computed, effect, inject, input, PendingTasks, signal, untracked} from '@angular/core';
import type {AudioClipDetails as ClipDetails} from '../../../../core/editor/audio-clip.models';
import type {ModelAsset} from '../../../../core/editor/editor.models';
import {EditorService} from '../../../../core/editor/editor.service';
import {ViewportEditorService} from '../../../../core/editor/viewport-editor.service';

const durationFormat = new Intl.NumberFormat(undefined, {maximumFractionDigits: 5});

@Component({
    selector: 'editor-audio-clip-details',
    template: `
        @if (loading()) {
            <p role="status">Reading Audio Clip…</p>
        } @else if (error(); as message) {
            <p role="alert">{{ message }}</p>
            <button type="button" (click)="retry()">Retry Audio Clip details</button>
        } @else if (details(); as clip) {
            <dl aria-label="Audio Clip details">
                <dt>Duration</dt><dd>{{ duration() }} s</dd>
                <dt>Format</dt><dd>16-bit mono PCM WAV</dd>
                <dt>Sample rate</dt><dd>{{ clip.sample_rate_hz }} Hz</dd>
                <dt>Frames</dt><dd>{{ clip.frames }}</dd>
                <dt>Asset ID</dt><dd>{{ asset().asset_id }}</dd>
                <dt>Referenced by</dt>
                <dd>
                    @if (clip.references.length) {
                        <ul>
                            @for (reference of clip.references; track reference.asset_id) {
                                <li>{{ reference.path }}</li>
                            }
                        </ul>
                    } @else {
                        None
                    }
                </dd>
            </dl>
        }
    `,
    styles: `
        :host { display: block; width: 100%; }
        dl { display: grid; grid-template-columns: max-content minmax(0, 1fr); gap: var(--space-1) var(--space-3); margin: 0; }
        dt { color: var(--color-text-secondary); }
        dd, ul { margin: 0; overflow-wrap: anywhere; }
        ul { padding-inline-start: var(--space-4); }
        [role='alert'] { color: var(--color-danger); }
    `,
})
export class AudioClipDetails {
    readonly asset = input.required<ModelAsset>();
    private readonly editor = inject(EditorService);
    private readonly viewport = inject(ViewportEditorService, {optional: true});
    private readonly pendingTasks = inject(PendingTasks);
    private readonly attempts = signal(0);
    protected readonly details = signal<ClipDetails | null>(null);
    protected readonly loading = signal(false);
    protected readonly error = signal<string | null>(null);
    protected readonly duration = computed(() => {
        const details = this.details();
        return details ? durationFormat.format(details.frames / details.sample_rate_hz) : '';
    });

    constructor() {
        effect(onCleanup => {
            const assetId = this.asset().asset_id;
            const generation = this.editor.workspaceGeneration();
            this.attempts();
            this.details.set(null);
            this.error.set(null);
            const revision = untracked(() => this.editor.snapshot()?.revision);
            const nativeGeneration = untracked(() => this.viewport?.state()?.workspace_generation ?? null);
            if (!assetId || revision === undefined) {
                this.loading.set(false);
                this.error.set('The Audio Clip identity is unavailable.');
                return;
            }
            let current = true;
            onCleanup(() => current = false);
            this.loading.set(true);
            this.pendingTasks.run(async () => {
                try {
                    const details = await this.editor.audioClipDetails(assetId, generation, revision, nativeGeneration);
                    if (current) this.details.set(details);
                } catch (cause: unknown) {
                    if (current) this.error.set(cause instanceof Error ? cause.message : String(cause));
                } finally {
                    if (current) this.loading.set(false);
                }
            });
        });
    }

    protected retry(): void {
        this.attempts.update(attempt => attempt + 1);
    }
}
