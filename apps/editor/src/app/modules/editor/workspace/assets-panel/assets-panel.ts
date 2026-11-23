import {
    afterNextRender, Component, computed, DestroyRef, effect, ElementRef, inject, Injector, input, linkedSignal, signal,
    untracked, viewChild,
} from '@angular/core';
import type {ModelAsset, SceneId} from '../../../../core/editor/editor.models';
import {EditorService} from '../../../../core/editor/editor.service';
import {Drawer} from '../../../../shared/components/drawer/drawer';
import type {DrawerSettings} from '../../../../shared/components/drawer/drawer-settings';
import {EditorPane} from '../../../../shared/components/pane/pane';
import {EditorIcon} from '../../../../shared/icons/editor-icon';
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
import {AnimatorAssets} from '../animator/animator-assets.service';
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
type AssetEntry = SceneAsset | (ModelAsset & {readonly kind: 'model' | 'prefab' | 'audio_clip'});
type AssetAction = 'models' | 'add';

interface AssetMenuContext {
    readonly file: AssetEntry;
    readonly generation: number;
    readonly sceneId: SceneId | null;
}

function searchable(text: string): string {
    return text.trim().replaceAll('\\', '/').toLocaleLowerCase();
}

@Component({
    selector: 'editor-assets-panel',
    imports: [Drawer, EditorPane, EditorIcon, AssetAuthoring, AudioClipDetails, PrefabAuthoring, EditorContextMenuTrigger],
    templateUrl: './assets-panel.html',
    styleUrl: './assets-panel.scss',
})
export class AssetsPanel {
    private readonly assetDrag = inject(ViewportAssetDragService, {optional: true});
    readonly opening = input(false);

    protected dragAudioClip(file: AssetEntry, event: DragEvent): void {
        if (file.kind === 'audio_clip' && file.asset_id) startAudioClipDrag(event, file.asset_id, this.editor.workspaceGeneration());
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
    private readonly animatorAssets = inject(AnimatorAssets, {optional: true});
    protected readonly animatorWorkspace = inject(AnimatorWorkspaceService, {optional: true});
    protected readonly controlsWorkspace = inject(ControlGraphWorkspaceService, {optional: true});
    private readonly drawer = viewChild(Drawer);
    private readonly propAuthoring = viewChild(PrefabAuthoring);
    private readonly injector = inject(Injector);
    private readonly element = inject<ElementRef<HTMLElement>>(ElementRef).nativeElement;
    private readonly destroy = inject(DestroyRef);
    private readonly sceneEditing = inject(SceneEditingService, {optional: true});
    private readonly sceneSwitch = inject(SceneSwitchService);
    protected readonly canOpenScene = computed(() => this.canRunAction() && this.sceneSwitch.canSwitch());
    protected readonly projectOpen = computed(() => this.editor.snapshot() !== null);
    protected readonly models = this.editor.modelAssets;
    protected readonly assetFilter = signal<'all' | 'audio_clip'>('all');
    protected readonly searchQuery = linkedSignal({source: this.editor.workspaceGeneration, computation: () => ''});
    private readonly query = computed(() => searchable(this.searchQuery()));
    protected readonly searching = computed(() => this.query().length > 0);
    protected readonly animators = computed(() => this.assetFilter() === 'all' && this.animatorAssets?.supported()
        ? this.animatorAssets.assets().filter(asset => searchable(asset.path).includes(this.query())) : []);
    protected readonly controlGraphs = computed(() => this.assetFilter() === 'all' && this.controlsWorkspace?.supported()
        ? this.controlsWorkspace.assets().filter(asset => searchable(asset.path).includes(this.query())) : []);
    protected readonly canRunAction = computed(() => {
        const snapshot = this.editor.snapshot();
        return snapshot?.mode === 'edit' && this.editor.nativeAvailable && !this.opening() &&
            !this.pendingAction() && (this.sceneEditing
                ? this.sceneEditing.ready()
                : !this.editor.busy() && snapshot.transaction === null);
    });
    protected readonly canLoadModels = this.canRunAction;
    protected readonly canOpenProp = computed(() => this.canRunAction() && this.propWorkspace?.canOpen() === true);
    private catalogWorkspace: number | null = null;
    private readonly scenes = computed(() => this.editor.snapshot()?.scenes);
    private readonly entries = computed<readonly AssetEntry[]>(() => [
        ...(this.scenes() ?? []).map(scene => ({
            path: scene.asset_path,
            name: scene.asset_path.split('/').at(-1) ?? scene.id,
            kind: 'scene' as const,
            sceneId: scene.id,
        })),
        ...(this.models() ?? []).flatMap(model => model.kind === 'sky_panorama'
            ? [] : [{...model, kind: model.kind ?? 'model' as const}]),
    ]);
    private readonly visibleEntries = computed(() => this.assetFilter() === 'audio_clip'
        ? this.entries().filter(entry => entry.kind === 'audio_clip') : this.entries());
    private readonly selection = linkedSignal({
        source: this.visibleEntries,
        computation: (entries, previous): AssetEntry | null => {
            const selected = previous?.value;
            if (!selected) return null;
            return entries.find(entry => this.sameFile(entry, selected)) ?? null;
        },
    });
    protected readonly selectedCatalogAsset = computed(() => {
        const selected = this.selected();
        return selected && selected.kind !== 'scene' ? selected : null;
    });
    protected readonly folder = linkedSignal({
        source: this.visibleEntries,
        computation: (entries, previous): string => {
            const folder = previous?.value ?? '';
            return entries.some(entry => entry.path.startsWith(`${folder}/`)) ? folder : '';
        },
    });
    protected readonly folderContents = computed(() => {
        const folder = this.folder();
        const query = this.query();
        const prefix = folder ? `${folder}/` : '';
        const folders = new Map<string, string>();
        const files: AssetEntry[] = [];
        for (const entry of this.visibleEntries()) {
            if (query) {
                if (searchable(entry.name).includes(query) || searchable(entry.path).includes(query)) files.push(entry);
                continue;
            }
            if (!entry.path.startsWith(prefix)) continue;
            const remainder = entry.path.slice(prefix.length);
            const separator = remainder.indexOf('/');
            if (separator === -1) {
                files.push(entry);
            } else {
                const name = remainder.slice(0, separator);
                folders.set(name, `${prefix}${name}`);
            }
        }
        return {
            folders: [...folders].sort(([a], [b]) => a.localeCompare(b)),
            files: files.sort((a, b) => a.name.localeCompare(b.name)),
        };
    });
    protected readonly selected = computed(() => {
        const selection = this.selection();
        return selection && this.folderContents().files.includes(selection) ? selection : null;
    });
    protected readonly noSearchMatches = computed(() => this.searching() &&
        !this.folderContents().files.length && !this.animators().length && !this.controlGraphs().length);
    private readonly assetMenuBuilder = ContextMenuBuilder.create<AssetMenuContext>()
        .action(ContextActionBuilder.create<AssetMenuContext>('asset.select', 'Select Asset')
            .icon('object')
            .enabledWhen(context => this.menuFile(context) !== null)
            .run(context => {
                const file = this.menuFile(context);
                if (file) this.selectFile(file);
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
            const workspace = this.editor.workspaceGeneration();
            if (workspace === this.catalogWorkspace || !this.canRunAction()) return;
            this.catalogWorkspace = workspace;
            if (this.models() === null) void this.loadCatalog(false);
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
        this.assetFilter.set('all');
        this.searchQuery.set('');
        this.folder.set(file.path.slice(0, Math.max(0, file.path.lastIndexOf('/'))));
        this.selectFile(file);
        afterNextRender(() => {
            if (!this.ownsReveal(request) || this.selectedCatalogAsset()?.asset_id !== request.sourceId) return;
            const row = [...this.element.querySelectorAll<HTMLButtonElement>('button[data-asset-id]')]
                .find(button => button.dataset['assetId'] === request.sourceId);
            row?.focus();
        }, {injector: this.injector});
    }

    protected upOneFolder(location: HTMLElement): void {
        const path = this.folder();
        this.openFolder(path.slice(0, Math.max(0, path.lastIndexOf('/'))), location);
    }

    protected filterAssets(value: string): void {
        this.assetFilter.set(value === 'audio_clip' ? 'audio_clip' : 'all');
    }

    protected openFolder(path: string, location: HTMLElement): void {
        this.folder.set(path);
        this.selection.set(null);
        location.focus();
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
        if (file.kind === 'prefab' || file.kind === 'audio_clip') return false;
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

    protected isActiveScene(file: AssetEntry): boolean {
        return file.kind === 'scene' && file.sceneId === this.editor.snapshot()?.active_scene_id;
    }

    protected async loadModels(): Promise<void> {
        if (this.canLoadModels()) await this.loadCatalog(true);
    }

    protected async openScene(): Promise<void> {
        const selected = this.selected();
        if (selected?.kind === 'scene') await this.openSceneFile(selected);
    }

    protected async addModel(): Promise<void> {
        const selected = this.selected();
        if (!selected || selected.kind === 'scene') return;
        await this.addModelFile(selected);
    }

    private async addModelFile(selected: Exclude<AssetEntry, SceneAsset>): Promise<void> {
        if (selected.kind === 'audio_clip') return;
        const generation = this.editor.workspaceGeneration();
        const sceneId = this.editor.snapshot()?.active_scene_id;
        await this.addAsset(async () => {
            if (generation !== this.editor.workspaceGeneration() || sceneId !== this.editor.snapshot()?.active_scene_id) return;
            const file = this.entries().find(entry => this.sameFile(entry, selected));
            if (!file || file.kind === 'scene' || file.kind === 'audio_clip') return;
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
            if (refresh && workspace === this.editor.workspaceGeneration()) this.status.set('Model catalogue refreshed.');
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
