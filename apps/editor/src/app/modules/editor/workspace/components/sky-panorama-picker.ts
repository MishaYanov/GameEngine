import {Component, computed, DestroyRef, effect, inject, input, output, signal, untracked} from '@angular/core';
import {form, FormField} from '@angular/forms/signals';
import {EditorService} from '../../../../core/editor/editor.service';
import {ViewportEditorService} from '../../../../core/editor/viewport-editor.service';
import {validPanoramaPath, type SkyPanoramaAsset, type SkyPanoramaScope} from '../../../../core/editor/sky.models';
import {EditorSearchPicker} from '../../../../shared/components/search-picker/search-picker';

@Component({
    selector: 'editor-sky-panorama-picker',
    imports: [EditorSearchPicker, FormField],
    templateUrl: './sky-panorama-picker.html',
    styles: [':host, .import-fields {display: grid; gap: .45rem} :host {padding-block: .45rem} .actions {display: flex; gap: .5rem; flex-wrap: wrap}'],
})
export class SkyPanoramaPicker {
    readonly image = input('');
    readonly context = input('');
    readonly editable = input(false);
    readonly review = input(false);
    readonly picked = output<string>();

    private readonly editor = inject(EditorService);
    private readonly viewport = inject(ViewportEditorService, {optional: true});
    private readonly destroy = inject(DestroyRef);
    private readonly assets = signal<readonly SkyPanoramaAsset[] | null>(null);
    private readonly owner = computed(() => {
        const scene = this.editor.snapshot()?.active_scene_id;
        const nativeGeneration = this.viewport?.state()?.workspace_generation;
        return scene && nativeGeneration != null && this.editor.nativeAvailable
            ? JSON.stringify([this.editor.workspaceGeneration(), nativeGeneration, scene, this.context()]) : null;
    });
    private request = 0;

    protected readonly loading = signal(false);
    protected readonly pending = signal(false);
    protected readonly failure = signal<string | null>(null);
    protected readonly source = signal<string | null>(null);
    protected readonly sourceName = computed(() => this.source()?.split(/[\\/]/).at(-1) ?? '');
    protected readonly path = signal({destination: ''});
    protected readonly pathForm = form(this.path);
    protected readonly available = computed(() => this.editable() && this.owner() !== null && !this.pending() &&
        this.editor.snapshot()?.mode === 'edit' && !this.editor.snapshot()?.transaction && !this.editor.busy());
    protected readonly canImport = computed(() => this.available() && validPanoramaPath(this.path().destination.trim()));
    protected readonly options = computed(() => (this.assets() ?? []).map(asset => ({
        id: asset.id, label: asset.path, description: `${asset.width} × ${asset.height} PNG`,
    })));
    protected readonly selected = computed(() => this.assets()?.find(asset => asset.id === this.image()));
    protected readonly missing = computed(() =>
        !this.loading() && !!this.image() && this.assets() !== null && !this.selected(),
    );

    constructor() {
        effect(() => {
            const owner = this.owner();
            untracked(() => {
                this.request++;
                this.assets.set(null);
                this.source.set(null);
                this.failure.set(null);
                this.pending.set(false);
                this.loading.set(false);
                if (owner) void this.refresh();
            });
        });
    }

    protected async refresh(): Promise<void> {
        const owner = this.owner();
        const scope = this.scope();
        if (!owner || !scope || this.loading()) return;
        const request = ++this.request;
        this.loading.set(true);
        this.failure.set(null);
        try {
            const assets = await this.editor.listSkyPanoramas(scope);
            if (this.current(owner, request)) this.assets.set(assets);
        } catch (error) {
            if (this.current(owner, request)) this.failure.set(this.message(error));
        } finally {
            if (this.current(owner, request)) this.loading.set(false);
        }
    }

    protected choose(image: string): void {
        if (this.available() && (image === '' || this.assets()?.some(asset => asset.id === image))) {
            this.picked.emit(image);
        }
    }

    protected async chooseSource(): Promise<void> {
        const owner = this.owner();
        if (!owner || !this.available()) return;
        this.pending.set(true);
        this.failure.set(null);
        try {
            const path = await this.editor.chooseExternalSkyPng();
            if (path && this.current(owner) && this.editor.snapshot()?.mode === 'edit') {
                this.source.set(path);
                this.path.set({destination: path.split(/[\\/]/).at(-1) ?? 'panorama.png'});
            }
        } catch (error) {
            if (this.current(owner)) this.failure.set(this.message(error));
        } finally {
            if (this.current(owner)) this.pending.set(false);
        }
    }

    protected cancelImport(): void {
        this.source.set(null);
        this.failure.set(null);
    }

    protected async importAndAssign(): Promise<void> {
        const owner = this.owner();
        const scope = this.scope();
        const source = this.source();
        if (!owner || !scope || !source || !this.canImport()) return;
        this.request++;
        this.loading.set(false);
        this.pending.set(true);
        this.failure.set(null);
        const destination = this.path().destination.trim();
        try {
            const result = await this.editor.importSkyPanorama(source, destination, scope);
            if (!this.current(owner) || this.editor.snapshot()?.mode !== 'edit') return;
            const next = this.scope();
            if (!next) return;
            this.source.set(null);
            const assets = await this.editor.listSkyPanoramas(next);
            if (!this.current(owner) || this.editor.snapshot()?.mode !== 'edit') return;
            this.assets.set(assets);
            const imported = assets.find(asset => asset.id === result.asset.asset_id || asset.path === destination);
            if (!imported) throw new Error('The imported panorama is unavailable. Refresh the catalog to review it.');
            this.picked.emit(imported.id);
        } catch (error) {
            if (this.current(owner)) this.failure.set(this.message(error));
        } finally {
            if (this.current(owner)) this.pending.set(false);
        }
    }

    private scope(): SkyPanoramaScope | null {
        const nativeGeneration = this.viewport?.state()?.workspace_generation;
        const snapshot = this.editor.snapshot();
        return nativeGeneration == null || !snapshot ? null : {
            nativeGeneration, revision: snapshot.revision, workspaceGeneration: this.editor.workspaceGeneration(),
        };
    }

    private current(owner: string, request?: number): boolean {
        return !this.destroy.destroyed && owner === this.owner() && (request === undefined || request === this.request);
    }

    private message(error: unknown): string {
        return error instanceof Error ? error.message : String(error);
    }
}
