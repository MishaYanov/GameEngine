import {
    afterNextRender,
    Component,
    computed,
    DestroyRef,
    DOCUMENT,
    effect,
    ElementRef,
    inject,
    Injector,
    input,
    output,
    signal,
    viewChild
} from '@angular/core';
import {listen} from '@tauri-apps/api/event';
import {EDITOR_ACTIONS, type EditorActionId} from '../../../core/editor/editor-actions';
import {
    allowsFolder,
    availableObjectMenuActions,
    OBJECT_MENU_GROUPS,
    objectMenuEntry,
    type ObjectMenuAction
} from '../../../core/editor/editor-object-menu';
import {resolveEditorShortcut} from '../../../core/editor/editor-keymap';
import type {WorkspaceViewKind} from '../../../core/editor/workspace/workspace-view';
import {Drawer} from '../../../shared/components/drawer/drawer';
import {EditorPane} from '../../../shared/components/pane/pane';
import {EditorIcon, type EditorIconName} from '../../../shared/icons/editor-icon';
import {EditorDialog} from '../../../shared/components/dialog/dialog';
import {ContextActionBuilder} from '../../../shared/actions/context-action';
import {ContextMenuBuilder, type ContextMenuItem} from '../../../shared/builder/context-menu-builder';
import {EditorContextMenuTrigger} from '../../../shared/components/context-menu/context-menu-trigger';
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
import {AgentDrawer} from './agent-drawer/agent-drawer';
import {AgentAnswerActions} from './agent-drawer/agent-answer-actions';
import {AgentDrawerService} from './agent-drawer/agent-drawer.service';
import {AgentSelectionContextService} from './agent-drawer/agent-selection-context';
import {AUTHORING_DRAWER_CONFIG, type RightDrawerMode} from './agent-drawer/authoring-drawer';
import {WorkspaceToolbar} from './workspace-toolbar/workspace-toolbar';
import {AssetsPanel} from './assets-panel/assets-panel';
import {PropAssetSelection} from './prefabs/prop-asset-selection';
import {sameHierarchyScene, type SceneEntitiesModel} from './scene-entities-panel/scene-entities.model';
import type {HierarchyFolderCommand} from './scene-entities-panel/hierarchy-folder-editor';
import {SceneEditingService} from './scene-editing.service';
import {WorkspaceDraftReview} from './workspace-draft-review';
import {WORKSPACE_LAYOUT_SCOPE_PROVIDER} from './workspace-layout-preferences';
import {ScopedPaneRequests} from './scoped-pane-request';
import {SceneActions} from './scene-actions/scene-actions';
import {SceneFlow} from './scene-flow/scene-flow';
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
import {AudioWorkspace} from './audio-source/audio-workspace';
import {ComponentWorkspaceSession} from './components/component-workspace-session';
import {BehaviorWorkspace} from './components/behavior-workspace';
import {ParticleDesignerSession} from './particle-designer/particle-designer-session';
import {SelectedCameraPreviewService} from './game-settings/selected-camera-preview.service';
import {RuntimeInspector} from './runtime-inspector/runtime-inspector';
import {
    CommandBuilder,
    type EditorCommand,
    type EditorSnapshot,
    type EntityKey,
    type FolderId,
    type PrimitiveKind,
    type SceneId,
    type Vec3,
    type ViewportInput
} from '../../../core/editor/editor.models';
import {WorkspaceNavigation, type NavigationDirection} from './workspace-navigation';
import type {SceneDependencyRequest} from '../../../core/editor/scene-verification.models';
import {CommandPalette} from './command-palette/command-palette';
import {FindUsages} from './find-usages/find-usages';
import {collectSceneReferences, type SceneReference} from './find-usages/scene-references';
import {editorProblems} from './problems-panel/editor-problems';
import {type EditorProblem, type ProblemLocation, ProblemsPanel} from './problems-panel/problems-panel';

interface SceneObjectPlacement {
    readonly workspace_generation: number;
    readonly revision: number;
    readonly scene_id: SceneId;
    readonly position: Vec3 | null;
}

interface SceneViewportContextMenu extends SceneObjectPlacement {
    readonly client_x: number;
    readonly client_y: number;
}

@Component({
    selector: 'editor-workspace',
    providers: [
        SceneEditingService, WorkspaceDraftReview, ScopedPaneRequests, ViewportEditorService, ViewportAssetDragService,
        WORKSPACE_LAYOUT_SCOPE_PROVIDER,
        SceneSwitchService, WorkspaceViewService, WorkspaceNavigation, PropAssetSelection, PropWorkspaceService, PropPreviewService, AnimationWorkspaceService,
        SelectedCameraPreviewService, InputActionsCatalog, ComponentWorkspaceSession, AgentSelectionContextService, AgentDrawerService, AgentAnswerActions,
        AnimatorAssets, AnimatorWorkspaceService, ControlGraphWorkspaceService, ParticleDesignerSession,
    ],
    imports: [
        Drawer, EditorPane, EditorIcon, SceneViewport, WorkspaceTabs, SceneEntitiesPanel, InspectorPanel, AgentDrawer,
        WorkspaceToolbar, AssetsPanel, SceneActions, SceneFlow, ProjectBuild, ViewportToolbar, GameInputManager,
        EditorContextMenuTrigger,
        CameraSettings, PhysicsLayersDialog, SceneLightsDialog, ParticleDesigner, RuntimeInspector, AudioWorkspace, BehaviorWorkspace,
        CommandPalette, FindUsages, ProblemsPanel, PropWorkspace, AnimationWorkspace, AnimatorWorkspace, ControlGraphWorkspace, EditorDialog,
    ],
    templateUrl: './editor-workspace.html',
    styleUrls: [
        './editor-workspace.scss',
        './animation/animation-workspace-layout.scss',
        './agent-drawer/right-drawer-layout.scss',
    ],
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
    protected readonly componentWorkspace = inject(ComponentWorkspaceSession);
    private readonly draftReview = inject(WorkspaceDraftReview);
    private readonly paneRequests = inject(ScopedPaneRequests);
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
    private readonly creatingObject = signal<string | null>(null);
    private readonly createdObject = signal<{ key: EntityKey; context: string; message: string } | null>(null);
    protected readonly hierarchyCreationStatus = computed(() => {
        const current = this.creatingObject();
        if (current) return current;
        const created = this.createdObject();
        return created?.context === this.sceneContext() && this.editorService.selection().includes(created.key)
            ? created.message : null;
    });
    readonly objectActionsEnabled = computed(() =>
        !this.workspaceActionBlocked() && this.sceneEditing.ready() && this.creatingObject() === null);
    readonly availableObjectActions = computed<readonly ObjectMenuAction[]>(() => {
        if (!this.editorService.nativeAvailable || this.editorService.snapshot()?.mode !== 'edit') return [];
        const capabilities = this.editorService.capabilities();
        const primitives = capabilities?.primitive_assets === true;
        return availableObjectMenuActions(new Set(capabilities?.commands?.map(command => command.name)), primitives);
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
    protected readonly componentPaneActive = computed(() => this.workspaceView.audioView() || this.workspaceView.behaviorView());
    protected readonly showingViewport = computed(() =>
        this.workspaceView.animatorView() || this.workspaceView.controlsGraphView() || this.componentPaneActive() ? false :
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
    protected readonly maximizeScene = signal(false);
    protected readonly sceneMaximized = computed(() => this.maximizeScene() &&
        this.workspaceView.activeViewKind() === 'scene-viewport' && !this.previewingCamera() &&
        !this.selectedCameraPreview.active() && this.showingViewport());
    private readonly sceneToolbar = viewChild(ViewportToolbar);
    private readonly sceneSurface = viewChild<unknown, ElementRef<HTMLElement>>('sceneSurface', {read: ElementRef});
    private readonly workspacePanel = viewChild<ElementRef<HTMLElement>>('workspacePanel');
    private readonly workspaceTabs = viewChild(WorkspaceTabs);
    private readonly workspaceToolbar = viewChild(WorkspaceToolbar);
    private readonly hierarchy = viewChild(SceneEntitiesPanel);
    private readonly inspector = viewChild(InspectorPanel);
    private readonly authoringDrawer = viewChild<Drawer>('authoringDrawer');
    private readonly agentDrawer = viewChild(AgentDrawer);
    private readonly propPanel = viewChild(PropWorkspace);
    protected readonly authoringDrawerConfig = AUTHORING_DRAWER_CONFIG;
    protected readonly rightDrawerMode = signal<RightDrawerMode>('inspector');
    protected readonly rightDrawerView = computed(() => this.editorService.playing() ?
        (this.workspaceView.gameView() ? 'runtime-inspector' : 'inspector') : this.rightDrawerMode());
    protected readonly sceneInspectorVisible = computed(() =>
        this.workspaceView.activeViewKind() === 'scene-viewport' || this.workspaceView.gameView());
    protected readonly showingAuthoringDrawer = computed(() =>
        this.rightDrawerView() === 'agent' || this.sceneInspectorVisible());
    protected readonly rightDrawerLabel = computed(() => this.rightDrawerView() === 'agent' ? 'Agent' : 'Inspector');
    protected readonly rightDrawerSwitchDisabled = computed(() =>
        this.workspaceActionBlocked() || this.editorService.playing() ||
        !this.editorService.snapshot());
    private readonly cameraSettings = viewChild(CameraSettings);
    private readonly sceneActions = viewChild(SceneActions);
    private readonly sceneFlow = viewChild(SceneFlow);
    private readonly gameInput = viewChild(GameInputManager);
    private readonly physicsLayers = viewChild(PhysicsLayersDialog);
    private readonly lights = viewChild(SceneLightsDialog);
    private readonly assets = viewChild(AssetsPanel);
    private readonly navigation = inject(WorkspaceNavigation);
    private readonly navigationBlocked = computed(() => this.workspaceActionBlocked() || this.editorService.playing() ||
        !!this.viewportEditor.state()?.gesture);
    readonly canGoBack = computed(() => !this.navigationBlocked() && this.navigation.canGoBack());
    readonly canGoForward = computed(() => !this.navigationBlocked() && this.navigation.canGoForward());
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
        this.listenForViewportContextMenu();
        inject(AgentSelectionContextService).setSceneSubject(() => this.inspector()?.subject() ?? null);
        inject(AgentAnswerActions).setReveal((key, componentId) => {
            const context = this.sceneContext();
            this.rightDrawerMode.set('inspector');
            afterNextRender(() => {
                if (this.sceneContext() !== context || this.rightDrawerMode() !== 'inspector') return;
                if (this.workspaceView.propView() && this.propWorkspace.selectedObject()?.key === key) {
                    this.propPanel()?.focusInspector();
                } else if (this.sceneEditing.entity()?.key === key) {
                    this.inspector()?.revealReference(key, componentId);
                }
            }, {injector: this.injector});
        });
        this.draftReview.setReviewers([
            () => this.gameInput()?.canLeaveProject() ?? true,
            () => this.sceneFlow()?.canLeaveProject() ?? true,
            () => this.physicsLayers()?.canLeaveProject() ?? true,
            () => this.lights()?.canLeaveProject() ?? true,
            () => this.particles.canLeaveProject(),
            () => this.animationPreview.reviewNavigation(),
            () => this.componentWorkspace.reviewNavigation(),
            () => this.reviewPropNavigation(),
            () => this.animatorWorkspace.reviewNavigation(),
            () => this.controlsGraph.reviewNavigation(),
        ]);
        this.workspaceView.setAuthoringAvailability({
            prop: () => this.propWorkspace.document()?.scope.generation === this.editorService.workspaceGeneration(),
            animator: this.animatorAssets.supported,
            controlsGraph: this.controlsGraph.supported,
            audio: () => this.componentWorkspace.available('audio-workspace'),
            behavior: () => this.componentWorkspace.available('behavior-workspace'),
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

    protected readonly canOpenWorkspaceMenu = (event: Event): boolean => {
        if (!this.workspaceMenuAvailable() || !(event.target instanceof Element)) return false;
        if (event.target.closest('input, textarea, select, [contenteditable]:not([contenteditable="false"]), dialog'))
            return false;
        return event.type !== 'contextmenu' || !event.target.closest('scene-viewport');
    };

    private listenForViewportContextMenu(): void {
        if (!this.editorService.nativeAvailable) return;
        const lifetime = inject(DestroyRef);
        let unsubscribe: (() => void) | undefined;
        lifetime.onDestroy(() => unsubscribe?.());
        void listen<SceneViewportContextMenu>('editor://viewport-context-menu', ({payload}) => {
            if (!lifetime.destroyed) this.openViewportContextMenu(payload);
        }).then(release => {
            if (lifetime.destroyed) release();
            else unsubscribe = release;
        }).catch(() => undefined);
    }

    private openViewportContextMenu(context: SceneViewportContextMenu): void {
        if (!context || !this.workspaceMenuAvailable(true) || this.workspaceView.activeViewKind() !== 'scene-viewport' ||
            this.previewingCamera() || this.selectedCameraPreview.active() ||
            !this.currentObjectPlacement(context) || !Number.isFinite(context.client_x) || !Number.isFinite(context.client_y)) return;
        const viewport = this.sceneSurface()?.nativeElement;
        const surface = viewport?.querySelector<HTMLElement>('.surface');
        const panel = this.workspacePanel()?.nativeElement;
        if (!viewport || !surface || !panel) return;
        const {left, top, right, bottom, width, height} = surface.getBoundingClientRect();
        const x = context.client_x, y = context.client_y;
        if (width <= 0 || height <= 0 || x < left - 2 || x > right + 2 || y < top - 2 || y > bottom + 2) return;
        const position = context.position;
        const placement = Array.isArray(position) && position.length === 3 &&
        position.every(value => typeof value === 'number' && Number.isFinite(Math.fround(value)))
            ? context : {...context, position: null};
        this.contextMenu.open({
            items: this.workspaceContextMenu(placement), label: 'Workspace actions', x, y, owner: viewport,
            returnFocus: panel,
        });
    }

    private currentObjectPlacement(placement: SceneObjectPlacement): boolean {
        const snapshot = this.editorService.snapshot();
        const viewport = this.viewportEditor.state();
        return snapshot?.mode === 'edit' && snapshot.active_scene_id === placement.scene_id &&
            snapshot.revision === placement.revision && viewport?.workspace_generation === placement.workspace_generation;
    }

    private readonly objectContextMenu = this.buildObjectContextMenu();

    private buildObjectContextMenu(): ContextMenuBuilder<{ scene: string; placement?: SceneObjectPlacement }> {
        return ContextMenuBuilder.create<{ scene: string; placement?: SceneObjectPlacement }>()
            .submenu('workspace.create-object', 'Create Object', submenu => {
                for (const [index, group] of OBJECT_MENU_GROUPS.entries()) {
                    if (index) submenu.separator();
                    for (const entry of group) {
                        submenu.action(ContextActionBuilder.create<{ scene: string; placement?: SceneObjectPlacement }>(
                            'workspace.' + entry.id, entry.label,
                        ).when(() => this.availableObjectActions().includes(entry.id))
                            .enabledWhen(({
                                              scene,
                                              placement
                                          }) => scene === this.sceneContext() && this.workspaceMenuAvailable(!!placement) &&
                                this.objectActionsEnabled() && (!placement || this.currentObjectPlacement(placement)))
                            .run(({placement}) => this.runObjectAction(entry.id, null, placement)).build());
                    }
                }
            }, 'plus');
    }

    protected readonly workspaceContextMenu = (placement?: SceneObjectPlacement): readonly ContextMenuItem[] => {
        const context = this.sceneContext();
        const toolbar = this.workspaceToolbar();
        const build = this.projectBuild();
        const run = (action: () => void | Promise<unknown>) => () => {
            if (context === this.sceneContext() && this.workspaceMenuAvailable()) return action();
        };
        const workspaceCommand = (id: 'document.save' | 'document.save-all' | 'document.undo' |
            'document.redo' | 'game.play' | 'game.stop', icon?: EditorIconName): ContextMenuItem => {
            const action = EDITOR_ACTIONS[id];
            return {
                kind: 'action', id, label: action.label, icon, destructive: false,
                disabled: toolbar?.canPerform(action.command) !== true,
                execute: run(() => {
                    this.dispatchEditorAction(id);
                }),
            };
        };
        const playing = this.editorService.playing();
        let buildLabel = 'Build Game…';
        if (playing) buildLabel = 'Build Game… (stop Play first)';
        else if (this.editorService.dirty() || this.sceneEditing.hasChanges())
            buildLabel = 'Build Game… (save changes first)';
        return [
            ...this.objectContextMenu.build({scene: context, placement}),
            {
                kind: 'action', id: 'project.build', label: buildLabel, icon: 'build',
                destructive: false, disabled: build?.canExport() !== true,
                execute: run(() => build?.exportProject())
            },
            {
                kind: 'action', id: 'scene.create', label: 'New Scene…', icon: 'plus',
                destructive: false, disabled: !this.canCreateScene(), execute: run(() => {
                    this.openNewScene();
                })
            },
            {kind: 'separator', id: 'workspace-edit'},
            workspaceCommand('document.save', 'save'),
            workspaceCommand('document.save-all', 'save'),
            workspaceCommand('document.undo'),
            workspaceCommand('document.redo'),
            {kind: 'separator', id: 'workspace-play'},
            playing ? workspaceCommand('game.stop', 'stop') : workspaceCommand('game.play', 'play'),
            {
                kind: 'action', id: 'workspace.problems', label: 'Problems', icon: 'script',
                destructive: false, disabled: false, execute: run(() => this.openProblems())
            },
        ];
    };

    private workspaceMenuAvailable(nativeRelease = false): boolean {
        const gesture = this.viewportEditor.state()?.gesture;
        // A trusted right-release is emitted after Fly ends. Its status may
        // still be queued behind the context menu event in the WebView.
        return !!this.editorService.snapshot() && !this.workspaceActionBlocked() &&
            (!gesture || (nativeRelease && gesture === 'fly')) && !this.document.querySelector('dialog[open]');
    }

    protected openPalette(): boolean {
        if (!this.canOpenPalette() || this.document.querySelector('dialog[open]')) return false;
        this.paletteOpen.set(true);
        return true;
    }

    openSceneFlow(): boolean {
        return !this.document.querySelector('dialog[open]') && (this.sceneFlow()?.show() ?? false);
    }

    canCreateScene(): boolean {
        return !this.workspaceActionBlocked() && this.sceneActions()?.canCreateScene() === true;
    }

    openNewScene(): boolean {
        return this.canCreateScene() && !this.document.querySelector('dialog[open]') &&
            (this.sceneActions()?.showNewScene() ?? false);
    }

    openSceneInformation(): boolean {
        return this.sceneActions()?.show() ?? false;
    }

    canOpenControls(): boolean {
        return this.gameInput()?.available() === true;
    }

    protected canOpenInputActionsAsset(): boolean {
        return !this.workspaceActionBlocked() && this.gameInput()?.canOpenInputActionsAsset() === true;
    }

    protected openInputActionsAsset(request: { readonly assetId: string; readonly workspaceGeneration: number }): void {
        if (this.canOpenInputActionsAsset() && request.workspaceGeneration === this.editorService.workspaceGeneration() &&
            !this.document.querySelector('dialog[open]')) this.gameInput()?.show(request.assetId);
    }

    openControls(): void {
        if (!this.document.querySelector('dialog[open]')) this.gameInput()?.show();
    }

    openPhysicsLayers(): boolean {
        return this.physicsLayers()?.show() ?? false;
    }

    protected async requestWorkspaceView(view: WorkspaceViewKind, replayOrigin?: 'scene' | 'prop'): Promise<boolean> {
        this.paneRequests.invalidate();
        const context = this.sceneContext();
        if (this.workspaceView.animationView() && view !== 'animation-workspace' &&
            !await this.animationPreview.reviewNavigation()) return false;
        if (this.componentPaneActive() && view !== this.workspaceView.activeViewKind() &&
            !await this.componentWorkspace.reviewNavigation()) return false;
        if (context !== this.sceneContext() || !this.workspaceView.availableViews().includes(view)) return false;
        this.workspaceView.selectView(view, replayOrigin, replayOrigin === undefined);
        return this.workspaceView.activeViewKind() === view;
    }

    protected recordAssetFolder(visit: {
        readonly previous: string; readonly current: string; readonly direction?: NavigationDirection;
    }): void {
        this.navigation.record({kind: 'folder', path: visit.previous}, {
            kind: 'folder',
            path: visit.current
        }, visit.direction);
    }

    async navigateHistory(direction: NavigationDirection): Promise<void> {
        if (this.navigationBlocked() || this.contextMenu.request() || this.document.querySelector('dialog[open], :popover-open')) return;
        await this.navigation.step(direction, async location => {
            if (location.kind === 'folder') {
                if (this.workspaceView.animationView() || !this.assets()?.replayFolder(location.path, direction)) return 'unavailable';
                this.maximizeScene.set(false);
                return 'applied';
            }
            if (location.kind === 'scene') {
                const snapshot = this.editorService.snapshot();
                if (!snapshot || location.id === snapshot.active_scene_id ||
                    !snapshot.scenes.some(scene => scene.id === location.id)) return 'unavailable';
                return await this.sceneSwitch.request(location.id, false) ? 'applied' : 'cancelled';
            }
            const active = this.workspaceView.navigationLocation();
            if (!this.workspaceView.availableViews().includes(location.view) ||
                active.view === location.view && active.origin === location.origin) return 'unavailable';
            return await this.requestWorkspaceView(location.view, location.origin) ? 'applied' : 'cancelled';
        });
    }

    protected handleNativeNavigation(input: ViewportInput): void {
        if (input.kind === 'pointer_up' && (input.button === 'back' || input.button === 'forward')) {
            void this.navigateHistory(input.button);
        } else if (input.kind === 'key_down' && !input.repeat && !input.modifiers.control &&
            !input.modifiers.super_key && !input.modifiers.shift) {
            if (input.code === 'BrowserBack' || input.modifiers.alt && input.code === 'ArrowLeft') {
                void this.navigateHistory('back');
            } else if (input.code === 'BrowserForward' || input.modifiers.alt && input.code === 'ArrowRight') {
                void this.navigateHistory('forward');
            }
        }
    }

    protected async requestRightDrawer(mode: RightDrawerMode): Promise<void> {
        if (this.rightDrawerSwitchDisabled() || this.draftReview.pending()) return;
        if (this.rightDrawerMode() === mode) {
            this.focusRightDrawer(mode);
            return;
        }
        const view = this.workspaceView.activeViewKind();
        await this.draftReview.run(this.draftReview.capture(), async () => {
            if (this.workspaceActionBlocked() || this.editorService.playing() ||
                this.workspaceView.activeViewKind() !== view) return;
            this.rightDrawerMode.set(mode);
            this.focusRightDrawer(mode);
        });
    }

    private focusRightDrawer(mode: RightDrawerMode): void {
        this.maximizeScene.set(false);
        const focus = () => {
            if (this.rightDrawerView() !== mode) return;
            if (mode === 'agent') this.agentDrawer()?.focus();
            else if (this.workspaceView.propView()) this.propPanel()?.focusInspector();
            else if (this.sceneInspectorVisible()) this.inspector()?.focus();
            else this.workspaceTabs()?.focusTab(this.workspaceView.activeViewKind());
        };
        afterNextRender(() => {
            if (this.rightDrawerView() !== mode) return;
            const drawer = this.authoringDrawer();
            if (this.showingAuthoringDrawer() && drawer && (drawer.collapsed() || !drawer.isExpanded())) {
                drawer.config.update(config => ({...config, collapsed: false, expanded: true}));
                afterNextRender(focus, {injector: this.injector});
            } else focus();
        }, {injector: this.injector});
    }

    async runObjectAction(
        id: ObjectMenuAction, folder: FolderId | null = null, placement?: SceneObjectPlacement,
    ): Promise<void> {
        const entry = objectMenuEntry(id);
        if (!entry || !this.objectActionsEnabled() || !this.availableObjectActions().includes(id) ||
            this.document.querySelector('dialog[open]')) return;
        if (folder && (!allowsFolder(entry) || !this.hierarchyFoldersEnabled())) return;
        const target = this.draftReview.capture();
        await this.draftReview.run(target, async () => {
            if (!this.objectActionsEnabled() || !this.availableObjectActions().includes(id) ||
                (placement && !this.currentObjectPlacement(placement))) return;
            this.maximizeScene.set(false);
            this.workspaceView.selectView('scene-viewport');
            const action = entry.action;
            if (action.kind === 'browse') this.assets()?.browse(action.asset);
            else if (action.kind === 'scene-settings') this.lights()?.show();
            else {
                this.creatingObject.set('Creating ' + entry.label + '…');
                this.createdObject.set(null);
                try {
                    if (action.kind === 'primitive') await this.createPrimitive(action.primitive, folder, placement);
                    else await this.createSceneObject(action.command, folder);
                } finally {
                    this.creatingObject.set(null);
                }
            }
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
                this.workspaceView.selectView('animation-workspace', 'scene');
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
                if ((from !== 'scene-viewport' || this.rightDrawerMode() === 'agent') &&
                    !await this.draftReview.run(target)) return;
                if (!current() || this.workspaceView.activeViewKind() !== from) return;

                await this.sceneEditing.select(location.key);
                if (!current() || this.sceneEditing.entity()?.key !== location.key ||
                    this.workspaceView.activeViewKind() !== from) return;
                this.workspaceView.selectView('scene-viewport');
                if (location.section !== 'hierarchy') this.rightDrawerMode.set('inspector');
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
        const accepted = await this.sceneEditing.withCommittedEdit(async () => {
            if (context !== this.sceneContext() || this.workspaceActionBlocked()) return;
            const current = this.editorService.snapshot();
            if (!current) return;
            const generation = this.editorService.workspaceGeneration();
            const create = (revision: number) => this.editorService.execute(command, generation, revision);
            const snapshot = folder
                ? await this.createInFolder(create, folder, generation, current.revision)
                : await create(current.revision);
            if (context !== this.sceneContext()) return;
            this.revealCreatedObject(snapshot, context);
        });
        if (context !== this.sceneContext()) return;
        if (!accepted) {
            this.hierarchyError.set(this.sceneEditing.error() ?? 'Finish the current edit before creating the object.');
        }
    }

    private revealCreatedObject(snapshot: EditorSnapshot, context: string): void {
        const key = snapshot.selection[0];
        if (!key || context !== this.sceneContext()) return;
        this.rightDrawerMode.set('inspector');
        const drawer = this.authoringDrawer();
        if (drawer && (drawer.collapsed() || !drawer.isExpanded())) {
            drawer.config.update(config => ({...config, collapsed: false, expanded: true}));
        }
        this.hierarchy()?.revealEntity(key);
        const name = snapshot.document.entities.find(entity => entity.key === key)?.name;
        if (name) this.createdObject.set({key, context, message: 'Created ' + name + '. Edit it in the Inspector.'});
    }

    async createPrimitive(
        kind: PrimitiveKind, folder: FolderId | null = null, placement?: SceneObjectPlacement,
    ): Promise<void> {
        const context = this.sceneContext();
        const editor = this.editorService;
        const generation = editor.workspaceGeneration();
        if (this.workspaceActionBlocked()) return;

        this.hierarchyError.set(null);
        const current = () => context === this.sceneContext() && !this.workspaceActionBlocked();
        const accepted = await this.sceneEditing.withCommittedEdit(async () => {
            const before = editor.snapshot();
            if (!current() || !before) return;
            const {active_scene_id: sceneId, revision} = before;
            const scope = await editor.authoringScope(generation);
            if (!current()) return;
            if (scope.revision !== revision || editor.snapshot()?.revision !== revision ||
                (placement && (placement.revision !== revision || placement.workspace_generation !== scope.nativeGeneration))) {
                throw new Error('The scene changed while preparing the object. Choose the object again.');
            }

            const {asset, snapshot: registered} = await editor.ensurePrimitive(kind, scope);
            if (!current()) return;
            try {
                const active = await editor.authoringScope(generation);
                if (!current()) return;
                if (active.nativeGeneration !== scope.nativeGeneration || active.revision !== registered.revision ||
                    editor.snapshot()?.revision !== registered.revision || registered.active_scene_id !== sceneId) {
                    throw new Error('The project changed while preparing the object. Choose the object again.');
                }
                const position: Vec3 = placement?.position ?? [0, 0, 0];
                const create = (nextRevision: number) =>
                    editor.importModel(asset.path, position, generation, nextRevision, {
                        nativeGeneration: scope.nativeGeneration,
                        sceneId
                    });
                const placed = folder
                    ? await this.createInFolder(create, folder, generation, registered.revision)
                    : await create(registered.revision);
                if (current()) this.revealCreatedObject(placed, context);
            } catch (cause) {
                const detail = cause instanceof Error ? cause.message : String(cause);
                throw new Error(`${asset.name} is available in Assets. ${detail}`, {cause});
            }
        });
        if (!accepted && current()) {
            this.hierarchyError.set(this.sceneEditing.error() ?? 'Finish the current edit before creating the object.');
        }
    }

    private async createInFolder(
        create: (revision: number) => Promise<EditorSnapshot>, folder: FolderId, generation: number, revision: number,
    ) {
        const editor = this.editorService;
        const started = await editor.execute({type: 'begin_transaction', label: 'Create object in folder'}, generation, revision);
        try {
            const created = await create(started.revision);
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
