import {computed, DestroyRef, inject, Injectable, signal} from '@angular/core';
import type {AudioAuditionScope} from '../../../../core/editor/audio-source.models';
import type {ComponentRecord} from '../../../../core/editor/editor-components';
import {EditorService} from '../../../../core/editor/editor.service';
import {ViewportEditorService} from '../../../../core/editor/viewport-editor.service';
import {type InspectorSubject, inspectorAddressKey, linkedInspectorRoot, linkedInspectorSubject, sceneInspectorSubject} from '../inspector-panel/inspector-subject';
import {PropPreviewService} from '../prop-workspace/prop-preview.service';
import {PropWorkspaceService} from '../prop-workspace/prop-workspace.service';
import {SceneEditingService} from '../scene-editing.service';
import {WorkspaceViewService} from '../scene-viewport/workspace-view.service';
import {ScopedPaneRequests, type ScopedPaneTarget} from '../scoped-pane-request';
import {checkedComponentChange, sceneComponentCommand} from './component-actions';
import type {ComponentEditor, ComponentEditIntent} from './component-editor';

export type ComponentWorkspaceKind = 'audio-workspace' | 'behavior-workspace';

interface ComponentTarget extends ScopedPaneTarget {
    readonly view: ComponentWorkspaceKind;
    readonly componentId: string;
    readonly ownerKey: string;
    readonly ownerName: string;
    readonly record: ComponentRecord;
}

function ownerIdentity(subject: InspectorSubject): string {
    return inspectorAddressKey(subject.address, subject.scope.workspaceGeneration, subject.scope.nativeGeneration);
}

function supports(record: ComponentRecord, view: ComponentWorkspaceKind): boolean {
    return view === 'audio-workspace' ? record.data.kind === 'audio_source'
        : record.data.kind === 'script' || record.data.kind === 'native_behavior';
}

/** Binds a reusable component form to the exact Scene object or editable Prop source that opened it. */
@Injectable()
export class ComponentWorkspaceSession {
    private readonly editor = inject(EditorService);
    private readonly viewport = inject(ViewportEditorService);
    private readonly editing = inject(SceneEditingService);
    private readonly props = inject(PropWorkspaceService);
    private readonly propPreview = inject(PropPreviewService);
    private readonly views = inject(WorkspaceViewService);
    private readonly panes = inject(ScopedPaneRequests);
    private readonly opened = signal<ComponentTarget | null>(null);
    private readonly form = signal<ComponentEditor | undefined>(undefined);
    private readonly applying = signal(false);
    private readonly failure = signal<string | null>(null);
    private decision: ((accepted: boolean) => void) | undefined;
    private readonly reviewing = signal(false);
    private readonly acknowledgement = signal(0);

    readonly target = computed(() => {
        const opened = this.opened();
        return opened ? this.resolve(opened.view, opened.subject, opened.componentId, opened.ownerKey) : null;
    });
    readonly record = computed(() => this.target()?.record ?? this.opened()?.record ?? null);
    readonly ownerKey = computed(() => this.opened()?.ownerKey ?? '');
    readonly origin = computed(() => this.opened()?.subject.address.kind === 'prop' ? 'prop' : 'scene');
    readonly description = computed(() => this.opened()?.ownerName ?? '');
    readonly context = computed(() => {
        const opened = this.opened();
        return opened ? JSON.stringify([ownerIdentity(opened.subject), opened.componentId, opened.view]) : '';
    });
    readonly models = computed(() => this.editor.modelAssets() ?? []);
    readonly scriptTypes = computed(() => this.editor.capabilities()?.object_scripts ?? []);
    readonly objectTargets = computed(() => this.origin() === 'prop'
        ? this.props.document()?.objects ?? [] : this.editor.snapshot()?.document.entities ?? []);
    readonly gameActions = computed(() => this.editor.snapshot()?.project.input_profile?.actions?.map(action => action.id) ?? []);
    readonly scriptSlots = computed(() => this.origin() === 'prop' ? Object.keys(this.props.draft()?.prefab.reference_slots ?? {}) : []);
    readonly scriptType = (id: string) => this.scriptTypes().find(type => type.id === id);
    readonly pending = this.applying.asReadonly();
    readonly error = this.failure.asReadonly();
    readonly confirming = this.reviewing.asReadonly();
    readonly acknowledged = this.acknowledgement.asReadonly();
    readonly canSave = computed(() => this.form()?.prepareEdit() != null && this.target() !== null && !this.pending());
    readonly editable = computed(() => {
        const target = this.target();
        if (!target || this.editor.busy() || this.pending()) return false;
        return target.subject.address.kind === 'prop' ? this.props.canEditDraft() : this.editing.editable();
    });
    readonly audioScope = computed<AudioAuditionScope | null>(() => {
        const target = this.target();
        const snapshot = this.editor.snapshot();
        if (!target || target.record.data.kind !== 'audio_source' || !this.views.audioView() ||
            !snapshot || snapshot.transaction) return null;
        const {address, scope, contentRevision} = target.subject;
        if (address.kind === 'linked') return null;
        const base = {generation: scope.workspaceGeneration, nativeGeneration: scope.nativeGeneration,
            revision: scope.revision, contentRevision};
        if (address.kind === 'scene') return {...base,
            owner: {kind: 'scene', scene_id: address.scene, key: address.object},
        };
        const preview = this.propPreview.currentState();
        return preview?.selection === address.sourceObject && preview.workspace_generation === scope.nativeGeneration
            && this.propPreview.ready() ? {...base,
                owner: {kind: 'prop', preview_token: preview.token, key: address.sourceObject},
            } : null;
    });

    constructor() {
        inject(DestroyRef).onDestroy(() => this.finishReview(false));
    }

    available(view: ComponentWorkspaceKind): boolean {
        const opened = this.opened();
        return opened?.view === view && opened.subject.scope.workspaceGeneration === this.editor.workspaceGeneration()
            && (opened.subject.address.kind === 'prop' || opened.subject.address.scene === this.editor.snapshot()?.active_scene_id);
    }

    async open(view: ComponentWorkspaceKind, subject: InspectorSubject, componentId: string): Promise<boolean> {
        const origin = this.views.activeViewKind();
        const target = this.resolve(view, subject, componentId, this.editing.selectedEntity()?.key);
        if (!target) return false;
        return this.panes.open<ComponentTarget>({
            pane: view, subject, componentId, asset: target.asset,
        }, {
            resolve: () => this.views.activeViewKind() === origin ? this.resolve(view, subject, componentId, target.ownerKey) : null,
            load: async resolved => {
                const prepared = this.resolve(view, resolved.subject, componentId, target.ownerKey);
                return prepared && await this.verifyLinked(prepared) ? {value: prepared, asset: prepared.asset} : null;
            },
            activate: prepared => {
                this.failure.set(null);
                this.opened.set(prepared);
                this.acknowledgement.update(value => value + 1);
                this.views.selectView(view, prepared.subject.address.kind === 'prop' ? 'prop' : 'scene');
                return this.views.activeViewKind() === view;
            },
        });
    }

    connectForm(form: ComponentEditor | undefined): () => void {
        this.form.set(form);
        return () => {
            if (this.form() === form) this.form.set(undefined);
        };
    }

    reviewNavigation(): Promise<boolean> {
        if (this.pending() || this.decision) return Promise.resolve(false);
        if (!this.form()?.draftDirty()) return Promise.resolve(true);
        this.reviewing.set(true);
        return new Promise(resolve => { this.decision = resolve; });
    }

    async settleNavigation(action: 'save' | 'discard' | 'cancel'): Promise<void> {
        if (!this.decision || this.pending()) return;
        if (action === 'save') {
            const intent = this.form()?.prepareEdit();
            if (!intent || !await this.apply(intent)) return;
        }
        if (action === 'discard') this.form()?.discardDraft();
        this.finishReview(action !== 'cancel');
    }

    private finishReview(accepted: boolean): void {
        const resolve = this.decision;
        this.decision = undefined;
        this.reviewing.set(false);
        resolve?.(accepted);
    }

    async apply(intent: ComponentEditIntent): Promise<boolean> {
        const opened = this.opened();
        if (!opened || !this.editable() || intent.kind !== 'replace' || intent.component.id !== opened.componentId ||
            intent.component.data.kind !== opened.record.data.kind) return false;
        this.applying.set(true);
        this.failure.set(null);
        try {
            let applied = false;
            const address = opened.subject.address;
            if (address.kind === 'prop') {
                applied = await this.props.editComponent(address.sourceObject, intent);
                if (!applied) throw new Error(this.props.error() ?? 'The Prop component could not be updated.');
            } else {
                const committed = await this.editing.withCommittedEdit(async () => {
                    const target = this.opened() === opened && this.target();
                    if (!target || !await this.verifyLinked(target) ||
                        target.subject.scope.revision !== this.editor.snapshot()?.revision || !this.target()) return;
                    if (!checkedComponentChange([target.record], intent, component => supports(component, opened.view))) return;
                    await this.editor.execute(sceneComponentCommand(target.ownerKey, intent),
                        target.subject.scope.workspaceGeneration, target.subject.scope.revision);
                    applied = true;
                });
                if (!committed) throw new Error(this.editing.error() ?? 'Finish the current Scene edit and try again.');
            }
            if (!applied || this.opened() !== opened) return false;
            this.acknowledgement.update(value => value + 1);
            return true;
        } catch (error) {
            if (this.opened() === opened) this.failure.set(error instanceof Error ? error.message : String(error));
            return false;
        } finally {
            this.applying.set(false);
        }
    }

    private async verifyLinked(target: ComponentTarget): Promise<boolean> {
        const {address, scope} = target.subject;
        if (address.kind !== 'linked') return true;
        const object = target.ownerKey;
        try {
            const result = await this.editor.inspectLinkedPropAddress(object, scope);
            const snapshot = this.editor.snapshot();
            const verified = snapshot && linkedInspectorSubject(snapshot,
                {object, scene: address.scene, root: address.root, scope}, result, true);
            return !!verified && ownerIdentity(verified) === ownerIdentity(target.subject);
        } catch {
            return false;
        }
    }

    private resolve(view: ComponentWorkspaceKind, captured: InspectorSubject, id: string, key?: string): ComponentTarget | null {
        const snapshot = this.editor.snapshot();
        if (!snapshot || snapshot.mode !== 'edit' || this.editor.playing()) return null;
        const {address} = captured;
        let subject: InspectorSubject | null;
        let object: {readonly key: string; readonly name: string};
        let record: ComponentRecord | undefined;
        if (address.kind === 'prop' && captured.access === 'prop-edit') {
            subject = this.props.inspectorSubject();
            const source = this.props.selectedObject();
            if (!source) return null;
            object = source;
            record = source.components.find(component => component.id === id);
        } else if (address.kind === 'scene' && captured.access === 'scene-edit' ||
            address.kind === 'linked' && captured.access === 'linked-overrides' && view === 'behavior-workspace') {
            const selected = this.editing.selectedEntity();
            const nativeGeneration = this.viewport.state()?.workspace_generation;
            if (!selected || nativeGeneration == null) return null;
            const scope = {workspaceGeneration: this.editor.workspaceGeneration(), nativeGeneration, revision: snapshot.revision};
            const root = selected.parent ? linkedInspectorRoot(selected, snapshot.document.entities) : null;
            if (address.kind === 'linked') {
                if (selected.key !== key || root !== address.root || address.scene !== snapshot.active_scene_id) return null;
                subject = {address, scope, contentRevision: snapshot.scene_revision, access: 'linked-overrides'};
            } else {
                if (selected.key !== address.object || root || (view === 'audio-workspace' && selected.linked_prefab)) return null;
                subject = sceneInspectorSubject(snapshot, selected.key, scope, true);
            }
            object = selected;
            record = selected.component_views?.find(component => component.id === id);
        } else return null;
        if (!subject || subject.access !== captured.access || ownerIdentity(subject) !== ownerIdentity(captured) ||
            !record || !supports(record, view) || (address.kind === 'prop' && record.data.kind === 'native_behavior')) return null;
        const clip = record.data.kind === 'audio_source' ? record.data.settings.clip?.asset : undefined;
        const asset = clip ? this.models().find(model => model.asset_id === clip) : undefined;
        return {view, subject, componentId: id, ownerKey: object.key, ownerName: object.name, record,
            asset: clip ? {id: clip, revision: asset?.revision ?? undefined, fingerprint: asset?.fingerprint ?? undefined} : undefined};
    }
}
