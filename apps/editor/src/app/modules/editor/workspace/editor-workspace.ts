import {afterNextRender, Component, computed, DOCUMENT, effect, inject, Injector, input, output, signal, viewChild} from '@angular/core';
import {EDITOR_ACTIONS, type EditorActionId} from '../../../core/editor/editor-actions';
import {availableObjectMenuActions, objectMenuEntry, type ObjectMenuAction} from '../../../core/editor/editor-object-menu';
import {resolveEditorShortcut} from '../../../core/editor/editor-keymap';
import type {WorkspaceViewKind} from '../../../core/editor/workspace/workspace-view';
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
import type {HierarchyReparentRequest} from './scene-entities-panel/scene-entities-panel';
import {SceneEntitiesPanel} from "./scene-entities-panel/scene-entities-panel";
import {InspectorPanel} from './inspector-panel/inspector-panel';
import {WorkspaceToolbar} from './workspace-toolbar/workspace-toolbar';
import {AssetsPanel} from './assets-panel/assets-panel';
import {PropAssetSelection} from './prefabs/prop-asset-selection';
import {sameHierarchyScene, type SceneEntitiesModel} from './scene-entities-panel/scene-entities.model';
import type {HierarchyFolderCommand} from './scene-entities-panel/hierarchy-folder-editor';
import {SceneEditingService} from './scene-editing.service';
import {WorkspaceDraftReview} from './workspace-draft-review';
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
import {PhysicsLayersDialog} from './physics-layers/physics-layers-dialog';
import {SceneLightsDialog} from './scene-lights/scene-lights-dialog';
import {ParticleDesigner} from './particle-designer/particle-designer';
import {ParticleDesignerSession} from './particle-designer/particle-designer-session';
import {SelectedCameraPreviewService} from './game-settings/selected-camera-preview.service';
import {RuntimeInspector} from './runtime-inspector/runtime-inspector';
import {CommandBuilder, type EditorCommand, type EntityKey, type FolderId} from '../../../core/editor/editor.models';
import type {SceneDependencyRequest} from '../../../core/editor/scene-verification.models';
import {CommandPalette} from './command-palette/command-palette';
import {FindUsages} from './find-usages/find-usages';
import {collectSceneReferences, type SceneReference} from './find-usages/scene-references';
import {editorProblems} from './problems-panel/editor-problems';
import {type EditorProblem, type ProblemLocation, ProblemsPanel} from './problems-panel/problems-panel';

@Component({
    selector: 'editor-workspace',
    providers: [
        SceneEditingService, WorkspaceDraftReview, ViewportEditorService, ViewportAssetDragService,
        SceneSwitchService, WorkspaceViewService, PropAssetSelection, PropWorkspaceService, PropPreviewService, AnimationWorkspaceService,
        SelectedCameraPreviewService, InputActionsCatalog,
        AnimatorAssets, AnimatorWorkspaceService, ControlGraphWorkspaceService, ParticleDesignerSession,
    ],
    imports: [
        EditorPane, EditorIcon, SceneViewport, WorkspaceTabs, SceneEntitiesPanel, InspectorPanel,
        WorkspaceToolbar, AssetsPanel, SceneActions, ProjectBuild, ViewportToolbar, GameInputManager,
        CameraSettings, PhysicsLayersDialog, SceneLightsDialog, ParticleDesigner, RuntimeInspector,
        CommandPalette, FindUsages, ProblemsPanel, PropWorkspace, AnimationWorkspace, AnimatorWorkspace, ControlGraphWorkspace, EditorDialog,
    ],
    templateUrl: './editor-workspace.html',
    styleUrls: ['./editor-workspace.scss', './animation/animation-workspace-layout.scss'],
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
    private readonly particles = inject(ParticleDesignerSession);
    private readonly draftReview = inject(WorkspaceDraftReview);
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
    readonly availableObjectActions = computed<readonly ObjectMenuAction[]>(() => {
        if (!this.editorService.nativeAvailable || this.workspaceActionBlocked() || !this.sceneEditing.ready() ||
            this.editorService.snapshot()?.mode !== 'edit') return [];
        return availableObjectMenuActions(new Set(this.editorService.capabilities()?.commands?.map(command => command.name)));
    });
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
    private readonly gameInput = viewChild(GameInputManager);
    private readonly physicsLayers = viewChild(PhysicsLayersDialog);
    private readonly lights = viewChild(SceneLightsDialog);
    private readonly assets = viewChild(AssetsPanel);
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
    }, {
        equal: sameHierarchyScene,
    });

    constructor() {
        this.draftReview.setReviewers([
            () => this.gameInput()?.canLeaveProject() ?? true,
            () => this.physicsLayers()?.canLeaveProject() ?? true,
            () => this.lights()?.canLeaveProject() ?? true,
            () => this.particles.canLeaveProject(),
            () => this.animationPreview.reviewNavigation(),
            () => this.reviewPropNavigation(),
            () => this.animatorWorkspace.reviewNavigation(),
            () => this.controlsGraph.reviewNavigation(),
        ]);
        this.workspaceView.setAuthoringAvailability({
            prop: () => this.propWorkspace.document()?.scope.generation === this.editorService.workspaceGeneration(),
            animator: this.animatorAssets.supported,
            controlsGraph: this.controlsGraph.supported,
        });
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

    canOpenControls(): boolean {
        return this.gameInput()?.available() === true;
    }

    openControls(): void {
        if (!this.document.querySelector('dialog[open]')) this.gameInput()?.show();
    }

    openPhysicsLayers(): boolean {
        return this.physicsLayers()?.show() ?? false;
    }

    protected async requestWorkspaceView(view: WorkspaceViewKind): Promise<void> {
        const context = this.sceneContext();
        if (this.workspaceView.animationView() && view !== 'animation-workspace' &&
            !await this.animationPreview.reviewNavigation()) return;
        if (context === this.sceneContext()) this.workspaceView.selectView(view);
    }

    async runObjectAction(id: ObjectMenuAction, folder: FolderId | null = null): Promise<void> {
        const entry = objectMenuEntry(id);
        if (!entry || !this.availableObjectActions().includes(id) || this.document.querySelector('dialog[open]')) return;
        if (folder && (entry.action.kind !== 'create' || !entry.action.allowFolder || !this.hierarchyFoldersEnabled())) return;
        const target = this.draftReview.capture();
        await this.draftReview.run(target, async () => {
            if (!this.availableObjectActions().includes(id)) return;
            this.workspaceView.selectView('scene-viewport');
            const action = entry.action;
            if (action.kind === 'browse') this.assets()?.browse(action.asset);
            else if (action.kind === 'scene-settings') this.lights()?.show();
            else await this.createSceneObject(action.command, folder);
        });
    }

    protected showUsages(key: EntityKey): void {
        if (this.canRevealReference()) this.usage.set({key, context: this.sceneContext()});
    }

    protected async closePropWorkspace(): Promise<void> {
        if (await this.reviewPropNavigation()) this.propWorkspace.close(true, () => this.workspaceTabs()?.focusTab('scene-viewport'));
    }

    reviewProjectNavigation(): Promise<boolean> {
        return this.draftReview.run(this.draftReview.capture());
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
        const inspect = async (field?: string) => {
            if (!current()) return;
            this.workspaceView.selectView('scene-viewport');
            await this.revealReference({kind: 'entity', key: owner.key, componentId: owner.component_id ?? undefined}, field);
        };
        switch (action.kind) {
            case 'inspector':
                await inspect();
                return;
            case 'script':
                await inspect(action.field ?? '$script');
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
                const target = this.draftReview.capture();
                const from = this.workspaceView.activeViewKind();
                const current = () => this.draftReview.current(target) && this.canRevealReference();
                if (from !== 'scene-viewport' && !await this.draftReview.run(target)) return;
                if (!current() || this.workspaceView.activeViewKind() !== from) return;

                await this.sceneEditing.select(location.key);
                if (!current() || this.sceneEditing.entity()?.key !== location.key ||
                    this.workspaceView.activeViewKind() !== from) return;
                this.workspaceView.selectView('scene-viewport');
                afterNextRender(() => {
                    if (!current() || this.workspaceView.activeViewKind() !== 'scene-viewport' ||
                        this.sceneEditing.entity()?.key !== location.key) return;
                    if (location.section === 'hierarchy') this.hierarchy()?.revealEntity(location.key);
                    else this.inspector()?.revealReference(location.key, location.componentId, location.section, location.slot, field);
                }, {injector: this.injector});
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

    private async createSceneObject(command: EditorCommand, folder: FolderId | null): Promise<void> {
        const context = this.sceneContext();
        this.hierarchyError.set(null);
        let light: EntityKey | undefined;
        let emitter: EntityKey | undefined;
        const accepted = await this.sceneEditing.withCommittedEdit(async () => {
            if (context !== this.sceneContext() || this.workspaceActionBlocked()) return;
            const current = this.editorService.snapshot();
            if (!current) return;
            const generation = this.editorService.workspaceGeneration();
            const snapshot = folder
                ? await this.createInFolder(command, folder, generation, current.revision)
                : await this.editorService.execute(command, generation, current.revision);
            if (context !== this.sceneContext()) return;
            if (command.type === 'create_light') {
                light = snapshot.document.lights.find(item => !current.document.lights.some(previous => previous.key === item.key))?.key;
                return;
            }
            const key = snapshot.selection[0];
            if (key) this.hierarchy()?.revealEntity(key);
            if (command.type === 'create_particle_emitter') emitter = key;
        });
        if (context !== this.sceneContext()) return;
        if (!accepted) {
            this.hierarchyError.set(this.sceneEditing.error() ?? 'Finish the current edit before creating the object.');
        }
        if (light) this.lights()?.show(light);
        if (emitter) this.particles.open(emitter);
    }

    private async createInFolder(command: EditorCommand, folder: FolderId, generation: number, revision: number) {
        const editor = this.editorService;
        const started = await editor.execute({type: 'begin_transaction', label: 'Create object in folder'}, generation, revision);
        try {
            const created = await editor.execute(command, generation, started.revision);
            const key = created.selection[0];
            if (!key) throw new Error('The editor did not select the new object.');
            const placed = await editor.execute({type: 'move_roots_to_folder', roots: [key], folder}, generation, created.revision);
            return await editor.execute({type: 'commit_transaction'}, generation, placed.revision);
        } catch (cause) {
            if (editor.workspaceGeneration() === generation && started.transaction &&
                editor.snapshot()?.transaction === started.transaction) {
                await editor.execute({type: 'cancel_transaction'}, generation, editor.snapshot()?.revision);
            }
            throw cause;
        }
    }
}
