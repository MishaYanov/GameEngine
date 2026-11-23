import {Component, computed, effect, inject, input, signal} from '@angular/core';
import {
    EMPTY_PHYSICS_LAYERS,
    PHYSICS_LAYER_COUNT,
    type PhysicsLayer,
    type PhysicsLayers,
    layerPair,
    layerPairKey,
    standardWorldPreview,
} from '../../../../core/editor/physics-layers.models';
import {EditorService} from '../../../../core/editor/editor.service';
import {EditorDialog} from '../../../../shared/components/dialog/dialog';
import {SceneEditingService} from '../scene-editing.service';

const BITS = Array.from({length: PHYSICS_LAYER_COUNT}, (_, bit) => bit);

@Component({
    selector: 'editor-physics-layers',
    imports: [EditorDialog],
    templateUrl: './physics-layers-dialog.html',
    styleUrl: './physics-layers-dialog.scss',
})
export class PhysicsLayersDialog {
    readonly opening = input(false);
    private readonly editor = inject(EditorService);
    private readonly editing = inject(SceneEditingService);
    private generation = -1;
    private readonly baseline = signal<PhysicsLayers | null>(null);
    protected readonly open = signal(false);
    protected readonly draft = signal<PhysicsLayers>(EMPTY_PHYSICS_LAYERS);
    protected readonly preview = signal<PhysicsLayers | null>(null);
    protected readonly pending = signal(false);
    protected readonly error = signal<string | null>(null);
    protected readonly showAll = signal(false);
    protected readonly dialogConfig = {width: '52rem'};
    protected readonly policy = computed(() => this.preview() ?? this.draft());
    protected readonly busy = computed(() => this.pending() || this.preview() !== null);
    protected readonly occupiedBits = computed(() => new Set(this.draft().layers.map(layer => layer.bit)));
    protected readonly vacantBits = computed(() => BITS.filter(bit => !this.occupiedBits().has(bit)));
    protected readonly changed = computed(() =>
        JSON.stringify(this.draft()) !== JSON.stringify(this.baseline() ?? EMPTY_PHYSICS_LAYERS));
    protected readonly ignored = computed(() =>
        new Set(this.policy().ignored_pairs.map(([a, b]) => layerPairKey(a, b))));
    protected readonly displayed = computed(() => {
        const policy = this.policy();
        const names = new Map(policy.layers.map(layer => [layer.bit, layer]));
        const used = new Set([0, ...names.keys(), ...policy.ignored_pairs.flat()]);
        return BITS.filter(bit => this.showAll() || used.has(bit)).map(bit => {
            const layer = names.get(bit);
            const label = layer?.label ?? (bit === 0 ? 'Default' : 'Bit ' + bit);
            return {bit, label: layer?.archived ? label + ' (archived)' : label};
        });
    });

    constructor() {
        effect(() => {
            if (this.open() && !this.current()) this.open.set(false);
        });
    }

    show(): boolean {
        const snapshot = this.editor.snapshot();
        if (!snapshot || snapshot.mode !== 'edit' || this.opening() || !this.editing.ready()) return false;
        this.generation = this.editor.workspaceGeneration();
        this.baseline.set(snapshot.project.physics_layers ?? null);
        this.draft.set(this.baseline() ?? EMPTY_PHYSICS_LAYERS);
        this.preview.set(null);
        this.error.set(null);
        this.open.set(true);
        return true;
    }

    protected allowed(a: number, b: number): boolean {
        return !this.ignored().has(layerPairKey(a, b));
    }

    protected togglePair(a: number, b: number): void {
        if (this.busy()) return;
        const key = layerPairKey(a, b);
        this.draft.update(policy => ({
            ...policy,
            ignored_pairs: this.allowed(a, b)
                ? [...policy.ignored_pairs, layerPair(a, b)].sort(([i, j], [k, l]) => layerPairKey(i, j) - layerPairKey(k, l))
                : policy.ignored_pairs.filter(([left, right]) => layerPairKey(left, right) !== key),
        }));
    }

    protected addLayer(bit: number, label: string): boolean {
        if (this.busy() || !this.vacantBits().includes(bit) || !this.validLabel(label)) return false;
        const ids = new Set(this.draft().layers.map(layer => layer.id));
        let id = bit === 0 && !ids.has('default') ? 'default' : 'layer_' + bit;
        for (let suffix = 1; ids.has(id); suffix++) id = 'layer_' + bit + '_' + suffix;
        this.draft.update(policy => ({
            ...policy, layers: [...policy.layers, {bit, id, label: label.trim()}].sort((a, b) => a.bit - b.bit),
        }));
        return true;
    }

    protected rename(layer: PhysicsLayer, field: HTMLInputElement): void {
        if (this.busy() || !this.validLabel(field.value)) {
            field.value = layer.label;
            return;
        }
        this.changeLayer({...layer, label: field.value.trim()});
    }

    protected toggleArchive(layer: PhysicsLayer): void {
        if (this.busy()) return;
        this.changeLayer({...layer, archived: !layer.archived});
    }

    protected previewStandardWorld(): void {
        if (this.busy()) return;
        this.error.set(null);
        try {
            this.preview.set(standardWorldPreview(this.draft()));
        } catch (error) {
            this.error.set(error instanceof Error ? error.message : String(error));
        }
    }

    protected usePreview(): void {
        const preview = this.preview();
        if (!preview) return;
        this.draft.set(preview);
        this.preview.set(null);
    }

    protected async apply(): Promise<void> {
        if (!this.open() || !this.current() || this.opening() || this.busy() || !this.changed()) return;
        this.pending.set(true);
        this.error.set(null);
        const baseline = this.baseline();
        try {
            const applied = await this.editing.withCommittedEdit(async () => {
                const snapshot = this.editor.snapshot();
                if (!snapshot || !this.current()) return;
                if (JSON.stringify(snapshot.project.physics_layers ?? null) !== JSON.stringify(baseline)) {
                    throw new Error('Physics layers changed while this dialog was open. Reopen it to review the latest policy.');
                }
                await this.editor.execute({
                    type: 'set_project', project: {...snapshot.project, physics_layers: this.draft()},
                }, this.generation, snapshot.revision);
                if (this.current()) this.open.set(false);
            });
            if (!applied && this.current()) {
                this.error.set(this.editing.error() ?? 'Finish the current edit and try again.');
            }
        } finally {
            this.pending.set(false);
        }
    }

    private changeLayer(next: PhysicsLayer): void {
        this.draft.update(policy => ({
            ...policy, layers: policy.layers.map(layer => layer.bit === next.bit ? next : layer),
        }));
    }

    private validLabel(label: string): boolean {
        const visible = label.trim();
        const valid = visible.length > 0 && new TextEncoder().encode(visible).length <= 80 && !/\p{Cc}/u.test(visible);
        this.error.set(valid ? null : 'Enter a visible layer name of up to 80 bytes.');
        return valid;
    }

    private current(): boolean {
        return this.generation === this.editor.workspaceGeneration() && this.editor.snapshot()?.mode === 'edit';
    }
}
