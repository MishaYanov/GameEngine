import {Component, computed, input, output, signal} from '@angular/core';
import {parseF32} from '../../../../core/editor/authoring-number';
import type {EntityKey, ModelAsset} from '../../../../core/editor/editor.models';
import type {NestedPropBinding, NestedPropPlacementRequest, NestedPropSource} from '../../../../core/editor/linked-prefabs.models';

import {EditorSearchPicker, type SearchPickerOption} from '../../../../shared/components/search-picker/search-picker';

type SlotChoice = SearchPickerOption & { readonly target: NestedPropBinding };

@Component({
    selector: 'editor-nested-prop-editor',
    imports: [EditorSearchPicker],
    templateUrl: './nested-prop-editor.html',
    styles: `
        form, label {display: grid; gap: var(--space-1);}
        fieldset {display: grid; gap: var(--space-2); min-width: 0; margin: 0; padding: var(--space-2);
            border: 1px solid var(--color-border);}
        legend {max-width: 100%; overflow-wrap: anywhere;}
        label {min-width: 0;}
        input, button {
            padding: var(--space-1) var(--space-2);
            border: 1px solid var(--color-border);
            border-radius: var(--radius-sm);
            background: var(--color-control);
            color: var(--color-text);
            font: inherit;
            max-width: 100%;
        }
        button:disabled {color: var(--color-text-disabled);}
    `,
})
export class NestedPropEditor {
    readonly assets = input.required<readonly ModelAsset[]>();
    readonly ownerAssetId = input.required<string>();
    readonly parent = input.required<{readonly key: EntityKey; readonly name: string}>();
    readonly objects = input.required<readonly {readonly key: EntityKey; readonly name: string}[]>();
    readonly outerSlots = input.required<readonly string[]>();
    readonly review = input<NestedPropSource | null>(null);
    readonly pending = input(false);
    readonly reviewRequested = output<{readonly assetId: string; readonly fingerprint: string}>();
    readonly placementRequested = output<NestedPropPlacementRequest>();
    readonly cancelled = output<void>();

    protected readonly selected = signal('');
    protected readonly name = signal('');
    protected readonly position = signal<readonly [string, string, string]>(['0', '0', '0']);
    protected readonly bindings = signal<ReadonlyMap<string, string>>(new Map());
    protected readonly axes = ['X', 'Y', 'Z'];
    protected readonly available = computed(() => this.assets().filter(asset =>
        asset.kind === 'prefab' && asset.asset_id && asset.asset_id !== this.ownerAssetId() && asset.fingerprint,
    ));
    protected readonly assetOptions = computed<readonly SearchPickerOption[]>(() =>
        this.available().flatMap(asset => asset.asset_id
            ? [{id: asset.asset_id, label: asset.name, description: asset.path}] : []));
    protected readonly choices = computed<readonly SlotChoice[]>(() => [
        ...this.objects().map(object => ({
            id: 'object:' + object.key, label: object.name, description: 'Object in this Prop',
            target: {kind: 'object' as const, key: object.key},
        })),
        ...this.outerSlots().map(name => ({
            id: 'slot:' + name, label: name, description: 'External slot',
            target: {kind: 'slot' as const, name},
        })),
    ]);
    private readonly bindingTargets = computed(() =>
        new Map(this.choices().map(choice => [choice.id, choice.target])));
    protected readonly currentReview = computed(() => {
        const reviewed = this.review();
        const asset = this.available().find(item => item.asset_id === this.selected());
        return reviewed && asset && reviewed.asset_id === asset.asset_id &&
            reviewed.fingerprint === asset.fingerprint ? reviewed : null;
    });
    protected readonly placement = computed<NestedPropPlacementRequest | null>(() => {
        const review = this.currentReview();
        const [x, y, z] = this.position().map(parseF32);
        const name = this.name().trim();
        if (!review || !name || /\p{Cc}/u.test(name) || new TextEncoder().encode(name).length > 256 ||
            x == null || y == null || z == null) return null;
        const slotChoices = this.bindingTargets();
        const bindings: [string, NestedPropBinding][] = [];
        for (const slot of review.slots) {
            const target = slotChoices.get(this.bindings().get(slot) ?? '');
            if (!target) return null;
            bindings.push([slot, target]);
        }
        return {
            asset_id: review.asset_id, fingerprint: review.fingerprint, name, parent: this.parent().key,
            local: {position: [x, y, z], rotation: [0, 0, 0], scale: [1, 1, 1]},
            bindings: Object.fromEntries(bindings),
        };
    });

    protected chooseAsset(assetId: string): void {
        if (this.pending() || assetId === this.selected()) return;
        const asset = this.available().find(item => item.asset_id === assetId);
        if (assetId && !asset) return;
        this.selected.set(assetId);
        this.bindings.set(new Map());
        this.name.set(asset?.name ?? '');
        if (asset?.asset_id && asset.fingerprint) {
            this.reviewRequested.emit({assetId: asset.asset_id, fingerprint: asset.fingerprint});
        }
    }

    protected setPosition(axis: number, value: string): void {
        this.position.update(current => [
            axis === 0 ? value : current[0], axis === 1 ? value : current[1], axis === 2 ? value : current[2],
        ]);
    }

    protected assign(slot: string, value: string): void {
        if (this.pending() || !this.currentReview()?.slots.includes(slot) ||
            value && !this.bindingTargets().has(value)) return;
        this.bindings.update(current => new Map(current).set(slot, value));
    }

    protected place(event: Event): void {
        event.preventDefault();
        const placement = this.placement();
        if (placement && !this.pending()) this.placementRequested.emit(placement);
    }
}
