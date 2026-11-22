import {afterNextRender, Component, computed, ElementRef, inject, output, signal, viewChild} from '@angular/core';
import type {ComponentRecord} from '../../../../core/editor/editor-components';
import type {EntityKey} from '../../../../core/editor/editor.models';
import type {NestedPropPlacement, NestedPropPlacementRequest} from '../../../../core/editor/linked-prefabs.models';
import {EditorIcon} from '../../../../shared/icons/editor-icon';
import {type PropWorkspaceObject, PropWorkspaceService} from './prop-workspace.service';
import {PropPreviewService} from './prop-preview.service';
import {PropObjectEditor} from './prop-object-editor';
import {NestedPropEditor} from './nested-prop-editor';

@Component({
    selector: 'editor-prop-workspace',
    imports: [EditorIcon, PropObjectEditor, NestedPropEditor],
    templateUrl: './prop-workspace.html',
    styleUrl: './prop-workspace.scss',
})
export class PropWorkspace {
    readonly closed = output<void>();
    protected readonly prop = inject(PropWorkspaceService);
    protected readonly preview = inject(PropPreviewService);
    protected readonly parentName = computed(() => {
        const key = this.prop.selectedObject()?.parent;
        return this.prop.document()?.objects.find(object => object.key === key)?.name ?? 'Prop root';
    });
    private readonly nestedTarget = signal<{readonly draftId: string; readonly key: EntityKey} | null>(null);
    protected readonly nestedParent = computed(() => {
        const target = this.nestedTarget();
        if (!target || this.prop.draft()?.draft_id !== target.draftId) return null;
        return this.prop.document()?.objects.find(object => object.key === target.key) ?? null;
    });
    protected readonly nestedOuterSlots = computed(() => Object.keys(this.prop.draft()?.prefab.reference_slots ?? {}));
    protected readonly placementEditable = computed<PropWorkspaceObject | null>(() => {
        const placement = this.prop.selectedPlacement();
        return placement ? {
            key: `placement:${placement.id}`, name: placement.name, parent: placement.parent,
            transform: placement.local, components: [], level: 1,
        } : null;
    });
    protected readonly placementsByParent = computed(() => {
        const children = new Map<EntityKey, NestedPropPlacement[]>();
        for (const placement of this.prop.document()?.placements ?? []) {
            const siblings = children.get(placement.parent);
            if (siblings) siblings.push(placement);
            else children.set(placement.parent, [placement]);
        }
        return children;
    });
    protected readonly placementSource = computed(() => {
        const placement = this.prop.selectedPlacement();
        return this.prop.draft()?.prefab.composition?.pins.find(pin => placement &&
            pin.version.asset_id === placement.target.asset_id && pin.version.fingerprint === placement.target.fingerprint);
    });
    protected readonly placementBindings = computed(() => {
        const byKey = new Map(this.prop.document()?.objects.map(object => [object.key, object.name]) ?? []);
        return Object.entries(this.prop.selectedPlacement()?.bindings ?? {}).map(([slot, binding]) => ({
            slot, target: binding.kind === 'slot' ? `External slot · ${binding.name}` :
                byKey.get(binding.key) ?? binding.key,
        }));
    });
    private readonly heading = viewChild<ElementRef<HTMLElement>>('heading');

    constructor() {
        afterNextRender(() => this.heading()?.nativeElement.focus({preventScroll: true}));
    }

    protected componentLabel(component: ComponentRecord): string {
        return component.data.kind.replaceAll('_', ' ');
    }

    protected beginNested(): void {
        const draft = this.prop.draft(), object = this.prop.selectedObject();
        if (!draft || !object || !this.prop.canEditDraft()) return;
        this.prop.clearNestedReview();
        this.nestedTarget.set({draftId: draft.draft_id, key: object.key});
    }

    protected endNested(): void {
        this.nestedTarget.set(null);
        this.prop.clearNestedReview();
    }

    protected async placeNested(placement: NestedPropPlacementRequest): Promise<void> {
        if (this.nestedParent()?.key === placement.parent && await this.prop.addNested(placement)) this.endNested();
    }

    protected mayReorder(offset: -1 | 1): boolean {
        const item = this.prop.selectedPlacement();
        if (!item || !this.prop.canEditDraft()) return false;
        const siblings = this.placementsByParent().get(item.parent) ?? [];
        return !!siblings[siblings.findIndex(sibling => sibling.id === item.id) + offset];
    }
}
