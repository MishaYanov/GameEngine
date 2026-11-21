import {Component, computed, DOCUMENT, effect, inject, input, output, signal, viewChild} from '@angular/core';
import {EDITOR_ACTIONS, type EditorActionId} from '../../../core/editor/editor-actions';
import {resolveEditorShortcut} from '../../../core/editor/editor-keymap';
import {EditorPane} from '../../../shared/components/pane/pane';
import {EditorIcon} from '../../../shared/icons/editor-icon';
import {ContextMenuService} from '../../../shared/components/context-menu/context-menu.service';
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
import {GameplaySettings} from './game-settings/gameplay-settings';
import {CameraSettings} from './game-settings/camera-settings';
import {RuntimeInspector} from './runtime-inspector/runtime-inspector';
import {CommandBuilder, type EntityKey} from '../../../core/editor/editor.models';
import {CommandPalette} from './command-palette/command-palette';
import {FindUsages} from './find-usages/find-usages';
import {collectSceneReferences, type SceneReference} from './find-usages/scene-references';
import {editorProblems} from './problems-panel/editor-problems';
import {ProblemsPanel, type EditorProblem, type ProblemLocation} from './problems-panel/problems-panel';

@Component({
    selector: 'editor-workspace',
    providers: [SceneEditingService, ViewportEditorService, ViewportAssetDragService, SceneSwitchService],
    imports: [
        EditorPane, EditorIcon, SceneViewport, WorkspaceTabs, SceneEntitiesPanel, InspectorPanel,
        WorkspaceToolbar, AssetsPanel, SceneActions, ProjectBuild, ViewportToolbar, GameInputManager,
        GameplaySettings, CameraSettings, RuntimeInspector, CommandPalette, FindUsages, ProblemsPanel,
    ],
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
    private readonly document = inject(DOCUMENT);
    private readonly contextMenu = inject(ContextMenuService);
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
    protected readonly gameStatus = this.viewportEditor.gameStatus;
    protected readonly canRestartRun = computed(() =>
        this.gameView() && this.gameStatus() !== null && !this.workspaceActionBlocked() &&
        this.editorService.viewport()?.attached === true && !this.editorService.busy() &&
        this.viewportEditor.pending() === 0
    );
    private readonly restartFailure = signal<{ runId: string; generation: number; message: string } | null>(null);
    protected readonly gameError = computed(() => {
        const status = this.gameStatus();
        if (!status) return null;
        const failure = this.restartFailure();
        return failure?.runId === status.run_id && failure.generation === this.editorService.workspaceGeneration()
            ? failure.message : this.viewportEditor.state()?.error ?? null;
    });
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
    private readonly hierarchy = viewChild(SceneEntitiesPanel);
    private readonly inspector = viewChild(InspectorPanel);
    private readonly cameraSettings = viewChild(CameraSettings);
    private readonly sceneActions = viewChild(SceneActions);
    private readonly projectBuild = viewChild(ProjectBuild);
    protected readonly paletteOpen = signal(false);
    protected readonly problemsOpen = signal(false);
    protected readonly problemsActive = signal(false);
    private readonly usage = signal<{ key: EntityKey; context: string } | null>(null);
    protected readonly usageTarget = computed(() => {
        const usage = this.usage();
        if (!usage || usage.context !== this.sceneContext()) return null;
        return this.editorService.document()?.entities.find(entity => entity.key === usage.key) ?? null;
    });
    protected readonly sceneReferences = computed(() => {
        if (!this.usageTarget() && !this.problemsActive()) return [];
        const document = this.editorService.document();
        return document ? collectSceneReferences(document) : [];
    });
    protected readonly problems = computed(() => editorProblems(
        this.sceneReferences(), this.projectBuild()?.exportFailure() ?? null,
    ));
    protected readonly canRevealReference = computed(() => !this.workspaceActionBlocked() && this.sceneEditing.ready());
    protected readonly canOpenPalette = computed(() => !!this.editorService.snapshot() &&
        !this.workspaceActionBlocked() && !this.viewportEditor.state()?.gesture && this.contextMenu.request() === null);
    protected readonly canRunPaletteCommand = (id: EditorActionId): boolean => {
        if (!this.canOpenPalette()) return false;
        const action = EDITOR_ACTIONS[id];
        switch (action.target) {
            case 'editor':
                return false;
            case 'workspace':
                return this.workspaceToolbar()?.canPerform(action.command) === true;
            case 'viewport':
                return this.sceneToolbar()?.canPerform(typeof action.command === 'function'
                    ? action.command(this.viewportEditor.settings()) : action.command) === true;
        }
    };
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
        effect(() => {
            this.sceneContext();
            this.hierarchyError.set(null);
            this.usage.set(null);
            this.paletteOpen.set(false);
            this.problemsOpen.set(false);
            this.problemsActive.set(false);
        });
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

    protected async restartRun(): Promise<void> {
        const snapshot = this.editorService.snapshot();
        const status = this.gameStatus();
        if (!snapshot || !status || !this.canRestartRun()) return;
        const generation = this.editorService.workspaceGeneration();
        this.restartFailure.set(null);
        try {
            await this.viewportEditor.action({
                type: 'restart_run', run_id: status.run_id, expected_revision: snapshot.revision,
            });
        } catch (error) {
            if (this.editorService.workspaceGeneration() === generation &&
                this.gameStatus()?.run_id === status.run_id) {
                this.restartFailure.set({
                    runId: status.run_id,
                    generation,
                    message: error instanceof Error ? error.message : String(error),
                });
            }
        }
    }

    protected handleEditorShortcut(event: KeyboardEvent): void {
        if (this.workspaceActionBlocked()) return;
        const id = resolveEditorShortcut(event);
        if (id && (id === 'palette.open' || this.activeViewKind() === 'scene-viewport') && this.dispatchEditorAction(id)) {
            event.preventDefault();
            event.stopPropagation();
        }
    }

    protected handleNativeShortcut(id: EditorActionId): void {
        if (this.workspaceActionBlocked() || this.activeViewKind() !== 'scene-viewport') return;
        const state = this.viewportEditor.state();
        if (state?.settings.view === 'scene' && state.focused && state.gesture === null) this.dispatchEditorAction(id);
    }

    protected dispatchEditorAction(id: EditorActionId): boolean | undefined {
        const action = EDITOR_ACTIONS[id];
        if (action.target === 'editor') return this.openPalette();
        return action.target === 'workspace'
            ? this.workspaceToolbar()?.dispatch(action.command)
            : this.sceneToolbar()?.dispatch(typeof action.command === 'function'
                ? action.command(this.viewportEditor.settings()) : action.command);
    }

    protected openPalette(): boolean {
        if (!this.canOpenPalette() || this.document.querySelector('dialog[open]')) return false;
        this.paletteOpen.set(true);
        return true;
    }

    protected showUsages(key: EntityKey): void {
        if (this.canRevealReference()) this.usage.set({key, context: this.sceneContext()});
    }

    protected closeUsages(): void {
        this.usage.set(null);
    }

    protected revealUsage(reference: SceneReference): void {
        void this.revealReference(reference.location, reference.field);
    }

    protected openProblems(): void {
        this.problemsActive.set(true);
        this.problemsOpen.set(true);
    }

    protected revealProblem(problem: EditorProblem): void {
        void this.revealReference(problem.location, problem.field);
    }

    private async revealReference(location: ProblemLocation, field?: string): Promise<void> {
        if (!this.canRevealReference()) return;
        switch (location.kind) {
            case 'build':
                this.projectBuild()?.revealExportFailure();
                return;
            case 'camera':
                this.cameraSettings()?.show(true, location.mode);
                return;
            case 'binding':
                this.sceneActions()?.revealBinding(location.name);
                return;
            case 'entity': {
                const context = this.sceneContext();
                await this.sceneEditing.select(location.key);
                if (context !== this.sceneContext() || this.sceneEditing.entity()?.key !== location.key) return;
                if (location.section === 'hierarchy') this.hierarchy()?.revealEntity(location.key);
                else this.inspector()?.revealReference(location.key, location.componentId, location.section, location.slot, field);
            }
        }
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
