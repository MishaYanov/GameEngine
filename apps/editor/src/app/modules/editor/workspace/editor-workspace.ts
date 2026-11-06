import {Component, computed, effect, inject, input, output, signal} from '@angular/core';
import {EditorPane} from '../../../shared/components/pane/pane';
import {EditorIcon} from '../../../shared/icons/editor-icon';
import {EditorService} from "../../../core/editor/editor.service";
import type {WorkspaceViewKind} from '../../../core/editor/workspace/workspace-view';
import {SceneViewport} from "./scene-viewport/scene-viewport";
import {WorkspaceTabs} from "./workspace-tabs/workspace-tabs";
import type {HierarchyReparentRequest} from './scene-entities-panel/scene-entities-panel';
import {SceneEntitiesPanel} from "./scene-entities-panel/scene-entities-panel";
import {InspectorPanel} from './inspector-panel/inspector-panel';
import {WorkspaceToolbar} from './workspace-toolbar/workspace-toolbar';
import {AssetsPanel} from './assets-panel/assets-panel';
import type {SceneEntitiesModel} from './scene-entities-panel/scene-entities.model';
import {SceneEditingService} from './scene-editing.service';
import {SceneActions} from './scene-actions/scene-actions';
import {ProjectBuild} from './project-build/project-build';
import {ViewportEditorService} from '../../../core/editor/viewport-editor.service';
import {ViewportAssetDragService} from './viewport-asset-drag.service';
import {ViewportToolbar} from './viewport-toolbar/viewport-toolbar';
import {CommandBuilder} from '../../../core/editor/editor.models';

@Component({
    selector: 'editor-workspace',
    providers: [SceneEditingService, ViewportEditorService, ViewportAssetDragService],
    imports: [EditorPane, EditorIcon, SceneViewport, WorkspaceTabs, SceneEntitiesPanel, InspectorPanel, WorkspaceToolbar, AssetsPanel, SceneActions, ProjectBuild, ViewportToolbar],
    templateUrl: './editor-workspace.html',
    styleUrl: './editor-workspace.scss',
})
export class EditorWorkspace {
    readonly opening = input(false);
    readonly canOpenProject = input(false);
    readonly openProjectRequested = output<void>();

    protected readonly editorService = inject(EditorService);
    protected readonly sceneEditing = inject(SceneEditingService);
    protected readonly viewportEditor = inject(ViewportEditorService);
    protected readonly assetDrag = inject(ViewportAssetDragService);
    protected readonly hierarchyError = signal<string | null>(null);
    protected readonly sceneContext = computed(() => `${this.editorService.workspaceGeneration()}:${this.editorService.snapshot()?.active_scene_id ?? ''}`);
    protected readonly sceneLabel = computed(() => {
        const snapshot = this.editorService.snapshot();
        return snapshot?.scenes.find(scene => scene.id === snapshot.active_scene_id)?.asset_path.split('/').at(-1) ?? 'Scene';
    });
    protected readonly activeViewKind = signal<WorkspaceViewKind>('scene-viewport');
    protected readonly gameView = computed(() => this.activeViewKind() === 'game-preview');
    protected readonly showingViewport = computed(() => !this.gameView() || this.editorService.playing());
    private requestedView: 'scene' | 'game' | null = null;
    protected readonly sceneEntities = computed<SceneEntitiesModel>(() => {
        const snapshot = this.editorService.snapshot();

        return {
            projectOpen: snapshot !== null,
            entities: snapshot?.document.entities ?? [],
        };
    });

    constructor() {
        effect(() => this.activeViewKind.set(this.editorService.playing() ? 'game-preview' : 'scene-viewport'));
        effect(() => {this.sceneContext(); this.hierarchyError.set(null);});
        effect(() => this.sceneEditing.blocked.set(this.opening() || this.viewportEditor.authoringGesture()));
        effect(() => {
            const state = this.viewportEditor.state();
            const view = this.gameView() ? 'game' : 'scene';
            if (!state || state.settings.view === view || this.requestedView === view) return;
            this.requestedView = view;
            void this.viewportEditor.action({type: 'set_view', view}).catch(() => undefined).finally(() => {
                if (this.requestedView === view) this.requestedView = null;
            });
        });
    }

    protected selectView(kind: WorkspaceViewKind): void {
        this.activeViewKind.set(kind);
    }

    protected async reparentEntity(request: HierarchyReparentRequest): Promise<void> {
        const context = this.sceneContext();
        this.hierarchyError.set(null);
        const accepted = await this.sceneEditing.withCommittedEdit(async () => {
            if (context !== this.sceneContext()) return;
            await this.editorService.execute(CommandBuilder.reparent(request.key, request.parent, true));
        });
        if (!accepted && context === this.sceneContext()) this.hierarchyError.set(this.sceneEditing.error() ?? 'Finish the current edit and try changing the parent again.');
    }

}
