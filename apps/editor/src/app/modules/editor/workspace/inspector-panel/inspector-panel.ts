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
import {NgTemplateOutlet} from '@angular/common';
import {AUTHORING_DRAWER_CONFIG} from '../agent-drawer/authoring-drawer';
import type {Collider, EntityKey, SceneEntity, Vec3} from '../../../../core/editor/editor.models';
import {EditorService, type WorkspaceAssetScope} from '../../../../core/editor/editor.service';
import {ViewportEditorService} from '../../../../core/editor/viewport-editor.service';
import {type Vector3Change, Vector3Field} from '../../../../shared/components/vector3-field/vector3-field';
import type {InspectorField, TransformFieldEdit} from '../scene-editing.service';
import {ColliderInspector} from '../collider/collider-inspector';
import type {ColliderInteractionChange, ColliderKind} from '../collider/collider';
import {BatchTransformInspector} from './batch-transform-inspector';
import {SceneComponentInspector} from '../components/scene-component-inspector';
import {LinkedPrefabInspector} from '../prefabs/linked-prefab-inspector';
import {
    inspectorAddressKey, type InspectorSubject, linkedInspectorRoot, linkedInspectorSubject, type LinkedInspectorTarget,
    sameLinkedInspectorTarget, sceneInspectorSubject, watchLinkedInspectorAddress,
} from './inspector-subject';

export type {InspectorField} from '../scene-editing.service';
export type TransformProperty = TransformFieldEdit['field'];
export type InspectorTransformChange = TransformFieldEdit;

let nextInspectorId = 0;

@Component({
    selector: 'inspector-panel',
    imports: [
        Drawer,
        NgTemplateOutlet,
        EditorPane,
        Vector3Field,
        ColliderInspector,
        SceneComponentInspector,
        BatchTransformInspector,
        LinkedPrefabInspector,
    ],
    templateUrl: './inspector-panel.html',
    styleUrl: './inspector-panel.scss',
})
export class InspectorPanel {
    readonly entity = input<SceneEntity | null>(null);
    readonly batchSelection = input<readonly SceneEntity[]>([]);
    readonly batchTransformRequested = output<TransformFieldEdit>();
    readonly activationRequested = output<boolean>();
    readonly embedded = input(false);
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
    readonly physicsLayersRequested = output<void>();
    private readonly element = inject<ElementRef<HTMLElement>>(ElementRef).nativeElement;
    private readonly injector = inject(Injector);
    private readonly drawer = viewChild(Drawer);
    private readonly parentDrawer = inject(Drawer, {optional: true});
    private readonly components = viewChild(SceneComponentInspector);
    private readonly editor = inject(EditorService, {optional: true});
    private readonly viewport = inject(ViewportEditorService, {optional: true});

    protected readonly catalogCollider = computed(() => this.editor?.capabilities()?.component_types?.some(type =>
        type.kind === 'collider') === true);
    protected readonly physicsLayersAvailable = computed(() => this.editor?.capabilities()?.commands?.some(command =>
        command.name === 'set_project') === true);
    protected readonly activationAvailable = computed(() => this.editor?.capabilities()?.commands?.some(command =>
        command.name === 'set_object_active') === true);
    protected readonly activationEditable = computed(() => {
        const selected = this.entity();
        const snapshot = this.snapshot();
        return this.editable() && !!selected && !!snapshot &&
            !linkedInspectorRoot(selected, snapshot.document.entities);
    });
    protected readonly inactiveAncestor = computed(() => {
        const entities = this.snapshot()?.document.entities;
        const selected = this.entity();
        if (!entities || !selected?.parent) return null;
        const lookup = new Map(entities.map(entity => [entity.key, entity]));
        let ancestor = lookup.get(selected.parent);
        for (let depth = 0; ancestor && depth < entities.length; depth++) {
            if (ancestor.active === false) return ancestor.name;
            ancestor = ancestor.parent ? lookup.get(ancestor.parent) : undefined;
        }
        return null;
    });

    protected setActive(input: HTMLInputElement): void {
        const active = input.checked;
        input.checked = this.entity()?.active !== false;
        this.activationRequested.emit(active);
    }

    protected readonly positionLinked = signal(false);
    protected readonly rotationLinked = signal(false);
    protected readonly scaleLinked = signal(false);
    protected readonly drawerConfig = AUTHORING_DRAWER_CONFIG;

    focus(): void {
        this.element.querySelector<HTMLElement>('[data-reference-entity], .placeholder, [aria-label="Selected objects"]')?.focus({preventScroll: true});
    }

    private readonly snapshot = computed(() => this.editor?.snapshot() ?? null);
    private readonly nativeGeneration = computed(() => this.viewport?.state()?.workspace_generation ?? null);
    private readonly scope = computed<WorkspaceAssetScope | null>(() => {
        const snapshot = this.snapshot();
        const nativeGeneration = this.nativeGeneration();
        return this.editor && snapshot && nativeGeneration !== null
            ? {workspaceGeneration: this.editor.workspaceGeneration(), nativeGeneration, revision: snapshot.revision}
            : null;
    });
    private readonly linkedTarget = computed<LinkedInspectorTarget | null>(() => {
        const snapshot = this.snapshot(), object = this.entity(), scope = this.scope();
        if (!snapshot || !object || !scope || snapshot.transaction !== null || snapshot.selection[0] !== object.key) return null;
        const root = linkedInspectorRoot(object, snapshot.document.entities);
        return root ? {scene: snapshot.active_scene_id, object: object.key, root, scope} : null;
    }, {equal: sameLinkedInspectorTarget});
    private readonly linkedAddress = watchLinkedInspectorAddress(this.editor, this.linkedTarget);
    readonly subject = computed<InspectorSubject | null>(() => {
        const snapshot = this.snapshot(), object = this.entity(), scope = this.scope();
        if (!snapshot || !object || !scope || snapshot.selection[0] !== object.key) return null;
        const target = this.linkedTarget();
        if (target) return linkedInspectorSubject(snapshot, target, this.linkedAddress(), this.editable());
        if (snapshot.transaction !== null && linkedInspectorRoot(object, snapshot.document.entities)) return null;
        return sceneInspectorSubject(snapshot, object.key, scope, this.editable());
    });

    // The rendered entity remains stable while its optional semantic linked address loads.
    private readonly selectionIdentity = computed(() => {
        const entity = this.entity();
        return entity ? this.identityFor(entity.key) : null;
    });
    protected readonly selectedEntities = computed(() => {
        const entity = this.entity(), identity = this.selectionIdentity();
        return entity && identity ? [{identity, entity}] : [];
    });
    protected readonly singleSelection = computed(() => this.batchSelection().length > 1 ? [] : this.selectedEntities());
    protected readonly rotationDegrees = computed<Vec3>(() => {
        const radians = this.entity()?.transform.rotation ?? [0, 0, 0];
        return [radians[0] * 180 / Math.PI, radians[1] * 180 / Math.PI, radians[2] * 180 / Math.PI];
    });
    private readonly nameEntity = signal<string | null>(null);
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
        const identity = this.identityFor(key);
        const focus = () => {
            if (this.selectionIdentity() !== identity) return;
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
        };
        afterNextRender(() => {
            if (this.selectionIdentity() !== identity) return;
            const drawer = this.drawer() ?? this.parentDrawer;
            if (drawer && (drawer.collapsed() || !drawer.isExpanded())) {
                drawer.config.update(config => ({...config, collapsed: false, expanded: true}));
                afterNextRender(focus, {injector: this.injector});
            } else focus();
        }, {injector: this.injector});
    }

    private identityFor(object: EntityKey): string {
        const snapshot = this.snapshot();
        if (!snapshot || !this.editor) return JSON.stringify([this.contextKey(), object]);
        return inspectorAddressKey({kind: 'scene', scene: snapshot.active_scene_id, object},
            this.editor.workspaceGeneration(), this.nativeGeneration());
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
