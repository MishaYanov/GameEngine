import {afterNextRender, Component, computed, DOCUMENT, effect, inject, Injector, input, output, signal, viewChild} from '@angular/core';
import {EDITOR_ACTIONS, type EditorActionId} from '../../../core/editor/editor-actions';
import {resolveEditorShortcut} from '../../../core/editor/editor-keymap';
import {EditorPane} from '../../../shared/components/pane/pane';
import {EditorIcon} from '../../../shared/icons/editor-icon';
import {EditorDialog} from '../../../shared/components/dialog/dialog';
import {ContextMenuService} from '../../../shared/components/context-menu/context-menu.service';
import {EditorService} from "../../../core/editor/editor.service";
import {SceneViewport} from "./scene-viewport/scene-viewport";
import {WorkspaceViewService} from './scene-viewport/workspace-view.service';
import {WorkspaceTabs} from "./workspace-tabs/workspace-tabs";
import {AnimationWorkspace} from './animation/animation-workspace';
import {AnimatorWorkspace} from './animator/animator-workspace';
import {ControlGraphWorkspace} from './control-graph/control-graph-workspace';
import {ControlGraphWorkspaceService} from './control-graph/control-graph-workspace.service';
import {AnimatorAssets} from './animator/animator-assets.service';
import {AnimatorWorkspaceService} from './animator/animator-workspace.service';
import {AnimationWorkspaceService} from './animation/animation-workspace.service';
import {PropWorkspace} from './prop-workspace/prop-workspace';
import {PropWorkspaceService} from './prop-workspace/prop-workspace.service';
import {PropPreviewService} from './prop-workspace/prop-preview.service';
import type {HierarchyCreateCommand, HierarchyReparentRequest} from './scene-entities-panel/scene-entities-panel';
import {SceneEntitiesPanel} from "./scene-entities-panel/scene-entities-panel";
import {InspectorPanel} from './inspector-panel/inspector-panel';
import {WorkspaceToolbar} from './workspace-toolbar/workspace-toolbar';
import {AssetsPanel} from './assets-panel/assets-panel';
import {PropAssetSelection} from './prefabs/prop-asset-selection';
import type {SceneEntitiesModel} from './scene-entities-panel/scene-entities.model';
import type {HierarchyFolderCommand} from './scene-entities-panel/hierarchy-folder-editor';
import {SceneEditingService} from './scene-editing.service';
import {SceneActions} from './scene-actions/scene-actions';
import {SceneSwitchService} from './scene-actions/scene-switch.service';
import {sceneSwitchProgress} from './scene-actions/scene-switch-progress';
import {ProjectBuild} from './project-build/project-build';
import {ViewportEditorService} from '../../../core/editor/viewport-editor.service';
import {ViewportAssetDragService} from './viewport-asset-drag.service';
import {ViewportToolbar} from './viewport-toolbar/viewport-toolbar';
import {GameInputManager} from './game-input/game-input-manager';
import {InputActionsCatalog} from './game-input/input-actions-catalog.service';
import {CameraSettings} from './game-settings/camera-settings';
import {SelectedCameraPreviewService} from './game-settings/selected-camera-preview.service';
import {RuntimeInspector} from './runtime-inspector/runtime-inspector';
import {CommandBuilder, type EntityKey} from '../../../core/editor/editor.models';
import type {SceneDependencyRequest} from '../../../core/editor/scene-verification.models';
import {CommandPalette} from './command-palette/command-palette';
import {FindUsages} from './find-usages/find-usages';
import {collectSceneReferences, type SceneReference} from './find-usages/scene-references';
import {editorProblems} from './problems-panel/editor-problems';
import {type EditorProblem, type ProblemLocation, ProblemsPanel} from './problems-panel/problems-panel';

@Component({
    selector: 'editor-workspace',
    providers: [
        SceneEditingService, ViewportEditorService, ViewportAssetDragService,
        SceneSwitchService, WorkspaceViewService, PropAssetSelection, PropWorkspaceService, PropPreviewService, AnimationWorkspaceService,
        SelectedCameraPreviewService, InputActionsCatalog,
        AnimatorAssets, AnimatorWorkspaceService, ControlGraphWorkspaceService,
    ],
    imports: [
        EditorPane, EditorIcon, SceneViewport, WorkspaceTabs, SceneEntitiesPanel, InspectorPanel,
        WorkspaceToolbar, AssetsPanel, SceneActions, ProjectBuild, ViewportToolbar, GameInputManager,
        CameraSettings, RuntimeInspector, CommandPalette, FindUsages, ProblemsPanel, PropWorkspace, AnimationWorkspace, AnimatorWorkspace, ControlGraphWorkspace, EditorDialog,
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
    protected readonly workspaceView = inject(WorkspaceViewService);
    protected readonly propWorkspace = inject(PropWorkspaceService);
    protected readonly propPreview = inject(PropPreviewService);
    protected readonly animationPreview = inject(AnimationWorkspaceService);
    protected readonly animatorAssets = inject(AnimatorAssets);
    protected readonly animatorWorkspace = inject(AnimatorWorkspaceService);
    protected readonly controlsGraph = inject(ControlGraphWorkspaceService);
    protected readonly previewingCamera = computed(() => this.cameraSettings()?.previewing() ?? false);
    protected readonly selectedCameraPreview = inject(SelectedCameraPreviewService);
    protected readonly missingGameCamera = computed(() => {
        const document = this.editorService.document();
        return !!document?.entities.some(entity => !!entity.camera) &&
            !document.entities.some(entity => entity.key === document.default_game_camera && entity.camera?.enabled);
    });
    private readonly document = inject(DOCUMENT);
    private readonly injector = inject(Injector);
    private readonly contextMenu = inject(ContextMenuService);
    protected readonly workspaceActionBlocked = computed(() => this.opening() || this.sceneSwitch.pending());
    protected readonly hierarchyError = signal<string | null>(null);
    protected readonly hierarchyCreation = computed(() => (['create_camera', 'create_player', 'create_audio_source', 'create_sky'] as const).filter(
        type => this.editorService.capabilities()?.commands?.some(command => command.name === type),
    ));
    protected readonly batchParentingEnabled = computed(() =>
        this.editorService.capabilities()?.commands?.some(command => command.name === 'reparent_objects') ?? false,
    );
    protected readonly hierarchyFoldersEnabled = computed(() => {
        const names = new Set(this.editorService.capabilities()?.commands?.map(command => command.name));
        return (['create_folder', 'rename_folder', 'move_folder', 'move_roots_to_folder', 'remove_folder'] as const)
            .every(name => names.has(name));
    });
    protected readonly sceneContext = computed(() => `${this.editorService.workspaceGeneration()}:${this.editorService.snapshot()?.active_scene_id ?? ''}`);
    protected readonly sceneLabel = computed(() => {
        const snapshot = this.editorService.snapshot();
        return snapshot?.scenes.find(scene => scene.id === snapshot.active_scene_id)?.asset_path.split('/').at(-1) ?? 'Scene';
    });
    protected readonly showingViewport = computed(() =>
        this.workspaceView.animatorView() || this.workspaceView.controlsGraphView() ? false :
        this.workspaceView.animationView() ? this.animationPreview.ready() :
        this.workspaceView.propView() ? this.propPreview.ready() :
            this.previewingCamera() || !this.workspaceView.gameView() || this.editorService.playing()
    );
    private readonly sceneSwitch = inject(SceneSwitchService);
    protected readonly sceneProgress = computed(() => {
        const switching = this.sceneSwitch.loading();
        const accepted = this.sceneSwitch.lastSwitch();
        if (!switching && !accepted) return null;

        return sceneSwitchProgress({
            switching,
            accepted,
            snapshot: this.editorService.snapshot(),
            viewport: this.editorService.viewport(),
            generation: this.editorService.workspaceGeneration(),
            nativeGeneration: this.viewportEditor.state()?.workspace_generation,
            gameView: this.workspaceView.gameView(),
            opening: this.opening(),
        });
    });
    private readonly sceneToolbar = viewChild(ViewportToolbar);
    private readonly workspaceTabs = viewChild(WorkspaceTabs);
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
    protected readonly sceneEntities = computed<SceneEntitiesModel>(() => {
        const snapshot = this.editorService.snapshot();

        return {
            projectOpen: snapshot !== null,
            entities: snapshot?.document.entities ?? [],
            organization: snapshot?.document.organization,
        };
    });

    constructor() {
        let previousView = this.workspaceView.activeViewKind();
        effect(() => {
            const next = this.workspaceView.activeViewKind();
            const previous = previousView;
            previousView = next;
            if (previous === next) return;
            afterNextRender(() => {
                if (this.workspaceView.activeViewKind() === next && this.document.activeElement === this.document.body) {
                    this.workspaceTabs()?.focusTab(next);
                }
            }, {injector: this.injector});
        });
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
            if (this.workspaceActionBlocked()) this.propWorkspace.cancelOpening();
        });
    }

    protected handleEditorShortcut(event: KeyboardEvent): void {
        if (this.workspaceActionBlocked()) return;
        const id = resolveEditorShortcut(event);
        if (this.workspaceView.propView() && !this.propWorkspace.confirmingClose()) {
            if (event.defaultPrevented) return;
            const closesProp = event.key === 'Escape' && event.target instanceof Element &&
                event.target.closest('[aria-label="Prop Workspace"]') !== null;
            if (closesProp || id && this.runPropAction(id)) {
                event.preventDefault();
                event.stopPropagation();
                if (closesProp) void this.closePropWorkspace();
                return;
            }
        }
        if (id && (id === 'palette.open' || this.workspaceView.activeViewKind() === 'scene-viewport') && this.dispatchEditorAction(id)) {
            event.preventDefault();
            event.stopPropagation();
        }
    }

    protected handleNativeShortcut(id: EditorActionId): void {
        if (this.workspaceActionBlocked()) return;
        if (this.workspaceView.propView()) {
            if (!this.propWorkspace.confirmingClose()) this.runPropAction(id);
            return;
        }
        if (this.workspaceView.activeViewKind() !== 'scene-viewport') return;
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

    openSceneInformation(): boolean {
        return this.sceneActions()?.show() ?? false;
    }

    protected showUsages(key: EntityKey): void {
        if (this.canRevealReference()) this.usage.set({key, context: this.sceneContext()});
    }

    protected async closePropWorkspace(): Promise<void> {
        if (await this.reviewPropNavigation()) this.propWorkspace.close(true, () => this.workspaceTabs()?.focusTab('scene-viewport'));
    }

    async reviewProjectNavigation(): Promise<boolean> {
        return await this.reviewPropNavigation() && await this.animatorWorkspace.reviewNavigation() &&
            await this.controlsGraph.reviewNavigation();
    }

    reviewPropNavigation(): Promise<boolean> {
        return this.propWorkspace.reviewNavigation();
    }

    private runPropAction(id: EditorActionId): boolean {
        const draft = this.propWorkspace.draft();
        if (!draft || draft.draft_revision === undefined) return false;
        switch (id) {
            case 'document.save':
                if (draft.dirty) void this.propWorkspace.saveDraft();
                return true;
            case 'document.undo':
                if (draft.can_undo) void this.propWorkspace.editDraft({type: 'undo'});
                return true;
            case 'document.redo':
                if (draft.can_redo) void this.propWorkspace.editDraft({type: 'redo'});
                return true;
            default:
                return false;
        }
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

    protected async repairSceneDependency(request: SceneDependencyRequest): Promise<void> {
        const before = this.editorService.snapshot();
        const {action, owner} = request;
        if (!this.canRevealReference() || before?.mode !== 'edit' || before.revision !== request.revision ||
            !before.document.entities.some(entity => entity.key === owner.key)) return;
        const current = () => {
            const snapshot = this.editorService.snapshot();
            const nativeGeneration = this.viewportEditor.state()?.workspace_generation;
            return this.editorService.workspaceGeneration() === request.generation &&
                (nativeGeneration == null || nativeGeneration === request.nativeGeneration) &&
                snapshot?.mode === 'edit' && snapshot.active_scene_id === request.scene &&
                snapshot.scene_revision === before.scene_revision;
        };
        if (!current() || !await this.reviewProjectNavigation() || !current() ||
            this.editorService.snapshot()?.revision !== request.revision) return;
        const inspect = async () => {
            if (!current()) return;
            this.workspaceView.selectView('scene-viewport');
            await this.revealReference({kind: 'entity', key: owner.key, componentId: owner.component_id ?? undefined});
        };
        switch (action.kind) {
            case 'inspector':
                await inspect();
                return;
            case 'prop':
                if (await this.propWorkspace.openInstance(action.key)) {
                    if (current()) this.workspaceTabs()?.focusTab('prop-workspace');
                    return;
                }
                break;
            case 'animation': {
                const key = action.key ?? owner.key;
                if (action.key && !before.document.entities.some(entity => entity.key === key && entity.animation?.enabled)) break;
                await this.sceneEditing.select(key);
                if (!current() || this.sceneEditing.entity()?.key !== key) return;
                if (!action.key) {
                    this.animationPreview.openAuthored();
                    this.workspaceTabs()?.focusTab('animation-workspace');
                    return;
                }
                this.animationPreview.library.set('imported');
                this.workspaceView.selectView('animation-workspace');
                this.animationPreview.chooseScope('scene');
                this.workspaceTabs()?.focusTab('animation-workspace');
                return;
            }
            case 'animator':
                if (this.animatorAssets.supported() && await this.animatorWorkspace.open(action.asset_id)) {
                    if (current()) this.workspaceTabs()?.focusTab('animation-controller');
                    return;
                }
                break;
            case 'controls_graph':
                if (!this.controlsGraph.supported()) break;
                await this.sceneEditing.select(owner.key);
                if (!current() || this.sceneEditing.entity()?.key !== owner.key) return;
                if (await this.controlsGraph.open(action.asset_id)) {
                    if (current()) this.workspaceTabs()?.focusTab('controls-graph');
                    return;
                }
                break;
        }
        await inspect();
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
            await this.editorService.execute(request.keys && this.batchParentingEnabled()
                ? {type: 'reparent_objects', keys: request.keys, parent: request.parent, keep_world: request.keepWorld ?? true}
                : CommandBuilder.reparent(request.key, request.parent, true));
        });
        if (!accepted && context === this.sceneContext()) this.hierarchyError.set(this.sceneEditing.error() ?? 'Finish the current edit and try changing the parent again.');
    }

    protected async editHierarchyFolder(command: HierarchyFolderCommand): Promise<void> {
        if (!this.hierarchyFoldersEnabled() || this.workspaceActionBlocked()) return;
        const context = this.sceneContext();
        this.hierarchyError.set(null);
        const accepted = await this.sceneEditing.withCommittedEdit(async () => {
            if (context === this.sceneContext() && !this.workspaceActionBlocked()) {
                await this.editorService.execute(command);
            }
        });
        if (!accepted && context === this.sceneContext()) {
            this.hierarchyError.set(this.sceneEditing.error() ?? 'Finish the current edit and try again.');
        }
    }

    protected async createHierarchyObject(type: HierarchyCreateCommand): Promise<void> {
        if (!this.hierarchyCreation().includes(type) || this.workspaceActionBlocked()) return;
        const context = this.sceneContext();
        this.hierarchyError.set(null);
        const accepted = await this.sceneEditing.withCommittedEdit(async () => {
            if (context !== this.sceneContext() || this.workspaceActionBlocked()) return;
            const current = this.editorService.snapshot();
            if (!current) return;
            const snapshot = await this.editorService.execute({type}, this.editorService.workspaceGeneration(), current.revision);
            if (context !== this.sceneContext()) return;
            const key = snapshot.selection[0];
            if (key) this.hierarchy()?.revealEntity(key);
        });
        if (!accepted && context === this.sceneContext()) {
            this.hierarchyError.set(this.sceneEditing.error() ?? 'Finish the current edit before creating the object.');
        }
    }
}
