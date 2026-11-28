import {afterNextRender, Component, computed, DestroyRef, effect, inject, Injector, input, signal} from '@angular/core';
import {form, FormField} from '@angular/forms/signals';
import type {ModelAsset, ProjectAssetFile} from '../../../../core/editor/editor.models';
import {EditorService, type WorkspaceAssetScope} from '../../../../core/editor/editor.service';
import {ViewportEditorService} from '../../../../core/editor/viewport-editor.service';
import {EditorDialog} from '../../../../shared/components/dialog/dialog';
import {EditorIcon} from '../../../../shared/icons/editor-icon';
import {SceneEditingService} from '../scene-editing.service';

type FileAction = 'import' | 'import-audio' | 'import-sprite' | 'move' | 'prefab' | 'rust-script';

const SCRIPT_TEMPLATE = `use engine_script_sdk::Script;

#[derive(Default)]
struct Behavior;

impl Script for Behavior {}

engine_script_sdk::export_script!(Behavior);
`;

@Component({
    selector: 'editor-asset-authoring',
    imports: [EditorDialog, EditorIcon, FormField],
    templateUrl: './asset-authoring.html',
    styleUrls: ['../game-settings/settings.scss', './asset-authoring.scss'],
})
export class AssetAuthoring {
    readonly opening = input(false);
    readonly selected = input<ModelAsset | null>(null);
    readonly selectedScript = input<ProjectAssetFile | null>(null);
    protected readonly editor = inject(EditorService);
    private readonly viewport = inject(ViewportEditorService, {optional: true});
    private readonly editing = inject(SceneEditingService, {optional: true});
    private readonly injector = inject(Injector);
    private readonly destroy = inject(DestroyRef);
    protected readonly pending = signal<string | null>(null);
    protected readonly error = signal<string | null>(null);
    protected readonly status = signal<string | null>(null);
    protected readonly open = signal(false);
    protected readonly mode = signal<FileAction>('import');
    protected readonly draft = signal({path: ''});
    protected readonly fields = form(this.draft);
    protected readonly scriptText = signal(SCRIPT_TEMPLATE);
    protected readonly scriptReplacement = signal<{ id: string; fingerprint: string } | null>(null);
    protected readonly available = computed(() => this.editor.nativeAvailable && !!this.editor.snapshot() &&
        this.editor.snapshot()?.mode === 'edit' && !this.opening() && !this.pending() &&
        (this.editing?.ready() ?? (!this.editor.busy() && !this.editor.snapshot()?.transaction)));
    protected readonly availableAssetScope = computed(() => this.available() && this.scope() !== null);
    protected readonly selectedGlb = computed(() => (this.selected()?.kind ?? 'model') === 'model' &&
        /\.glb$/i.test(this.selected()?.path ?? ''));
    protected readonly canReimport = computed(() => this.availableAssetScope() && this.selectedGlb() &&
        !!this.selected()?.asset_id && !!this.selected()?.fingerprint);
    protected readonly canMove = computed(() => this.availableAssetScope() && !!this.selected()?.asset_id &&
        (this.selectedGlb() || this.selected()?.kind === 'prefab' || this.selected()?.kind === 'audio_clip'));
    protected readonly unsupportedLegacyModel = computed(() => !!this.selected() &&
        (this.selected()?.kind ?? 'model') === 'model' && !this.selectedGlb());
    protected readonly title = computed(() => {
        switch (this.mode()) {
            case 'import': return 'Import External GLB';
            case 'import-audio': return 'Import Audio Clip';
            case 'import-sprite':
                return 'Import Particle Sprite';
            case 'rust-script':
                return this.scriptReplacement() ? 'Edit Rust Script' : 'Create Rust Script';
            case 'move': return 'Move Asset';
            case 'prefab': return 'Save Gameplay Prefab';
        }
    });
    protected readonly destinationExtension = computed(() => {
        const mode = this.mode();
        const kind = this.selected()?.kind;
        if (mode === 'prefab' || (mode === 'move' && kind === 'prefab')) return '.json';
        if (mode === 'import-sprite') return '.png';
        if (mode === 'rust-script') return '.rs';
        return mode === 'import-audio' || (mode === 'move' && kind === 'audio_clip') ? '.wav' : '.glb';
    });
    protected readonly validPath = computed(() => {
        const value = this.draft().path.trim();
        return value.length > 0 && !/[:\\\p{Cc}]/u.test(value) && !value.startsWith('/') &&
            value.split('/').every(segment => segment !== '' && segment !== '.' && segment !== '..') &&
            (this.destinationExtension() === '.json' ? value : value.toLowerCase()).endsWith(this.destinationExtension());
    });
    private source = '';
    private context: {generation: number; nativeGeneration: number | null; sceneId: string; asset: ModelAsset | null; selection: string} | null = null;

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

    protected createScript(): void {
        this.scriptText.set(SCRIPT_TEMPLATE);
        this.scriptReplacement.set(null);
        this.show('rust-script', 'behavior.rs');
    }

    protected async editScript(): Promise<void> {
        const scope = this.scope();
        const id = this.selectedScript()?.asset_id;
        if (!scope || !id || !this.available()) return;
        this.pending.set('Opening Rust script…');
        this.error.set(null);
        try {
            const source = await this.editor.openRustScript(id, scope);
            if (!this.currentScope(scope) || this.editor.playing()) return;
            this.scriptText.set(source.source);
            this.scriptReplacement.set({id, fingerprint: source.fingerprint});
            this.show('rust-script', source.path, true);
        } catch (error) {
            if (this.currentScope(scope)) this.error.set(error instanceof Error ? error.message : String(error));
        } finally {
            this.pending.set(null);
        }
    }

    protected async chooseImport(format: 'glb' | 'wav' | 'png' = 'glb', opener?: HTMLButtonElement): Promise<void> {
        const scope = this.scope();
        if (!this.available() || !scope) return;
        const generation = scope.workspaceGeneration;
        this.pending.set(`Choosing a ${format.toUpperCase()} source…`);
        this.error.set(null);
        try {
            const source = await (format === 'png' ? this.editor.chooseParticlePng(scope)
                : format === 'wav' ? this.editor.chooseExternalWav(scope) : this.editor.chooseExternalGlb(scope));
            if (!source || !this.currentScope(scope) || this.editor.playing()) return;
            this.source = source;
            const mode = format === 'png' ? 'import-sprite' : format === 'wav' ? 'import-audio' : 'import';
            this.show(mode, source.split(/[\\/]/).at(-1) ?? `asset.${format}`, true);
        } catch (error) {
            if (generation === this.editor.workspaceGeneration()) this.error.set(String(error instanceof Error ? error.message : error));
        } finally {
            this.pending.set(null);
            if (opener && !this.destroy.destroyed && !this.open() && generation === this.editor.workspaceGeneration()) {
                afterNextRender(() => {
                    if (!this.open() && generation === this.editor.workspaceGeneration() && opener.isConnected) opener.focus();
                }, {injector: this.injector});
            }
        }
    }

    protected show(mode: FileAction, path = '', chosenSource = false): void {
        const snapshot = this.editor.snapshot();
        if (!snapshot || snapshot.mode !== 'edit' || (!chosenSource && !this.available())) return;
        if (mode === 'move' && !this.canMove()) return;
        this.mode.set(mode);
        this.draft.set({path: path || (mode === 'prefab' ? 'gameplay.json' : this.selected()?.path ?? '')});
        this.context = {generation: this.editor.workspaceGeneration(), nativeGeneration: this.scope()?.nativeGeneration ?? null,
            sceneId: snapshot.active_scene_id,
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
            if (mode === 'prefab') {
                if (JSON.stringify(snapshot.selection) !== context.selection) throw new Error('The selection changed. Review it before saving a prefab.');
                await this.editor.savePrefab(path, context.generation, snapshot.revision);
            } else {
                const scope = this.scope();
                if (!scope || scope.nativeGeneration !== context.nativeGeneration || scope.workspaceGeneration !== context.generation) {
                    throw new Error('The workspace changed. Choose the asset operation again.');
                }
                if (mode === 'import') await this.editor.importExternalGlb(this.source, path, scope);
                else if (mode === 'import-audio') await this.editor.importExternalWav(this.source, path, scope);
                else if (mode === 'import-sprite') await this.editor.importParticleSprite(this.source, path, scope);
                else if (mode === 'rust-script') await this.editor.saveRustScript(path, this.scriptText(), this.scriptReplacement(), scope);
                else if (context.asset?.asset_id) await this.editor.moveAsset(context.asset.asset_id, path, scope);
                else return;
            }
            if (context.generation !== this.editor.workspaceGeneration()) return;
            await this.editor.assets(true);
            if (context.generation !== this.editor.workspaceGeneration()) return;
            this.status.set(mode === 'prefab' ? 'Gameplay prefab saved.' : mode === 'move' ? 'Asset moved.'
                : mode === 'import-audio' ? 'Audio Clip imported.' : mode === 'import-sprite' ? 'Particle sprite imported.'
                    : mode === 'rust-script' ? 'Rust script saved.' : 'External GLB imported.');
            this.open.set(false);
        });
    }

    protected async reimport(): Promise<void> {
        const asset = this.selected();
        const scope = this.scope();
        if (!asset?.asset_id || !asset.fingerprint || !scope || !this.canReimport()) return;
        const {asset_id, fingerprint} = asset;
        const generation = scope.workspaceGeneration;
        this.pending.set('Choosing a replacement GLB…');
        this.error.set(null);
        let source: string | null;
        try {
            source = await this.editor.chooseExternalGlb(scope);
        } catch (error) {
            if (generation === this.editor.workspaceGeneration()) this.error.set(error instanceof Error ? error.message : String(error));
            return;
        } finally {
            this.pending.set(null);
        }
        if (!source || !this.currentScope(scope)) return;
        const sourcePath = source;
        await this.run('Reimporting GLB…', generation, async () => {
            const current = this.scope();
            if (!current || !this.currentScope(scope)) return;
            await this.editor.reimportAsset(asset_id, fingerprint, sourcePath, current);
            if (generation !== this.editor.workspaceGeneration()) return;
            await this.editor.assets(true);
            if (generation !== this.editor.workspaceGeneration()) return;
            this.status.set('Asset reimported with its existing identity.');
        });
    }

    private scope(): WorkspaceAssetScope | null {
        const nativeGeneration = this.viewport?.state()?.workspace_generation;
        const snapshot = this.editor.snapshot();
        return nativeGeneration == null || !snapshot ? null : {
            workspaceGeneration: this.editor.workspaceGeneration(), nativeGeneration, revision: snapshot.revision,
        };
    }

    private currentScope(scope: WorkspaceAssetScope): boolean {
        const current = this.scope();
        return current?.workspaceGeneration === scope.workspaceGeneration && current.nativeGeneration === scope.nativeGeneration;
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
