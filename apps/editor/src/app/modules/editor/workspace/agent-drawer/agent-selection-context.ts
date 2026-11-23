import {computed, inject, Injectable, linkedSignal, signal} from '@angular/core';
import type {ComponentRecord} from '../../../../core/editor/editor-components';
import type {EntityKey, SceneEntity} from '../../../../core/editor/editor.models';
import {EditorService, type WorkspaceAssetScope} from '../../../../core/editor/editor.service';
import {ViewportEditorService} from '../../../../core/editor/viewport-editor.service';
import {ComponentWorkspaceSession} from '../components/component-workspace-session';
import {
    inspectorAddressKey, type InspectorSubject, linkedInspectorRoot, linkedInspectorSubject, type LinkedInspectorTarget,
    sameLinkedInspectorTarget, sceneInspectorSubject, sourceInspectorSubject, watchLinkedInspectorAddress,
} from '../inspector-panel/inspector-subject';
import {PropWorkspaceService} from '../prop-workspace/prop-workspace.service';
import {WorkspaceViewService} from '../scene-viewport/workspace-view.service';

export interface AgentSelectionContext {
    readonly subject: InspectorSubject;
    readonly label: string;
    readonly componentId?: string;
}

interface Candidate {
    readonly objectKey: EntityKey;
    readonly context: AgentSelectionContext;
    readonly componentIds: readonly string[];
}

interface Pin {
    readonly objectKey: EntityKey;
    readonly context: AgentSelectionContext;
}

function candidate(
    subject: InspectorSubject | null, object: {readonly key: EntityKey; readonly name: string} | undefined,
    components: readonly ComponentRecord[] | undefined, legacyIds: Readonly<Record<string, string>> = {},
): Candidate | null {
    if (!subject || !object) return null;
    return {objectKey: object.key, context: {subject, label: object.name},
        componentIds: components?.map(record => record.id) ?? Object.values(legacyIds)};
}

function sceneCandidate(subject: InspectorSubject | null, object: SceneEntity | undefined): Candidate | null {
    return candidate(subject, object, object?.component_views, object?.component_ids);
}

function identity(subject: InspectorSubject): string {
    return inspectorAddressKey(subject.address, subject.scope.workspaceGeneration, subject.scope.nativeGeneration);
}

/** Supplies request captures from the Inspector's selected target, or from one exact pinned target. */
@Injectable()
export class AgentSelectionContextService {
    private readonly editor = inject(EditorService);
    private readonly viewport = inject(ViewportEditorService);
    private readonly prop = inject(PropWorkspaceService);
    private readonly views = inject(WorkspaceViewService);
    private readonly componentWorkspace = inject(ComponentWorkspaceSession, {optional: true});
    private readonly sceneSubject = signal<() => InspectorSubject | null>(() => null);
    private readonly pin = linkedSignal({
        source: this.editor.workspaceGeneration,
        computation: (): Pin | null => null,
    });
    private readonly nativeGeneration = computed(() => {
        const document = this.prop.document();
        const draftGeneration = document?.kind === 'source' &&
            document.scope.generation === this.editor.workspaceGeneration() ? document.draft.workspace_generation : null;
        return this.viewport.state()?.workspace_generation ?? draftGeneration;
    });
    private readonly scope = computed<WorkspaceAssetScope | null>(() => {
        const snapshot = this.editor.snapshot(), nativeGeneration = this.nativeGeneration();
        return snapshot?.mode === 'edit' && snapshot.transaction === null && nativeGeneration !== null ?
            {workspaceGeneration: this.editor.workspaceGeneration(), nativeGeneration, revision: snapshot.revision} : null;
    });
    private readonly live = computed<Candidate | null>(() => {
        const snapshot = this.editor.snapshot(), scope = this.scope();
        if (!snapshot || !scope) return null;
        if (this.views.audioView() || this.views.behaviorView()) {
            const target = this.componentWorkspace?.target();
            if (!target || !this.currentScope(target.subject, scope)) return null;
            const result = target.subject.address.kind === 'prop' ?
                this.propCandidate(target.subject) :
                sceneCandidate(target.subject, snapshot.document.entities.find(entity => entity.key === target.ownerKey));
            return result ? {...result, context: {...result.context, componentId: target.componentId}} : null;
        }
        const inProp = this.views.propView() || (this.views.animationView() && this.views.animationOrigin() === 'prop');
        if (inProp) {
            const subject = this.prop.inspectorSubject(), object = this.prop.selectedObject();
            return subject && this.currentScope(subject, scope) ?
                candidate(subject, object ?? undefined, object?.components) : null;
        }
        const subject = this.sceneSubject()();
        const object = snapshot.document.entities.find(entity => entity.key === snapshot.selection[0]);
        if (!subject || subject.address.kind === 'prop' || subject.address.scene !== snapshot.active_scene_id ||
            (subject.address.kind === 'scene' && subject.address.object !== object?.key) ||
            !this.currentScope(subject, scope)) return null;
        return sceneCandidate(subject, object);
    });
    private readonly pinnedLinkedTarget = computed<LinkedInspectorTarget | null>(() => {
        const pin = this.pin(), scope = this.scope(), snapshot = this.editor.snapshot();
        const address = pin?.context.subject.address;
        if (address?.kind !== 'linked' || !pin || !scope || !snapshot ||
            !this.canDeliver(pin.context) || snapshot.active_scene_id !== address.scene) return null;
        const object = snapshot.document.entities.find(entity => entity.key === pin.objectKey);
        return object && linkedInspectorRoot(object, snapshot.document.entities) === address.root ?
            {scene: address.scene, root: address.root, object: object.key, scope} : null;
    }, {equal: sameLinkedInspectorTarget});
    private readonly pinnedLinkedAddress = watchLinkedInspectorAddress(this.editor, this.pinnedLinkedTarget);
    private readonly resolvedPin = computed<Candidate | null>(() => {
        const pin = this.pin(), scope = this.scope(), snapshot = this.editor.snapshot();
        if (!pin || !scope || !snapshot || !this.canDeliver(pin.context)) return null;
        const original = pin.context.subject;
        const address = original.address;
        if (address.kind === 'prop') {
            const document = this.prop.document();
            if (document?.kind !== 'source' || document.draft.draft_id !== address.draftId) return null;
            const object = document.objects.find(item => item.key === address.sourceObject);
            return candidate(sourceInspectorSubject(document.draft, address.sourceObject, scope, this.prop.canEditDraft()),
                object, object?.components);
        }
        if (snapshot.active_scene_id !== address.scene) return null;
        const object = snapshot.document.entities.find(item => item.key === pin.objectKey);
        if (!object) return null;
        if (address.kind === 'scene') {
            if (linkedInspectorRoot(object, snapshot.document.entities)) return null;
            return sceneCandidate(sceneInspectorSubject(snapshot, object.key, scope, original.access === 'scene-edit'), object);
        }
        const current = this.live();
        if (current && identity(current.context.subject) === identity(original)) return current;
        const target = this.pinnedLinkedTarget();
        const subject = target ? linkedInspectorSubject(snapshot, target, this.pinnedLinkedAddress(),
            original.access === 'linked-overrides') : null;
        return subject && identity(subject) === identity(original) ? sceneCandidate(subject, object) : null;
    });
    private readonly selected = computed(() => this.pin() ? this.resolvedPin() : this.live());
    readonly context = computed(() => this.selected()?.context ?? null);
    readonly componentIds = computed(() => this.selected()?.componentIds ?? []);
    readonly pinned = computed(() => this.pin() !== null);
    readonly pinnedLabel = computed(() => this.pin()?.context.label ?? null);

    setSceneSubject(read: () => InspectorSubject | null): void {
        this.sceneSubject.set(read);
    }

    togglePin(): void {
        if (this.pin()) {
            this.pin.set(null);
            return;
        }
        const live = this.live();
        if (live) this.pin.set({objectKey: live.objectKey, context: structuredClone(live.context)});
    }

    capture(componentId?: string): AgentSelectionContext | null {
        const current = this.selected();
        if (!current || (componentId !== undefined && !current.componentIds.includes(componentId))) return null;
        return structuredClone(componentId === undefined ? current.context : {...current.context, componentId});
    }

    canDeliver(capture: AgentSelectionContext): boolean {
        const {scope} = capture.subject;
        return this.editor.snapshot() !== null && scope.workspaceGeneration === this.editor.workspaceGeneration() &&
            scope.nativeGeneration === this.nativeGeneration();
    }

    private propCandidate(subject: InspectorSubject): Candidate | null {
        const address = subject.address, document = this.prop.document();
        if (address.kind !== 'prop' || document?.kind !== 'source' || document.draft.draft_id !== address.draftId) return null;
        const object = document.objects.find(item => item.key === address.sourceObject);
        return candidate(subject, object, object?.components);
    }

    private currentScope(subject: InspectorSubject, scope: WorkspaceAssetScope): boolean {
        return subject.scope.workspaceGeneration === scope.workspaceGeneration &&
            subject.scope.nativeGeneration === scope.nativeGeneration && subject.scope.revision === scope.revision;
    }
}
