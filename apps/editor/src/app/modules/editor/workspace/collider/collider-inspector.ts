import {Component, computed, effect, inject, input, output, signal} from '@angular/core';
import type {Collider} from '../../../../core/editor/editor.models';
import {EditorService} from '../../../../core/editor/editor.service';
import {PHYSICS_LAYER_COUNT} from '../../../../core/editor/physics-layers.models';
import {
    changeColliderNumber, colliderLayerEnabled, colliderNumber, COLLIDER_NUMBER_ERROR,
    normalizeColliderInteraction, parseColliderNumber,
    type ColliderInteractionChange, type ColliderKind, type ColliderNumberField,
} from './collider';

interface NumberField {
    readonly key: string;
    readonly label: string;
    readonly field: ColliderNumberField;
    readonly axis: 0 | 1 | 2;
    readonly positive: boolean;
}

let nextColliderId = 0;

@Component({
    selector: 'editor-collider-inspector',
    templateUrl: './collider-inspector.html',
    styleUrl: './collider-inspector.scss',
})
export class ColliderInspector {
    readonly collider = input<Collider | null>(null);
    readonly contextKey = input('');
    readonly editable = input(false);
    readonly editStarted = output<void>();
    readonly colliderChanged = output<Collider>();
    readonly draftInvalid = output<void>();
    readonly shapeChanged = output<ColliderKind>();
    readonly interactionChanged = output<ColliderInteractionChange>();
    readonly editCommitted = output<void>();
    readonly editCancelled = output<void>();
    protected readonly errorId = `collider-error-${nextColliderId++}`;
    protected readonly numberError = COLLIDER_NUMBER_ERROR;
    protected readonly kind = computed(() => this.collider()?.kind ?? 'none');
    protected readonly interaction = computed(() => normalizeColliderInteraction(this.collider()?.interaction));
    private readonly editor = inject(EditorService, {optional: true});
    private readonly projectLayers = computed(() => this.editor?.snapshot?.()?.project.physics_layers?.layers ?? []);
    protected readonly namedLayers = computed(() => this.projectLayers().filter(layer => !layer.archived));
    protected readonly selectedLayer = computed(() => this.namedLayers().find(layer =>
        this.interaction().layers === 2 ** layer.bit) ?? null);
    protected readonly triggerLayer = computed(() => this.namedLayers().find(layer => layer.id === 'trigger') ?? null);
    protected readonly reviewingTrigger = signal(false);
    protected readonly collisionLayers = computed(() => {
        const labels = new Map(this.projectLayers().map(layer => [layer.bit, layer.label]));
        return Array.from({length: PHYSICS_LAYER_COUNT}, (_value, bit) => ({bit, label: bit + 1, name: labels.get(bit)}));
    });
    protected readonly filterGroups = [
        {field: 'layers', label: 'Belongs to layers', checkboxLabel: 'Belongs to layer', allLabel: 'Belong to all layers', noneLabel: 'Belong to no layers'},
        {field: 'mask', label: 'Detects layers', checkboxLabel: 'Detects layer', allLabel: 'Detect all layers', noneLabel: 'Detect no layers'},
    ] as const;
    protected readonly filterHelpId = `${this.errorId}-filter-help`;
    protected readonly fields = computed<readonly NumberField[]>(() => {
        const collider = this.collider();
        if (!collider) return [];
        const axes = [0, 1, 2] as const;
        const offset = axes.map(axis => ({
            key: `offset-${axis}`, label: `Collider Offset ${['X', 'Y', 'Z'][axis]}`,
            field: 'offset' as const, axis, positive: false,
        }));
        if (collider.kind === 'sphere') {
            return [...offset, {key: 'radius', label: 'Collider Radius', field: 'radius', axis: 0, positive: true}];
        }
        return [...offset, ...axes.map(axis => ({
            key: `half-extents-${axis}`, label: `Collider Half extents ${['X', 'Y', 'Z'][axis]}`,
            field: 'half_extents' as const, axis, positive: true,
        }))];
    });
    protected readonly active = signal<{key: string; raw: string; invalid: boolean} | null>(null);

    constructor() {
        effect(() => {
            this.contextKey();
            this.kind();
            this.editable();
            this.active.set(null);
        });
        effect(() => {
            this.contextKey();
            this.kind();
            this.editable();
            this.triggerLayer();
            this.reviewingTrigger.set(false);
        });
    }

    protected display(field: NumberField): string {
        const active = this.active();
        if (active?.key === field.key) return active.raw;
        const collider = this.collider();
        return collider ? String(colliderNumber(collider, field.field, field.axis)) : '';
    }

    protected changeShape(select: HTMLSelectElement): void {
        const kind = select.value;
        select.value = this.kind();
        if (this.editable() && (kind === 'none' || kind === 'box' || kind === 'sphere')) this.shapeChanged.emit(kind);
    }

    protected changeMode(select: HTMLSelectElement): void {
        const mode = select.value;
        select.value = this.interaction().sensor ? 'sensor' : 'blocking';
        if (this.editable() && this.collider() && (mode === 'blocking' || mode === 'sensor')) {
            this.interactionChanged.emit({kind: 'sensor', sensor: mode === 'sensor'});
        }
    }

    protected chooseLayer(select: HTMLSelectElement): void {
        const selected = this.namedLayers().find(layer => select.value === String(layer.bit));
        const current = this.selectedLayer();
        select.value = current === null ? '' : String(current.bit);
        if (selected && selected !== current) this.changeFilter('layers', 2 ** selected.bit);
    }

    protected useTriggerPreset(): void {
        const layer = this.triggerLayer();
        if (!layer || !this.editable() || !this.collider() || !this.reviewingTrigger()) return;
        this.reviewingTrigger.set(false);
        this.interactionChanged.emit({kind: 'preset', layers: 2 ** layer.bit, sensor: true});
    }

    protected layerEnabled(field: 'layers' | 'mask', bit: number): boolean {
        return colliderLayerEnabled(this.interaction()[field], bit);
    }

    protected changeLayer(field: 'layers' | 'mask', bit: number, input: HTMLInputElement): void {
        const enabled = input.checked;
        input.checked = this.layerEnabled(field, bit);
        if (this.editable() && this.collider()) this.interactionChanged.emit({kind: 'layer', field, bit, enabled});
    }

    protected changeFilter(field: 'layers' | 'mask', value: number): void {
        if (this.editable() && this.collider()) this.interactionChanged.emit({kind: 'filter', field, value});
    }

    protected start(field: NumberField, input: HTMLInputElement): void {
        if (!this.editable() || this.active()?.key === field.key) return;
        this.active.set({key: field.key, raw: input.value, invalid: false});
        this.editStarted.emit();
    }

    protected change(field: NumberField, input: HTMLInputElement): void {
        if (!this.editable()) return;
        this.start(field, input);
        const value = parseColliderNumber(input.value, field.positive);
        this.active.set({key: field.key, raw: input.value, invalid: value === null});
        if (value === null) {
            this.draftInvalid.emit();
            return;
        }
        const collider = this.collider();
        if (collider) this.colliderChanged.emit(changeColliderNumber(collider, field.field, field.axis, value));
    }

    protected finish(field: NumberField, input: HTMLInputElement, cancel = false): void {
        const active = this.active();
        if (active?.key !== field.key) return;
        this.active.set(null);
        input.value = this.display(field);
        if (cancel || active.invalid) this.editCancelled.emit();
        else this.editCommitted.emit();
    }

    protected keydown(event: KeyboardEvent, field: NumberField, input: HTMLInputElement): void {
        if (event.key !== 'Enter' && event.key !== 'Escape') return;
        event.preventDefault();
        event.stopPropagation();
        this.finish(field, input, event.key === 'Escape');
        input.blur();
    }
}
