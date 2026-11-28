import {
    afterNextRender,
    Component,
    computed,
    DestroyRef,
    effect,
    ElementRef,
    inject,
    Injector,
    input,
    linkedSignal,
    output,
    signal,
    untracked, viewChild, viewChildren,
} from '@angular/core';
import {NgTemplateOutlet} from '@angular/common';
import type {ModelAsset, ProjectAssetFile, SceneId} from '../../../../core/editor/editor.models';
import {ViewportEditorService} from '../../../../core/editor/viewport-editor.service';
import {EditorService} from '../../../../core/editor/editor.service';
import {Drawer} from '../../../../shared/components/drawer/drawer';
import type {DrawerSettings} from '../../../../shared/components/drawer/drawer-settings';
import {EditorPane} from '../../../../shared/components/pane/pane';
import {EditorIcon, type EditorIconName} from '../../../../shared/icons/editor-icon';
import type {Config} from '../../../../shared/models/config.model';
import {SceneSwitchService} from '../scene-actions/scene-switch.service';
import {SceneEditingService} from '../scene-editing.service';
import {ViewportAssetDragService} from '../viewport-asset-drag.service';
import {AssetAuthoring} from './asset-authoring';
import {AudioClipDetails} from './audio-clip-details';
import {startAudioClipDrag} from '../audio-source/audio-clip-drag';
import {PrefabAuthoring} from '../prefabs/prefab-authoring';
import {type PropAssetReveal, PropAssetSelection} from '../prefabs/prop-asset-selection';
import {PropWorkspaceService} from '../prop-workspace/prop-workspace.service';
import {AnimationWorkspaceService} from '../animation/animation-workspace.service';
import {AnimatorWorkspaceService} from '../animator/animator-workspace.service';
import {ControlGraphWorkspaceService} from '../control-graph/control-graph-workspace.service';
import {ContextActionBuilder} from '../../../../shared/actions/context-action';
import {ContextMenuBuilder, type ContextMenuItem} from '../../../../shared/builder/context-menu-builder';
import {EditorContextMenuTrigger} from '../../../../shared/components/context-menu/context-menu-trigger';

interface AssetFile {
    readonly path: string;
    readonly name: string;
}

type SceneAsset = AssetFile & { readonly kind: 'scene'; readonly sceneId: SceneId };
type PlaceableAsset = ModelAsset & { readonly kind: 'model' | 'prefab' };
type CataloguedAudio = ModelAsset & { readonly kind: 'audio_clip' };
type AssetEntry = SceneAsset | PlaceableAsset | CataloguedAudio | ProjectAssetFile;
type AssetAction = 'models' | 'add';
type BrowseScope = 'folder' | 'project';

function isPlaceable(asset: AssetEntry): asset is PlaceableAsset {
    return asset.kind === 'model' || asset.kind === 'prefab';
}

const ASSET_ICONS: Readonly<Record<AssetEntry['kind'], EditorIconName>> = {
    scene: 'cube', model: 'object', prefab: 'layers', audio_clip: 'audio', game_audio: 'audio',
    animation_clip: 'play', animator_graph: 'graph', controls_graph: 'graph', input_actions: 'graph',
    sky_panorama: 'cube', texture: 'object', rust_script: 'script',
};

const ASSET_LABELS: Readonly<Record<AssetEntry['kind'], string>> = {
    scene: 'Scene', model: 'Model', prefab: 'Prop', audio_clip: 'Audio Clip', game_audio: 'Game Audio',
    animation_clip: 'Animation Clip', animator_graph: 'Animator', controls_graph: 'Controls Graph',
    input_actions: 'Input Actions', sky_panorama: 'Sky Panorama', texture: 'Texture', rust_script: 'Rust Script',
};

const FILE_GUIDANCE: Partial<Record<AssetEntry['kind'], string>> = {
    game_audio: 'Used by game audio settings.',
    sky_panorama: 'Choose this panorama in a Sky component.',
};

const ASSET_FILTERS = [
    {value: 'all', label: 'All assets'},
    {value: 'model', label: 'Models'},
    {value: 'prefab', label: 'Props'},
    {value: 'scene', label: 'Scenes'},
    {value: 'audio_clip', label: 'Audio Clips'},
    {value: 'animator_graph', label: 'Animators'},
    {value: 'controls_graph', label: 'Controls Graphs'},
    {value: 'animation_clip', label: 'Animation Clips'},
    {value: 'input_actions', label: 'Input Actions'},
    {value: 'sky_panorama', label: 'Sky Panoramas'},
    {value: 'texture', label: 'Textures'},
    {value: 'rust_script', label: 'Rust Scripts'},
    {value: 'game_audio', label: 'Game Audio'},
] as const;

type AssetFilter = typeof ASSET_FILTERS[number]['value'];

const PRIMARY_FILTERS = new Set<AssetFilter>(['all', 'model', 'prefab', 'scene', 'audio_clip', 'game_audio']);

interface AssetMenuContext {
    readonly file: AssetEntry;
    readonly generation: number;
    readonly sceneId: SceneId | null;
}

interface FolderCrumb {
    readonly path: string;
    readonly label: string;
}

interface FolderHistory {
    readonly paths: readonly string[];
    readonly index: number;
}

export interface AssetsFolderNavigation {
    readonly previous: string;
    readonly current: string;
    readonly direction?: 'back' | 'forward';
}

const MAX_FOLDER_VISITS = 100;

function availableFolder(entries: readonly AssetEntry[], path: string): boolean {
    return path === '' || entries.some(entry => entry.path.startsWith(path + '/'));
}

function searchable(text: string): string {
    return text.trim().replaceAll('\\', '/').toLocaleLowerCase();
}

function parentFolder(path: string): string {
    return path.slice(0, Math.max(0, path.lastIndexOf('/')));
}

@Component({
    selector: 'editor-assets-panel',
    imports: [Drawer, EditorPane, EditorIcon, NgTemplateOutlet, AssetAuthoring, AudioClipDetails, PrefabAuthoring, EditorContextMenuTrigger],
    templateUrl: './assets-panel.html',
    styleUrls: ['./asset-browser-controls.scss', './assets-panel.scss'],
})
export class AssetsPanel {
    private readonly assetDrag = inject(ViewportAssetDragService, {optional: true});
    readonly opening = input(false);
    readonly canCreateScene = input(false);
    readonly newSceneRequested = output<void>();
    readonly folderNavigated = output<AssetsFolderNavigation>();
    readonly inputActionsAvailable = input(false);
    readonly inputActionsRequested = output<{ readonly assetId: string; readonly workspaceGeneration: number }>();

    protected dragAudioClip(file: AssetEntry, event: DragEvent): void {
        if (this.canRunAction() && file.kind === 'audio_clip' && file.asset_id) {
            startAudioClipDrag(event, file.asset_id, this.editor.workspaceGeneration());
        }
    }

    private readonly pendingAction = signal<AssetAction | null>(null);
    protected readonly loading = computed(() => this.pendingAction() === 'models');
    protected readonly error = signal<string | null>(null);
    protected readonly status = signal<string | null>(null);
    protected readonly drawerConfig: Config<DrawerSettings> = {
        height: '11rem',
        minHeight: '6rem',
        maxHeight: '50%',
    };
    private readonly editor = inject(EditorService);
    private readonly propAssetSelection = inject(PropAssetSelection, {optional: true});
    protected readonly propWorkspace = inject(PropWorkspaceService, {optional: true});
    private readonly animationWorkspace = inject(AnimationWorkspaceService, {optional: true});
    private readonly viewport = inject(ViewportEditorService, {optional: true});
    protected readonly animatorWorkspace = inject(AnimatorWorkspaceService, {optional: true});
    protected readonly controlsWorkspace = inject(ControlGraphWorkspaceService, {optional: true});
    private readonly drawer = viewChild(Drawer);
    private readonly folderLocation = viewChild<ElementRef<HTMLElement>>('location');
    private readonly searchInput = viewChild<ElementRef<HTMLInputElement>>('search');
    private readonly assetRows = viewChildren<ElementRef<HTMLButtonElement>>('assetRow');
    private readonly propAuthoring = viewChild(PrefabAuthoring);
    private readonly injector = inject(Injector);
    private readonly destroy = inject(DestroyRef);
    private readonly sceneEditing = inject(SceneEditingService, {optional: true});
    private readonly sceneSwitch = inject(SceneSwitchService);
    protected readonly canOpenScene = computed(() => this.canRunAction() && this.sceneSwitch.canSwitch());
    protected readonly projectOpen = computed(() => this.editor.snapshot() !== null);
    protected readonly models = this.editor.modelAssets;
    protected readonly assetIcons = ASSET_ICONS;
    protected readonly assetLabels = ASSET_LABELS;
    protected readonly fileGuidance = FILE_GUIDANCE;
    protected readonly refreshLabel = computed(() =>
        this.loading() ? 'Loading assets…' : this.models() === null ? 'Load Assets' : 'Refresh Assets');
    protected readonly assetFilters = computed(() => {
        const present = new Set(this.entries().map(entry => entry.kind));
        return ASSET_FILTERS.filter(({value}) =>
            value === 'all' || value === 'model' || value === 'prefab' || value === 'scene' ||
            value === 'audio_clip' || present.has(value));
    });
    protected readonly primaryFilters = computed(() =>
        this.assetFilters().filter(filter => PRIMARY_FILTERS.has(filter.value)));
    protected readonly additionalFilters = computed(() =>
        this.assetFilters().filter(filter => !PRIMARY_FILTERS.has(filter.value)));
    protected readonly activeAdditionalFilter = computed(() =>
        this.additionalFilters().find(filter => filter.value === this.assetFilter())?.label ?? null);
    protected readonly assetFilter = linkedSignal({
        source: () => ({options: this.assetFilters(), generation: this.editor.workspaceGeneration()}),
        computation: ({options, generation}, previous): AssetFilter =>
            previous?.source.generation === generation
                ? options.find(option => option.value === previous.value)?.value ?? 'all' : 'all',
    });
    protected readonly searchQuery = linkedSignal({source: this.editor.workspaceGeneration, computation: () => ''});
    private readonly query = computed(() => searchable(this.searchQuery()));
    protected readonly searching = computed(() => this.query().length > 0);
    private readonly scopePreference = linkedSignal({
        source: this.editor.workspaceGeneration,
        computation: (): BrowseScope | null => null,
    });
    protected readonly browseScope = computed(() => this.scopePreference() ?? (this.searching() ? 'project' : 'folder'));
    protected readonly canShowInFolder = computed(() => this.searching() || this.browseScope() === 'project');
    protected readonly canRunAction = computed(() => {
        const snapshot = this.editor.snapshot();
        return snapshot?.mode === 'edit' && this.editor.nativeAvailable && !this.opening() &&
            !this.pendingAction() && (this.sceneEditing
                ? this.sceneEditing.ready()
                : !this.editor.busy() && snapshot.transaction === null);
    });
    protected readonly canLoadModels = this.canRunAction;
    protected readonly canOpenProp = computed(() => this.canRunAction() && this.propWorkspace?.canOpen() === true);
    private catalogContext: { readonly generation: number; readonly version: number } | null = null;
    private readonly scenes = computed(() => this.editor.snapshot()?.scenes);
    private readonly entries = computed<readonly AssetEntry[]>(() => [
        ...(this.scenes() ?? []).map(scene => ({
            path: scene.asset_path,
            name: scene.asset_path.split('/').at(-1) ?? scene.id,
            kind: 'scene' as const,
            sceneId: scene.id,
        })),
        ...(this.models() ?? []).filter(model => model.kind !== 'sky_panorama').map(model => ({
            ...model, kind: (model.kind as PlaceableAsset['kind'] | CataloguedAudio['kind'] | undefined) ?? 'model',
        })),
        ...(this.editor.catalogueFiles?.() ?? []),
    ]);
    private readonly visibleEntries = computed(() => {
        const filter = this.assetFilter();
        return filter === 'all' ? this.entries() : this.entries().filter(entry => entry.kind === filter);
    });
    private readonly selection = linkedSignal({
        source: () => ({entries: this.visibleEntries(), generation: this.editor.workspaceGeneration()}),
        computation: ({entries, generation}, previous): AssetEntry | null => {
            const selected = previous?.source.generation === generation ? previous.value : null;
            if (!selected) return null;
            return entries.find(entry => this.sameFile(entry, selected)) ?? null;
        },
    });
    protected readonly selectedCatalogAsset = computed(() => {
        const selected = this.selected();
        return selected && (isPlaceable(selected) || selected.kind === 'audio_clip') ? selected : null;
    });
    protected readonly selectedRustScript = computed(() => {
        const selected = this.selected();
        return selected?.kind === 'rust_script' ? selected : null;
    });
    private readonly folderHistory = linkedSignal({
        source: () => ({entries: this.entries(), generation: this.editor.workspaceGeneration()}),
        computation: ({entries, generation}, previous): FolderHistory => {
            if (previous?.source.generation !== generation) return {paths: [''], index: 0};
            const {paths, index} = previous.value;
            const available = new Map<string, boolean>();
            const valid = (path: string) => {
                if (!available.has(path)) available.set(path, availableFolder(entries, path));
                return available.get(path);
            };
            const current = valid(paths[index]) ? paths[index] : '';
            const past = [...paths.slice(0, index).filter(valid), current];
            const next = [...past, ...paths.slice(index + 1).filter(valid)];
            return {
                paths: next.filter((path, position) => path !== next[position - 1]),
                index: past.filter((path, position) => path !== past[position - 1]).length - 1,
            };
        },
    });
    protected readonly folder = computed(() => {
        const {paths, index} = this.folderHistory();
        return paths[index];
    });
    protected readonly canGoBack = computed(() => this.projectOpen() && !this.opening() && this.folderHistory().index > 0);
    protected readonly canGoForward = computed(() => {
        const {paths, index} = this.folderHistory();
        return this.projectOpen() && !this.opening() && index < paths.length - 1;
    });
    protected readonly folderCrumbs = computed<readonly FolderCrumb[]>(() => {
        const crumbs: FolderCrumb[] = [{path: '', label: 'Assets'}];
        const folder = this.folder();
        if (!folder) return crumbs;
        let path = '';
        for (const label of folder.split('/')) {
            path = path ? `${path}/${label}` : label;
            crumbs.push({path, label});
        }
        return crumbs;
    });
    protected readonly folderContents = computed(() => {
        const folder = this.folder();
        const query = this.query();
        const prefix = folder ? folder + '/' : '';
        const projectWide = this.browseScope() === 'project';
        const folders = new Map<string, string>();
        if (!query && !projectWide) {
            for (const entry of this.entries()) {
                if (!entry.path.startsWith(prefix)) continue;
                const remainder = entry.path.slice(prefix.length);
                const separator = remainder.indexOf('/');
                if (separator !== -1) {
                    const name = remainder.slice(0, separator);
                    folders.set(name, prefix + name);
                }
            }
        }
        const files = this.visibleEntries().filter(entry => {
            if (!projectWide && !entry.path.startsWith(prefix)) return false;
            if (query) return searchable(entry.name).includes(query) || searchable(entry.path).includes(query);
            return projectWide || parentFolder(entry.path) === folder;
        });
        return {
            folders: [...folders].sort(([a], [b]) => a.localeCompare(b)),
            files: files.sort((a, b) => a.name.localeCompare(b.name) || a.path.localeCompare(b.path)),
        };
    });
    protected readonly selected = computed(() => {
        const selection = this.selection();
        return selection && this.folderContents().files.includes(selection) ? selection : null;
    });
    protected readonly noMatches = computed(() => (this.searching() || this.assetFilter() !== 'all') &&
        !this.folderContents().files.length);
    private readonly assetMenuBuilder = ContextMenuBuilder.create<AssetMenuContext>()
        .action(ContextActionBuilder.create<AssetMenuContext>('asset.select', 'Select Asset')
            .icon('object')
            .enabledWhen(context => this.menuFile(context) !== null)
            .run(context => {
                const file = this.menuFile(context);
                if (file) this.selectFile(file);
            }).build())
        .action(ContextActionBuilder.create<AssetMenuContext>('asset.show-in-folder', 'Show in Folder')
            .icon('folder')
            .when(() => this.canShowInFolder())
            .enabledWhen(context => this.menuFile(context) !== null)
            .run(context => {
                const file = this.menuFile(context);
                if (file) this.showInFolder(file);
            }).build())
        .separator()
        .action(ContextActionBuilder.create<AssetMenuContext>('asset.open-scene', 'Open Scene')
            .icon('cube')
            .when(context => context.file.kind === 'scene')
            .enabledWhen(context => {
                const file = this.menuFile(context);
                return file?.kind === 'scene' && this.canOpenScene() && !this.isActiveScene(file);
            })
            .run(async context => {
                const file = this.menuFile(context);
                if (file?.kind === 'scene') await this.openSceneFile(file);
            }).build())
        .action(ContextActionBuilder.create<AssetMenuContext>('asset.open-file', 'Open Asset')
            .icon('openFolder')
            .when(context => this.fileOpenLabel(context.file) !== null &&
                context.file.kind !== 'scene' && context.file.kind !== 'prefab')
            .enabledWhen(context => {
                const file = this.menuFile(context);
                return file !== null && this.canOpenFile(file);
            })
            .run(async context => {
                const file = this.menuFile(context);
                if (file) await this.openFile(file);
            }).build())
        .action(ContextActionBuilder.create<AssetMenuContext>('asset.add-model', 'Add to Scene')
            .icon('plus')
            .when(context => context.file.kind === 'model')
            .enabledWhen(context => this.menuFile(context) !== null && this.canRunAction())
            .run(async context => {
                const file = this.menuFile(context);
                if (file?.kind === 'model') await this.addModelFile(file);
            }).build())
        .action(ContextActionBuilder.create<AssetMenuContext>('asset.open-prop', 'Open Prop Workspace')
            .icon('object')
            .when(context => context.file.kind === 'prefab' && !!this.propWorkspace)
            .enabledWhen(context => this.menuFile(context) !== null && this.canOpenProp())
            .run(async context => {
                const file = this.menuFile(context);
                if (file) await this.openProp(file);
            }).build())
        .action(ContextActionBuilder.create<AssetMenuContext>('asset.place-linked-prop', 'Place Linked Prop…')
            .icon('plus')
            .when(context => context.file.kind === 'prefab')
            .enabledWhen(context => this.menuFile(context) !== null && this.canRunAction())
            .run(async context => {
                const file = this.menuFile(context);
                if (file?.kind === 'prefab') await this.propAuthoring()?.placeLinkedProp(file);
            }).build())
        .action(ContextActionBuilder.create<AssetMenuContext>('asset.instantiate-prefab', 'Place as Copy')
            .icon('plus')
            .when(context => context.file.kind === 'prefab')
            .enabledWhen(context => this.menuFile(context) !== null && this.canRunAction())
            .run(async context => {
                const file = this.menuFile(context);
                if (file?.kind === 'prefab') await this.addModelFile(file);
            }).build());

    protected assetMenu(file: AssetEntry): () => readonly ContextMenuItem[] {
        const context: AssetMenuContext = {
            file,
            generation: this.editor.workspaceGeneration(),
            sceneId: this.editor.snapshot()?.active_scene_id ?? null,
        };
        return () => this.assetMenuBuilder.build(context);
    }

    private menuFile(context: AssetMenuContext): AssetEntry | null {
        const snapshot = this.editor.snapshot();
        if (!snapshot || context.generation !== this.editor.workspaceGeneration() ||
            context.sceneId !== snapshot.active_scene_id) return null;
        return this.entries().find(file => this.sameFile(file, context.file)) ?? null;
    }

    constructor() {
        effect(() => {
            const generation = this.editor.workspaceGeneration();
            const version = this.editor.assetCatalogVersion?.() ?? 0;
            const previous = this.catalogContext;
            if ((previous?.generation === generation && previous.version === version) ||
                !this.editor.nativeAvailable || !this.projectOpen() || this.opening() || this.pendingAction()) return;
            this.catalogContext = {generation, version};
            if (this.models() === null || (previous !== null && previous.version !== version)) {
                untracked(() => void this.loadCatalog(false));
            }
        });
        effect(() => {
            const request = this.propAssetSelection?.request();
            if (!request) return;
            if (!this.ownsReveal(request)) {
                this.propAssetSelection?.complete(request);
                return;
            }
            if (this.opening() || this.pendingAction() || !this.drawer()?.configReady()) return;
            untracked(() => void this.revealProp(request));
        });
    }

    private ownsReveal(request: PropAssetReveal): boolean {
        return !this.destroy.destroyed &&
            request.workspaceGeneration === this.editor.workspaceGeneration() &&
            request.sceneId === this.editor.snapshot()?.active_scene_id &&
            this.editor.snapshot()?.mode === 'edit';
    }

    browse(filter: 'model' | 'prefab'): void {
        if (!this.projectOpen() || this.opening()) return;
        const context = this.editor.workspaceGeneration();
        this.drawer()?.config.update(config => ({...config, expanded: true, collapsed: false}));
        this.searchQuery.set('');
        this.visitFolder('');
        this.scopePreference.set(null);
        this.selection.set(null);
        this.assetFilter.set(filter);
        afterNextRender(() => {
            if (context === this.editor.workspaceGeneration() && this.assetFilter() === filter) {
                this.searchInput()?.nativeElement.focus({preventScroll: true});
            }
        }, {injector: this.injector});
    }

    private async revealProp(request: PropAssetReveal): Promise<void> {
        const current = () => this.ownsReveal(request) && this.propAssetSelection?.request() === request;
        const asset = () => this.entries().find(file => file.kind === 'prefab' && file.asset_id === request.sourceId);
        let file = asset();
        if (!file) {
            this.pendingAction.set('models');
            try {
                await this.editor.assets(true);
            } catch {
                // Report the failed reveal in the Assets drawer below.
            } finally {
                this.pendingAction.set(null);
            }
            file = asset();
        }
        if (!current()) return;
        this.propAssetSelection?.complete(request);
        this.drawer()?.config.update(config => ({...config, expanded: true, collapsed: false}));
        if (!file) {
            this.error.set('The linked Prop source is unavailable in Assets.');
            return;
        }
        if (this.assetFilter() !== 'prefab') this.assetFilter.set('all');
        this.revealAsset(file);
    }

    protected showInFolder(file: AssetEntry): void {
        if (!this.canShowInFolder()) return;
        const visible = this.folderContents().files.find(entry => this.sameFile(entry, file));
        if (visible) this.revealAsset(visible);
    }

    private revealAsset(file: AssetEntry): void {
        const generation = this.editor.workspaceGeneration();
        const snapshot = this.editor.snapshot();
        const folder = parentFolder(file.path);
        this.searchQuery.set('');
        this.scopePreference.set(null);
        this.visitFolder(folder);
        this.selectFile(file);
        afterNextRender(() => {
            const selected = this.selected();
            const current = this.editor.snapshot();
            if (generation !== this.editor.workspaceGeneration() || current?.active_scene_id !== snapshot?.active_scene_id ||
                current?.mode !== snapshot?.mode || this.searching() || this.folder() !== folder ||
                !selected || !this.sameFile(selected, file)) return;
            const index = this.folderContents().files.indexOf(selected);
            this.assetRows()[index]?.nativeElement.focus();
        }, {injector: this.injector});
    }

    protected upOneFolder(): void {
        const path = this.folder();
        if (path) this.openFolder(parentFolder(path));
    }

    protected chooseFilter(value: string, disclosure?: HTMLDetailsElement): void {
        this.filterAssets(value);
        if (disclosure) {
            disclosure.open = false;
            disclosure.querySelector('summary')?.focus({preventScroll: true});
        }
    }

    protected filterAssets(value: string): void {
        const filter = this.assetFilters().find(option => option.value === value);
        if (filter) this.assetFilter.set(filter.value);
    }

    protected setBrowseScope(scope: BrowseScope): void {
        this.scopePreference.set(scope);
    }

    protected openFolder(path: string): void {
        if (!this.canVisitFolder(path) || path === this.folder()) return;
        this.searchQuery.set('');
        this.scopePreference.set(null);
        this.selection.set(null);
        this.visitFolder(path);
        this.focusCurrentFolder(path);
    }

    protected moveFolderHistory(offset: -1 | 1, trigger: HTMLButtonElement): void {
        if (!(offset === -1 ? this.canGoBack() : this.canGoForward())) return;
        const history = this.folderHistory();
        const previous = this.folder();
        const index = history.index + offset;
        this.folderHistory.set({paths: history.paths, index});
        this.folderNavigated.emit({previous, current: this.folder(), direction: offset < 0 ? 'back' : 'forward'});
        if (!(offset === -1 ? this.canGoBack() : this.canGoForward())) this.focusCurrentFolder(this.folder(), trigger);
    }

    replayFolder(path: string, direction?: AssetsFolderNavigation['direction']): boolean {
        if (!this.canVisitFolder(path) || path === this.folder()) return false;
        const {paths, index} = this.folderHistory();
        const previous = paths.slice(0, index).lastIndexOf(path);
        const next = paths.indexOf(path, index + 1);
        if (previous >= 0 || next >= 0) {
            const destination = direction === 'back' && previous >= 0 ? previous
                : direction === 'forward' && next >= 0 ? next
                    : previous >= 0 && (next < 0 || index - previous <= next - index) ? previous : next;
            this.folderHistory.set({paths, index: destination});
        } else {
            this.folderHistory.set({paths: paths.map((folder, position) => position === index ? path : folder), index});
        }
        this.drawer()?.config.update(config => ({...config, expanded: true, collapsed: false}));
        return true;
    }

    private canVisitFolder(path: string): boolean {
        return this.projectOpen() && !this.opening() && availableFolder(this.entries(), path);
    }

    private visitFolder(path: string): void {
        const previous = this.folder();
        if (path === previous) return;
        const history = this.folderHistory();
        const paths = [...history.paths.slice(0, history.index + 1), path].slice(-MAX_FOLDER_VISITS);
        this.folderHistory.set({paths, index: paths.length - 1});
        this.folderNavigated.emit({previous, current: path});
    }

    private focusCurrentFolder(path: string, trigger?: HTMLButtonElement): void {
        const generation = this.editor.workspaceGeneration();
        afterNextRender(() => {
            if (this.editor.workspaceGeneration() !== generation || this.folder() !== path) return;
            const location = this.folderLocation()?.nativeElement;
            const active = location?.ownerDocument.activeElement;
            if (trigger && active !== trigger && active !== location?.ownerDocument.body) return;
            location?.focus({preventScroll: true});
        }, {injector: this.injector});
    }

    protected selectFile(file: AssetEntry): void {
        this.selection.set(file);
        this.error.set(null);
        this.status.set(null);
    }

    protected async openProp(file: AssetEntry): Promise<void> {
        if (file.kind !== 'prefab' || !this.canOpenProp() || !this.propWorkspace) return;
        const generation = this.editor.workspaceGeneration();
        const scene = this.editor.snapshot()?.active_scene_id;
        this.error.set(null);
        if (!await this.propWorkspace.openSource(file) && this.editor.workspaceGeneration() === generation &&
            this.editor.snapshot()?.active_scene_id === scene) this.error.set(this.propWorkspace.error());
    }

    protected canDrag(file: AssetEntry): boolean {
        if (file.kind !== 'scene' && file.kind !== 'model') return false;
        return this.assetDrag?.canBegin() === true && (file.kind === 'scene'
            ? this.canOpenScene() && !this.isActiveScene(file)
            : this.canRunAction());
    }

    protected beginAssetDrag(file: AssetEntry, event: PointerEvent): void {
        if (!this.canDrag(file)) return;
        if (file.kind === 'scene') {
            this.assetDrag?.candidateScene(file.sceneId, event, () => this.openSceneFile(file));
        } else {
            this.assetDrag?.candidate(file.path, event);
        }
    }

    protected isSelected(file: AssetEntry): boolean {
        const selected = this.selected();
        return selected !== null && this.sameFile(file, selected);
    }

    protected assetRowKey(file: AssetEntry): string {
        if (file.kind === 'scene') return `scene:${file.sceneId}`;
        return file.asset_id ? `${file.kind}:id:${file.asset_id}` : `${file.kind}:path:${file.path}`;
    }

    protected isActiveScene(file: AssetEntry): boolean {
        const snapshot = this.editor.snapshot();
        const runtime = snapshot?.mode === 'play' ? this.viewport?.gameStatus?.()?.scene_id : null;
        return file.kind === 'scene' && file.sceneId === (runtime ?? snapshot?.active_scene_id);
    }

    protected fileOpenLabel(file: AssetEntry): string | null {
        switch (file.kind) {
            case 'scene':
                return 'Open Scene';
            case 'prefab':
                return this.propWorkspace ? 'Open Prop Workspace' : null;
            case 'animation_clip':
                return this.animationWorkspace && file.asset_id ? 'Open Animation Clip' : null;
            case 'animator_graph':
                return this.animatorWorkspace && file.asset_id ? 'Open Animator' : null;
            case 'controls_graph':
                return this.controlsWorkspace && file.asset_id ? 'Open Controls Graph' : null;
            case 'input_actions':
                return this.inputActionsAvailable() && file.asset_id ? 'Open Input Actions' : null;
            default:
                return null;
        }
    }

    protected canOpenFile(file: AssetEntry): boolean {
        if (file.kind === 'scene') return this.canOpenScene() && !this.isActiveScene(file);
        if (file.kind === 'prefab') return this.canOpenProp();
        return this.canRunAction() && this.fileOpenLabel(file) !== null;
    }

    protected async openFile(requested: AssetEntry): Promise<void> {
        const file = this.entries().find(entry => this.sameFile(entry, requested));
        if (!file || !this.canOpenFile(file)) return;
        if (file.kind === 'scene') {
            await this.openSceneFile(file);
        } else if (file.kind === 'prefab') {
            await this.openProp(file);
        } else if (file.asset_id) {
            switch (file.kind) {
                case 'animation_clip': {
                    const generation = this.editor.workspaceGeneration();
                    const opened = await this.animationWorkspace?.openAuthoredAsset(file.asset_id);
                    const message = this.animationWorkspace?.authoredOpenError();
                    if (!opened && message && generation === this.editor.workspaceGeneration()) this.error.set(message);
                    break;
                }
                case 'animator_graph':
                    await this.animatorWorkspace?.open(file.asset_id);
                    break;
                case 'controls_graph':
                    await this.controlsWorkspace?.open(file.asset_id);
                    break;
                case 'input_actions':
                    this.inputActionsRequested.emit({
                        assetId: file.asset_id,
                        workspaceGeneration: this.editor.workspaceGeneration()
                    });
                    break;
            }
        }
    }

    protected async loadModels(): Promise<void> {
        if (this.canLoadModels()) await this.loadCatalog(true);
    }


    protected async addModel(): Promise<void> {
        const selected = this.selected();
        if (selected && isPlaceable(selected)) await this.addModelFile(selected);
    }

    private async addModelFile(selected: PlaceableAsset): Promise<void> {
        const generation = this.editor.workspaceGeneration();
        const sceneId = this.editor.snapshot()?.active_scene_id;
        await this.addAsset(async () => {
            if (generation !== this.editor.workspaceGeneration() || sceneId !== this.editor.snapshot()?.active_scene_id) return;
            const file = this.entries().find(entry => this.sameFile(entry, selected));
            if (!file || !isPlaceable(file)) return;
            if (file.kind === 'prefab') {
                await this.editor.loadPrefab(file.path, [0, 0, 0], generation, this.editor.snapshot()!.revision);
            } else await this.editor.importModel(file.path, [0, 0, 0], generation, this.editor.snapshot()!.revision);
            if (generation === this.editor.workspaceGeneration()) this.status.set(`Added ${file.name} to the scene.`);
        });
    }

    private async loadCatalog(refresh: boolean): Promise<void> {
        const workspace = this.editor.workspaceGeneration();
        this.pendingAction.set('models');
        this.error.set(null);
        this.status.set(null);
        try {
            await this.editor.assets(refresh);
            if (refresh && workspace === this.editor.workspaceGeneration()) this.status.set('Project assets refreshed.');
        } catch (error) {
            if (workspace === this.editor.workspaceGeneration()) this.error.set(error instanceof Error ? error.message : String(error));
        } finally {
            this.pendingAction.set(null);
        }
    }

    private async openSceneFile(file: SceneAsset): Promise<boolean> {
        if (!this.canOpenScene() || this.isActiveScene(file)) return false;
        this.error.set(null);
        this.status.set(null);
        return this.sceneSwitch.request(file.sceneId);
    }

    private async addAsset(action: () => Promise<void>): Promise<void> {
        if (!this.canRunAction()) return;
        const generation = this.editor.workspaceGeneration();
        this.pendingAction.set('add');
        this.error.set(null);
        this.status.set(null);
        try {
            if (this.sceneEditing) {
                if (!await this.sceneEditing.withCommittedEdit(action) && generation === this.editor.workspaceGeneration()) {
                    this.error.set(this.sceneEditing.error() ?? 'Finish or discard the current edit before changing assets.');
                }
            } else {
                await action();
            }
        } catch (error) {
            if (generation === this.editor.workspaceGeneration()) this.error.set(error instanceof Error ? error.message : String(error));
        } finally {
            this.pendingAction.set(null);
        }
    }

    private sameFile(a: AssetEntry, b: AssetEntry): boolean {
        if (a.kind !== b.kind) return false;
        if (a.kind === 'scene') return b.kind === 'scene' && a.sceneId === b.sceneId;
        if (b.kind === 'scene') return false;
        return a.asset_id && b.asset_id ? a.asset_id === b.asset_id : a.path === b.path;
    }
}
