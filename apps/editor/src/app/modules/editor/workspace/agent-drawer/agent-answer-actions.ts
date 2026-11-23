import {inject, Injectable, signal} from '@angular/core';
import {invoke} from '@tauri-apps/api/core';
import type {ComponentRecord} from '../../../../core/editor/editor-components';
import {EditorService} from '../../../../core/editor/editor.service';
import type {AnimationPaneRequest} from '../animation/animation-pane-request';
import {AnimationWorkspaceService} from '../animation/animation-workspace.service';
import {ComponentWorkspaceSession} from '../components/component-workspace-session';
import {inspectorAddressKey, type InspectorAddress, type InspectorSubject} from '../inspector-panel/inspector-subject';
import {PropPreviewService} from '../prop-workspace/prop-preview.service';
import {PropWorkspaceService} from '../prop-workspace/prop-workspace.service';
import {SceneEditingService} from '../scene-editing.service';
import {WorkspaceViewService} from '../scene-viewport/workspace-view.service';
import {WorkspaceDraftReview} from '../workspace-draft-review';
import {AgentSelectionContextService, type AgentSelectionContext} from './agent-selection-context';

export type AgentAnswerAction =
    | {readonly kind: 'show-object'; readonly target: AgentSelectionContext}
    | {readonly kind: 'open-component-editor'; readonly target: AgentSelectionContext; readonly componentId: string};

interface AgentObjectSummary {
    readonly address: InspectorAddress;
    readonly entity_key: string;
    readonly workspace_generation: number;
    readonly content_revision: number;
    readonly revision: number;
    readonly selected_component?: ComponentRecord;
}

@Injectable()
export class AgentAnswerActions {
    private readonly editor = inject(EditorService);
    private readonly context = inject(AgentSelectionContextService);
    private readonly drafts = inject(WorkspaceDraftReview);
    private readonly editing = inject(SceneEditingService);
    private readonly views = inject(WorkspaceViewService);
    private readonly prop = inject(PropWorkspaceService);
    private readonly propPreview = inject(PropPreviewService);
    private readonly animation = inject(AnimationWorkspaceService);
    private readonly components = inject(ComponentWorkspaceSession);
    private reveal: ((key: string, componentId?: string) => void) | null = null;
    readonly notice = signal<string | null>(null);
    readonly pending = signal(false);

    setReveal(reveal: (key: string, componentId?: string) => void): void {
        this.reveal = reveal;
    }

    async run(action: AgentAnswerAction, refresh = false): Promise<boolean> {
        if (this.pending()) return false;
        this.pending.set(true);
        this.notice.set(null);
        try {
            return await this.open(action, refresh);
        } catch (error) {
            this.notice.set(typeof error === 'object' && error && 'message' in error && typeof error.message === 'string'
                ? error.message : 'The addressed object could not be opened.');
            return false;
        } finally {
            this.pending.set(false);
        }
    }

    private async open(action: AgentAnswerAction, refresh: boolean): Promise<boolean> {
        const {target} = action;
        const componentId = action.kind === 'open-component-editor' ? action.componentId : undefined;
        const reviewScope = this.drafts.capture();
        let object: AgentObjectSummary | null = null;
        const reviewed = await this.drafts.run(reviewScope, async () => {
            const resolved = await this.resolve(target, componentId, refresh);
            if (!this.drafts.current(reviewScope) || !this.context.canDeliver(target)) return;
            if (resolved.address.kind === 'prop') {
                const doc = this.prop.document();
                if (doc?.kind !== 'source' || doc.draft.draft_id !== resolved.address.draftId) return;
                this.views.selectView('prop-workspace');
                this.prop.selectObject(resolved.entity_key);
                await this.propPreview.select(resolved.entity_key);
                if (this.prop.selectedObject()?.key !== resolved.entity_key) return;
            } else {
                this.views.selectView('scene-viewport');
                await this.editing.select(resolved.entity_key);
                if (this.editor.snapshot()?.selection[0] !== resolved.entity_key) return;
            }
            if (this.drafts.current(reviewScope)) object = resolved;
        });
        if (!reviewed) return false;
        const resolved = object as AgentObjectSummary | null;
        if (!resolved || !this.context.canDeliver(target)) return false;
        if (!componentId) {
            this.reveal?.(resolved.entity_key);
            return true;
        }

        const snapshot = this.editor.snapshot();
        if (!snapshot) return false;
        const subject: InspectorSubject = {...target.subject,
            scope: {...target.subject.scope, revision: snapshot.revision}, contentRevision: resolved.content_revision};
        const record = resolved.selected_component;
        if (record?.data.kind === 'animation') {
            const {source, playback} = record.data.settings;
            const request: AnimationPaneRequest = {
                library: 'imported', subject, componentId, asset: {id: source.asset_id, fingerprint: source.expected_sha256},
                clipIndex: playback.kind === 'single_clip' ? playback.clip_index : playback.idle,
            };
            return this.animation.openScoped(request, () => this.stillSelected(subject, resolved.entity_key) ? request : null);
        }
        if (record?.data.kind === 'audio_source') {
            return this.components.open('audio-workspace', subject, componentId);
        }
        if (record?.data.kind === 'script' || record?.data.kind === 'native_behavior') {
            return this.components.open('behavior-workspace', subject, componentId);
        }
        this.reveal?.(resolved.entity_key, componentId);
        return true;
    }

    private stillSelected(subject: InspectorSubject, key: string): boolean {
        const snapshot = this.editor.snapshot();
        if (!snapshot || snapshot.revision !== subject.scope.revision ||
            this.editor.workspaceGeneration() !== subject.scope.workspaceGeneration) return false;
        if (subject.address.kind !== 'prop') return snapshot.selection[0] === key &&
            snapshot.active_scene_id === subject.address.scene && snapshot.scene_revision === subject.contentRevision;
        const address = this.prop.inspectorSubject();
        return address?.address.kind === 'prop' && address.address.draftId === subject.address.draftId &&
            address.address.sourceObject === key && address.contentRevision === subject.contentRevision;
    }

    private async resolve(target: AgentSelectionContext, componentId: string | undefined, refresh: boolean): Promise<AgentObjectSummary> {
        const {subject} = target;
        const {scope, address} = subject;
        const snapshot = this.editor.snapshot();
        if (!snapshot || !this.context.canDeliver(target) || scope.nativeGeneration === null) {
            throw new Error('This answer belongs to an earlier project session.');
        }
        const document = this.prop.document();
        const contentRevision = address.kind !== 'prop' ? snapshot.scene_revision :
            document?.kind === 'source' && document.draft.draft_id === address.draftId ? document.draft.draft_revision : undefined;
        if (contentRevision === undefined || address.kind !== 'prop' && address.scene !== snapshot.active_scene_id) {
            throw new Error('Open the scene or Prop from this answer and try again.');
        }
        if (!refresh && contentRevision !== subject.contentRevision) throw new Error('The object may have changed. Refresh Target to check its current address.');
        const object = await invoke<AgentObjectSummary>('editor_agent_context', {request: {
            workspace_generation: scope.nativeGeneration, revision: snapshot.revision, content_revision: contentRevision,
            address, ...(componentId ? {component_id: componentId} : {}),
        }});
        const key = (value: InspectorAddress) => inspectorAddressKey(value, scope.workspaceGeneration, scope.nativeGeneration);
        if (!this.context.canDeliver(target) || object.workspace_generation !== scope.nativeGeneration ||
            object.revision !== this.editor.snapshot()?.revision || object.content_revision !== contentRevision ||
            key(object.address) !== key(address) || componentId && object.selected_component?.id !== componentId) {
            throw new Error('The addressed object or attachment changed. Refresh Target and try again.');
        }
        return object;
    }
}
