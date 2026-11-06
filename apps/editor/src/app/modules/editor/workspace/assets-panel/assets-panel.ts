import {Component, computed, inject, input, linkedSignal, signal} from '@angular/core';
import type {SceneId} from '../../../../core/editor/editor.models';
import {EditorService} from '../../../../core/editor/editor.service';
import {Drawer} from '../../../../shared/components/drawer/drawer';
import type {DrawerSettings} from '../../../../shared/components/drawer/drawer-settings';
import {EditorPane} from '../../../../shared/components/pane/pane';
import {EditorIcon} from '../../../../shared/icons/editor-icon';
import type {Config} from '../../../../shared/models/config.model';
import {SceneEditingService} from '../scene-editing.service';
import {ViewportAssetDragService} from '../viewport-asset-drag.service';

interface AssetFile {
    readonly path: string;
    readonly name: string;
}

type AssetEntry = AssetFile & ({readonly kind: 'scene'; readonly sceneId: SceneId} | {readonly kind: 'model'});
type AssetAction = 'models' | 'open' | 'add';

@Component({
    selector: 'editor-assets-panel',
    imports: [Drawer, EditorPane, EditorIcon],
    templateUrl: './assets-panel.html',
    styleUrl: './assets-panel.scss',
})
export class AssetsPanel {
    protected readonly modelDrag = inject(ViewportAssetDragService, {optional: true});
    readonly opening = input(false);

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
    private readonly sceneEditing = inject(SceneEditingService, {optional: true});
    protected readonly projectOpen = computed(() => this.editor.snapshot() !== null);
    protected readonly models = this.editor.modelAssets;
    protected readonly canRunAction = computed(() => {
        const snapshot = this.editor.snapshot();
        return snapshot?.mode === 'edit' && this.editor.nativeAvailable && !this.opening() &&
            !this.pendingAction() && (this.sceneEditing
                ? this.sceneEditing.ready()
                : !this.editor.busy() && snapshot.transaction === null);
    });
    protected readonly canLoadModels = this.canRunAction;
    private readonly scenes = computed(() => this.editor.snapshot()?.scenes);
    private readonly entries = computed<readonly AssetEntry[]>(() => [
        ...(this.scenes() ?? []).map(scene => ({
            path: scene.asset_path,
            name: scene.asset_path.split('/').at(-1) ?? scene.id,
            kind: 'scene' as const,
            sceneId: scene.id,
        })),
        ...(this.models() ?? []).map(model => ({...model, kind: 'model' as const})),
    ]);
    protected readonly selected = linkedSignal({
        source: this.entries,
        computation: (entries, previous): AssetEntry | null => {
            const selected = previous?.value;
            if (!selected) return null;
            return entries.find(entry => this.sameFile(entry, selected)) ?? null;
        },
    });
    protected readonly folder = linkedSignal({
        source: this.entries,
        computation: (entries, previous): string => {
            const folder = previous?.value ?? '';
            return entries.some(entry => entry.path.startsWith(`${folder}/`)) ? folder : '';
        },
    });
    protected readonly folderContents = computed(() => {
        const folder = this.folder();
        const prefix = folder ? `${folder}/` : '';
        const folders = new Map<string, string>();
        const files: AssetEntry[] = [];
        for (const entry of this.entries()) {
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

    protected upOneFolder(location: HTMLElement): void {
        const path = this.folder();
        this.openFolder(path.slice(0, Math.max(0, path.lastIndexOf('/'))), location);
    }

    protected openFolder(path: string, location: HTMLElement): void {
        this.folder.set(path);
        this.selected.set(null);
        location.focus();
    }

    protected selectFile(file: AssetEntry): void {
        this.selected.set(file);
        this.error.set(null);
        this.status.set(null);
    }

    protected beginModelDrag(file: AssetEntry, event: PointerEvent): void {
        if (file.kind === 'model' && this.canRunAction()) this.modelDrag?.candidate(file.path, event);
    }

    protected isSelected(file: AssetEntry): boolean {
        const selected = this.selected();
        return selected !== null && this.sameFile(file, selected);
    }

    protected isActiveScene(file: AssetEntry): boolean {
        return file.kind === 'scene' && file.sceneId === this.editor.snapshot()?.active_scene_id;
    }

    protected async loadModels(): Promise<void> {
        await this.runAction('models', async () => {
            await this.editor.assets(true);
            this.status.set('Model catalogue refreshed.');
        });
    }

    protected async openScene(): Promise<void> {
        const selected = this.selected();
        if (selected?.kind !== 'scene' || this.isActiveScene(selected)) return;
        await this.runAction('open', async () => {
            await this.editor.switchScene(selected.sceneId);
            this.status.set(`Opened ${selected.name}.`);
        });
    }

    protected async addModel(): Promise<void> {
        const selected = this.selected();
        if (selected?.kind !== 'model') return;
        await this.runAction('add', async () => {
            await this.editor.importModel(selected.path, [0, 0, 0]);
            this.status.set(`Added ${selected.name} to the scene.`);
        });
    }

    private async runAction(kind: AssetAction, action: () => Promise<void>): Promise<void> {
        if (!this.canRunAction()) return;
        this.pendingAction.set(kind);
        this.error.set(null);
        this.status.set(null);
        try {
            if (this.sceneEditing) {
                if (!await this.sceneEditing.withCommittedEdit(action)) {
                    this.error.set(this.sceneEditing.error() ?? 'Finish or discard the current edit before changing assets.');
                }
            } else {
                await action();
            }
        } catch (error) {
            this.error.set(error instanceof Error ? error.message : String(error));
        } finally {
            this.pendingAction.set(null);
        }
    }

    private sameFile(a: AssetEntry, b: AssetEntry): boolean {
        return a.kind === b.kind && a.path === b.path &&
            (a.kind !== 'scene' || (b.kind === 'scene' && a.sceneId === b.sceneId));
    }
}
