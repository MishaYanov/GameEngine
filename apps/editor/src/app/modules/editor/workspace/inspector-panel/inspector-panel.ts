import {
    afterNextRender,
    Component,
    computed,
    effect,
    ElementRef,
    inject,
    Injector,
    input,
    output,
    signal,
    viewChild
} from '@angular/core';
import {Drawer} from "../../../../shared/components/drawer/drawer";
import {EditorPane} from "../../../../shared/components/pane/pane";
import type {Config} from '../../../../shared/models/config.model';
import type {DrawerSettings} from '../../../../shared/components/drawer/drawer-settings';
import type {Collider, SceneEntity, Vec3} from '../../../../core/editor/editor.models';
import {type Vector3Change, Vector3Field} from '../../../../shared/components/vector3-field/vector3-field';
import type {InspectorField, TransformFieldEdit} from '../scene-editing.service';
import {ColliderInspector} from '../collider/collider-inspector';
import type {ColliderInteractionChange, ColliderKind} from '../collider/collider';
import {SceneComponentInspector} from '../components/scene-component-inspector';
import {LinkedPrefabInspector} from '../prefabs/linked-prefab-inspector';

export type {InspectorField} from '../scene-editing.service';
export type TransformProperty = TransformFieldEdit['field'];
export type InspectorTransformChange = TransformFieldEdit;

let nextInspectorId = 0;

@Component({
    selector: 'inspector-panel',
    imports: [
        Drawer,
        EditorPane,
        Vector3Field,
        ColliderInspector,
        SceneComponentInspector,
        LinkedPrefabInspector,
    ],
    templateUrl: './inspector-panel.html',
    styleUrl: './inspector-panel.scss',
})
export class InspectorPanel {
    readonly entity = input<SceneEntity | null>(null);
    readonly drawerMaxWidth = input('45%');
    readonly contextKey = input('');
    readonly editable = input(false);
    readonly editError = input<string | null>(null);
    readonly editStarted = output<InspectorField>();
    readonly nameChanged = output<string>();
    readonly transformChanged = output<InspectorTransformChange>();
    readonly transformResetRequested = output<TransformProperty | 'all'>();
    readonly colliderChanged = output<Collider>();
    readonly colliderInvalid = output<void>();
    readonly colliderShapeChanged = output<ColliderKind>();
    readonly colliderInteractionChanged = output<ColliderInteractionChange>();
    readonly editCommitted = output<void>();
    readonly editCancelled = output<void>();
    private readonly element = inject<ElementRef<HTMLElement>>(ElementRef).nativeElement;
    private readonly injector = inject(Injector);
    private readonly drawer = viewChild(Drawer);
    private readonly components = viewChild(SceneComponentInspector);

    protected readonly positionLinked = signal(false);
    protected readonly rotationLinked = signal(false);
    protected readonly scaleLinked = signal(false);
    protected readonly drawerConfig: Config<DrawerSettings> = {
        width: '20rem',
        minWidth: '12rem',
        maxWidth: '45%',
    };

    // A new context also replaces drafts when two scenes reuse the same entity key.
    protected readonly selectedEntities = computed(() => {
        const entity = this.entity();
        const context = this.contextKey();
        return entity ? [{identity: `${context}:${entity.key}`, entity}] : [];
    });
    protected readonly rotationDegrees = computed<Vec3>(() => {
        const radians = this.entity()?.transform.rotation ?? [0, 0, 0];
        return [radians[0] * 180 / Math.PI, radians[1] * 180 / Math.PI, radians[2] * 180 / Math.PI];
    });
    private readonly nameEntity = signal<string | null>(null);
    private readonly selectionIdentity = computed(() => {
        const entity = this.entity();
        return entity ? `${this.contextKey()}:${entity.key}` : null;
    });
    protected readonly nameDraft = signal('');
    protected readonly invalidName = signal(false);
    protected readonly nameErrorId = `inspector-name-error-${nextInspectorId++}`;
    protected readonly nameValue = computed(() => this.nameEntity() !== null && this.nameEntity() === this.selectionIdentity()
        ? this.nameDraft()
        : this.entity()?.name ?? '');
    private initialName = '';
    private selectedKey: string | null = null;
    private readonly linked = {
        position: this.positionLinked,
        rotation: this.rotationLinked,
        scale: this.scaleLinked,
    };

    constructor() {
        effect(() => {
            const key = this.selectionIdentity();
            if (key === this.selectedKey) return;
            this.selectedKey = key;
            this.nameEntity.set(null);
            this.invalidName.set(false);
        });
    }

    revealReference(key: string, componentId?: string, section?: 'prefab', slot?: string, field?: string): void {
        this.drawer()?.config.update(config => ({...config, collapsed: false, expanded: true}));
        afterNextRender(() => {
            if (this.entity()?.key !== key) return;
            const components = this.components();
            if (components && componentId !== undefined && field !== undefined &&
                components.revealReference(componentId, field)) return;
            const matching = componentId === undefined ? null :
                [...this.element.querySelectorAll<HTMLElement>('[data-reference-component]')]
                    .find(component => component.dataset['referenceComponent'] === componentId);
            const prefab = section === 'prefab' ? this.element.querySelector<HTMLElement>('[data-reference-prefab]') : null;
            const binding = slot === undefined ? null : [...(prefab?.querySelectorAll<HTMLElement>('[data-prefab-slot]') ?? [])]
                .find(element => element.dataset['prefabSlot'] === slot);
            const control = matching ?? binding ?? prefab ??
                this.element.querySelector<HTMLElement>('[data-reference-entity]');
            control?.scrollIntoView?.({block: 'nearest'});
            control?.focus({preventScroll: true});
        }, {injector: this.injector});
    }

    protected startName(input: HTMLInputElement): void {
        if (!this.editable() || this.nameEntity() === this.selectionIdentity()) return;
        this.initialName = input.value;
        this.nameEntity.set(this.selectionIdentity());
        this.nameDraft.set(input.value);
        this.invalidName.set(false);
        this.editStarted.emit('name');
    }

    protected changeName(input: HTMLInputElement): void {
        if (!this.editable()) return;
        this.startName(input);
        this.nameDraft.set(input.value);
        const invalid = input.value.trim().length === 0;
        this.invalidName.set(invalid);
        this.nameChanged.emit(input.value);
    }

    protected finishName(input: HTMLInputElement): void {
        if (this.nameEntity() === null) return;
        this.nameEntity.set(null);
        this.invalidName.set(false);
        input.value = this.entity()?.name ?? '';
        this.editCommitted.emit();
    }

    protected nameKeydown(event: KeyboardEvent, input: HTMLInputElement): void {
        if (event.key !== 'Enter' && event.key !== 'Escape') return;
        event.preventDefault();
        event.stopPropagation();
        if (event.key === 'Escape' && this.nameEntity() !== null) {
            this.nameEntity.set(null);
            this.invalidName.set(false);
            input.value = this.initialName;
            this.editCancelled.emit();
        } else if (event.key === 'Enter') {
            this.finishName(input);
        }
        input.blur();
    }

    protected changeTransform(field: TransformProperty, change: Vector3Change): void {
        this.transformChanged.emit({
            field,
            axis: change.axis,
            value: field === 'rotation' ? change.value * Math.PI / 180 : change.value,
            linked: this.linked[field](),
        });
    }
}
