import {Component, effect, inject, input, output, signal, untracked} from '@angular/core';
import type {ImportedKeyPage, ScopedAnimationPreview} from '../../../../core/editor/animation-workspace.models';
import {EditorService} from '../../../../core/editor/editor.service';

@Component({
    selector: 'editor-imported-clip-keys',
    template: `
        <button type="button" [disabled]="pending()" (click)="load(0, 0)">Inspect Imported Keys</button>
        @if (page(); as page) {
            <section aria-label="Read-only imported keys">
                <label>GLB track
                    <select #track [value]="page.track" [disabled]="pending()" (change)="load(+track.value, 0)">
                        @for (item of page.tracks; track item.index) {
                            <option [value]="item.index">Node {{ item.source_node }} · {{ item.property }} · {{ item.interpolation }}</option>
                        }
                    </select>
                </label>
                <ul>
                    @for (key of page.keys; track $index) {
                        <li><button type="button" (click)="seek.emit(key.at_seconds)">{{ key.at_seconds.toFixed(3) }} s</button>
                            {{ key.value.join(', ') }}</li>
                    }
                </ul>
                <button type="button" [disabled]="pending() || page.offset === 0"
                        (click)="load(page.track, page.offset - 64)">Previous Keys</button>
                <button type="button" [disabled]="pending() || page.offset + page.keys.length >= page.total"
                        (click)="load(page.track, page.offset + 64)">Next Keys</button>
            </section>
        }
        @if (error(); as message) { <p role="alert">{{ message }}</p> }
    `,
})
export class ImportedClipKeys {
    readonly preview = input.required<ScopedAnimationPreview>();
    readonly seek = output<number>();
    private readonly editor = inject(EditorService);
    protected readonly page = signal<ImportedKeyPage | null>(null);
    protected readonly error = signal<string | null>(null);
    protected readonly pending = signal(false);
    private token = '';

    constructor() {
        effect(() => {
            const token = this.preview().token;
            if (this.token !== token) untracked(() => {
                this.token = token;
                this.page.set(null);
                this.error.set(null);
                this.pending.set(false);
            });
        });
    }

    protected async load(track: number, offset: number): Promise<void> {
        const preview = this.preview();
        this.pending.set(true);
        this.error.set(null);
        try {
            const page = await this.editor.importedAnimationKeys(preview.token, track, offset, this.editor.workspaceGeneration());
            if (this.preview().token === preview.token && page.token === preview.token && page.clip_index === preview.clip_index) {
                this.page.set(page);
            }
        } catch (error) {
            if (this.preview().token === preview.token) this.error.set(String(error));
        } finally {
            if (this.preview().token === preview.token) this.pending.set(false);
        }
    }
}
