import {Component, computed, effect, inject, input, signal} from '@angular/core';
import {form, FormField} from '@angular/forms/signals';
import type {ModelAsset} from '../../../../core/editor/editor.models';
import {EditorService} from '../../../../core/editor/editor.service';
import {EditorDialog} from '../../../../shared/components/dialog/dialog';
import {SceneEditingService} from '../scene-editing.service';

type FileAction = 'import' | 'move' | 'prefab';

@Component({
    selector: 'editor-asset-authoring',
    imports: [EditorDialog, FormField],
    templateUrl: './asset-authoring.html',
    styleUrls: ['../game-settings/settings.scss', './asset-authoring.scss'],
})
export class AssetAuthoring {
    readonly opening = input(false);
    readonly selected = input<ModelAsset | null>(null);
    protected readonly editor = inject(EditorService);
    private readonly editing = inject(SceneEditingService, {optional: true});
    protected readonly pending = signal<string | null>(null);
    protected readonly error = signal<string | null>(null);
    protected readonly status = signal<string | null>(null);
    protected readonly open = signal(false);
    protected readonly mode = signal<FileAction>('import');
    protected readonly draft = signal({path: ''});
    protected readonly fields = form(this.draft);
    protected readonly available = computed(() => this.editor.nativeAvailable && !!this.editor.snapshot() &&
        this.editor.snapshot()?.mode === 'edit' && !this.opening() && !this.pending() &&
        (this.editing?.ready() ?? (!this.editor.busy() && !this.editor.snapshot()?.transaction)));
    protected readonly selectedGlb = computed(() => this.selected()?.kind !== 'prefab' &&
        /\.glb$/i.test(this.selected()?.path ?? ''));
    protected readonly canReimport = computed(() => this.available() && this.selectedGlb() &&
        !!this.selected()?.asset_id && !!this.selected()?.fingerprint);
    protected readonly canMove = computed(() => this.available() && !!this.selected()?.asset_id &&
        (this.selectedGlb() || this.selected()?.kind === 'prefab'));
    protected readonly unsupportedLegacyModel = computed(() => !!this.selected() &&
        this.selected()?.kind !== 'prefab' && !this.selectedGlb());
    protected readonly title = computed(() => this.mode() === 'import' ? 'Import External GLB'
        : this.mode() === 'move' ? 'Move Asset' : 'Save Gameplay Prefab');
    protected readonly validPath = computed(() => {
        const value = this.draft().path.trim();
        return value.length > 0 && !/[:\\\p{Cc}]/u.test(value) && !value.startsWith('/') &&
            value.split('/').every(segment => segment !== '' && segment !== '.' && segment !== '..') &&
            (this.mode() === 'prefab' || (this.mode() === 'move' && this.selected()?.kind === 'prefab')
                ? value.endsWith('.json') : /\.glb$/i.test(value));
    });
    private source = '';
    private context: {generation: number; sceneId: string; asset: ModelAsset | null; selection: string} | null = null;

    constructor() {
        effect(() => {
            this.editor.workspaceGeneration();
            this.open.set(false);
            this.error.set(null);
            this.status.set(null);
        });
        effect(() => {
            if (this.editor.playing()) this.open.set(false);
        });
    }

    protected async chooseImport(): Promise<void> {
        if (!this.available()) return;
        const generation = this.editor.workspaceGeneration();
        this.pending.set('Choosing a GLB source…');
        this.error.set(null);
        try {
            const source = await this.editor.chooseExternalGlb();
            if (!source || generation !== this.editor.workspaceGeneration() || this.editor.playing()) return;
            this.source = source;
            this.show('import', source.split(/[\\/]/).at(-1) ?? 'model.glb', true);
        } catch (error) {
            if (generation === this.editor.workspaceGeneration()) this.error.set(String(error instanceof Error ? error.message : error));
        } finally {
            this.pending.set(null);
        }
    }

    protected show(mode: FileAction, path = '', chosenSource = false): void {
        const snapshot = this.editor.snapshot();
        if (!snapshot || snapshot.mode !== 'edit' || (!chosenSource && !this.available())) return;
        if (mode === 'move' && !this.canMove()) return;
        this.mode.set(mode);
        this.draft.set({path: path || (mode === 'prefab' ? 'gameplay.json' : this.selected()?.path ?? '')});
        this.context = {generation: this.editor.workspaceGeneration(), sceneId: snapshot.active_scene_id,
            asset: this.selected(), selection: JSON.stringify(snapshot.selection)};
        this.error.set(null);
        this.status.set(null);
        this.open.set(true);
    }

    protected async apply(): Promise<void> {
        const context = this.context;
        if (!context || !this.open() || !this.available() || !this.validPath()) return;
        const path = this.draft().path.trim();
        const mode = this.mode();
        await this.run(this.title(), context.generation, async () => {
            const snapshot = this.editor.snapshot()!;
            if (snapshot.active_scene_id !== context.sceneId) throw new Error('The scene changed. Choose the asset operation again.');
            if (mode === 'import') await this.editor.importExternalGlb(this.source, path, context.generation, snapshot.revision);
            else if (mode === 'move') {
                if (!context.asset?.asset_id) return;
                await this.editor.moveAsset(context.asset.asset_id, path, context.generation, snapshot.revision);
            } else {
                if (JSON.stringify(snapshot.selection) !== context.selection) throw new Error('The selection changed. Review it before saving a prefab.');
                await this.editor.savePrefab(path, context.generation, snapshot.revision);
            }
            if (context.generation !== this.editor.workspaceGeneration()) return;
            await this.editor.assets(true);
            if (context.generation !== this.editor.workspaceGeneration()) return;
            this.status.set(mode === 'prefab' ? 'Gameplay prefab saved.' : mode === 'move' ? 'Asset moved.' : 'External GLB imported.');
            this.open.set(false);
        });
    }

    protected async reimport(): Promise<void> {
        const asset = this.selected();
        if (!asset?.asset_id || !asset.fingerprint || !this.canReimport()) return;
        const generation = this.editor.workspaceGeneration();
        this.pending.set('Choosing a replacement GLB…');
        this.error.set(null);
        let source: string | null;
        try {
            source = await this.editor.chooseExternalGlb();
        } catch (error) {
            if (generation === this.editor.workspaceGeneration()) this.error.set(error instanceof Error ? error.message : String(error));
            return;
        } finally {
            this.pending.set(null);
        }
        if (!source || generation !== this.editor.workspaceGeneration()) return;
        await this.run('Reimporting GLB…', generation, async () => {
            await this.editor.reimportAsset(asset.asset_id!, asset.fingerprint!, source, generation, this.editor.snapshot()!.revision);
            if (generation !== this.editor.workspaceGeneration()) return;
            await this.editor.assets(true);
            if (generation !== this.editor.workspaceGeneration()) return;
            this.status.set('Asset reimported with its existing identity.');
        });
    }

    private async run(label: string, generation: number, action: () => Promise<void>): Promise<void> {
        if (!this.available() || generation !== this.editor.workspaceGeneration()) return;
        this.pending.set(label);
        this.error.set(null);
        this.status.set(null);
        const current = () => generation === this.editor.workspaceGeneration() && !this.editor.playing();
        try {
            const guarded = async () => { if (current()) await action(); };
            if (this.editing) {
                if (!await this.editing.withCommittedEdit(guarded) && current()) this.error.set(this.editing.error() ?? 'Finish the current edit and try again.');
            } else await guarded();
        } catch (error) {
            if (current()) this.error.set(error instanceof Error ? error.message : String(error));
        } finally {
            this.pending.set(null);
        }
    }
}
