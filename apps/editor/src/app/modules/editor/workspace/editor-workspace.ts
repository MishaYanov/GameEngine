import {Component, computed, effect, inject, input, output, signal, viewChild} from '@angular/core';
import {EDITOR_ACTIONS, type EditorActionId} from '../../../core/editor/editor-actions';
import {resolveEditorShortcut} from '../../../core/editor/editor-keymap';
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
import {SceneSwitchService} from './scene-actions/scene-switch.service';
import {ProjectBuild} from './project-build/project-build';
import {ViewportEditorService} from '../../../core/editor/viewport-editor.service';
import {ViewportAssetDragService} from './viewport-asset-drag.service';
import {ViewportToolbar} from './viewport-toolbar/viewport-toolbar';
import {GameInputManager} from './game-input/game-input-manager';
import {CommandBuilder} from '../../../core/editor/editor.models';

@Component({
    selector: 'editor-workspace',
    providers: [SceneEditingService, ViewportEditorService, ViewportAssetDragService, SceneSwitchService],
    imports: [EditorPane, EditorIcon, SceneViewport, WorkspaceTabs, SceneEntitiesPanel, InspectorPanel, WorkspaceToolbar, AssetsPanel, SceneActions, ProjectBuild, ViewportToolbar, GameInputManager],
    templateUrl: './editor-workspace.html',
    styleUrl: './editor-workspace.scss',
    host: {'(keydown)': 'handleEditorShortcut($event)', 'data-editor-shortcut-scope': 'workspace'},
})
export class EditorWorkspace {
    readonly opening = input(false);
    readonly canOpenProject = input(false);
    readonly openProjectRequested = output<void>();

    protected readonly editorService = inject(EditorService);
    protected readonly sceneEditing = inject(SceneEditingService);
    protected readonly viewportEditor = inject(ViewportEditorService);
    protected readonly assetDrag = inject(ViewportAssetDragService);
    protected readonly sceneSwitch = inject(SceneSwitchService);
    protected readonly workspaceActionBlocked = computed(() => this.opening() || this.sceneSwitch.pending());
    protected readonly hierarchyError = signal<string | null>(null);
    protected readonly sceneContext = computed(() => `${this.editorService.workspaceGeneration()}:${this.editorService.snapshot()?.active_scene_id ?? ''}`);
    protected readonly sceneLabel = computed(() => {
        const snapshot = this.editorService.snapshot();
        return snapshot?.scenes.find(scene => scene.id === snapshot.active_scene_id)?.asset_path.split('/').at(-1) ?? 'Scene';
    });
    protected readonly activeViewKind = signal<WorkspaceViewKind>('scene-viewport');
    protected readonly gameView = computed(() => this.activeViewKind() === 'game-preview');
    protected readonly showingViewport = computed(() => !this.gameView() || this.editorService.playing());
    protected readonly gameKeyboardCaptured = computed(() => {
        const state = this.viewportEditor.state();
        return state?.settings.view === 'game' && state.focused;
    });
    protected readonly sceneProgress = computed(() => {
        const switching = this.sceneSwitch.loading();
        const accepted = this.sceneSwitch.lastSwitch();
        const target = switching ?? accepted;
        if (!target) return null;
        const name = target.path.split('/').at(-1) ?? target.path;
        const viewport = this.editorService.viewport();
        const snapshot = this.editorService.snapshot();
        if (!switching && (!accepted || accepted.generation !== this.editorService.workspaceGeneration() ||
            snapshot?.active_scene_id !== accepted.sceneId || this.gameView() || this.opening() ||
            !viewport?.attached || viewport.error)) return null;

        const nativeGeneration = this.viewportEditor.state()?.workspace_generation;
        const load = viewport?.scene_load;
        const revision = switching ? snapshot?.scene_revision : accepted?.sceneRevision;
        const matching = load && load.scene_id === target.sceneId && snapshot?.active_scene_id === target.sceneId &&
            revision !== undefined && load.scene_revision >= revision &&
            (nativeGeneration == null || load.workspace_generation === nativeGeneration);
        if (matching && load.stage === 'presented' && !switching) return null;
        if (matching && load.stage === 'preparing' && load.total != null && load.total > 0 && load.completed != null) {
            return {name, label: 'Preparing scene objects…', completed: load.completed, total: load.total};
        }
        const label = matching && load.stage === 'rendering' ? 'Presenting the scene…'
            : switching ? 'Switching scene…' : 'Preparing the scene…';
        return {name, label, completed: null, total: null};
    });
    private readonly sceneToolbar = viewChild(ViewportToolbar);
    private readonly workspaceToolbar = viewChild(WorkspaceToolbar);
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

    protected handleEditorShortcut(event: KeyboardEvent): void {
        if (this.workspaceActionBlocked() || this.activeViewKind() !== 'scene-viewport') return;
        const id = resolveEditorShortcut(event);
        if (id && this.dispatchEditorAction(id)) {
            event.preventDefault();
            event.stopPropagation();
        }
    }

    protected handleNativeShortcut(id: EditorActionId): void {
        if (this.workspaceActionBlocked() || this.activeViewKind() !== 'scene-viewport') return;
        const state = this.viewportEditor.state();
        if (state?.settings.view === 'scene' && state.focused && state.gesture === null) this.dispatchEditorAction(id);
    }

    private dispatchEditorAction(id: EditorActionId): boolean | undefined {
        const action = EDITOR_ACTIONS[id];
        return action.target === 'workspace'
            ? this.workspaceToolbar()?.dispatch(action.command)
            : this.sceneToolbar()?.dispatch(typeof action.command === 'function'
                ? action.command(this.viewportEditor.settings()) : action.command);
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
