import {Component, computed, inject, input, output} from '@angular/core';
import {EditorService} from '../../../../core/editor/editor.service';

@Component({
    selector: 'editor-particle-sprite-picker',
    template: `
        <label>
            {{ label() }}
            <select #sprite [disabled]="disabled()" [value]="assetId()" (change)="choose(sprite.value)">
                <option value="" [selected]="!assetId()">Colored diamond</option>
                @if (assetId() && !selected()) {
                    <option [value]="assetId()" selected>Unavailable sprite</option>
                }
                @for (item of assets(); track item.asset_id) {
                    <option [value]="item.asset_id" [selected]="item.asset_id === assetId()">{{ item.path }}</option>
                }
            </select>
        </label>
    `,
    styles: [':host, label { display: grid; gap: .25rem; min-inline-size: 0; } select { min-inline-size: 0; max-inline-size: 100%; }'],
})
export class EditorParticleSpritePicker {
    readonly assetId = input('');
    readonly label = input('Sprite');
    readonly disabled = input(false);
    readonly picked = output<string>();
    private readonly editor = inject(EditorService);
    protected readonly assets = computed(() => (this.editor.catalogueFiles?.() ?? []).filter(file =>
        file.kind === 'texture' && !!file.asset_id && file.path.toLowerCase().endsWith('.png')));
    protected readonly selected = computed(() => this.assets().some(file => file.asset_id === this.assetId()));

    constructor() {
        if (this.editor.nativeAvailable) void this.editor.assets().catch(() => undefined);
    }

    protected choose(id: string): void {
        if (!this.disabled() && (!id || this.assets().some(asset => asset.asset_id === id))) this.picked.emit(id);
    }
}
