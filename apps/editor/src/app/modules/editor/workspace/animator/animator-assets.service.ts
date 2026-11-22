import {computed, effect, inject, Injectable, signal, untracked} from '@angular/core';
import {EditorService, type WorkspaceAssetScope} from '../../../../core/editor/editor.service';
import {ViewportEditorService} from '../../../../core/editor/viewport-editor.service';
import type {AnimatorAsset} from '../../../../core/editor/animator.models';

@Injectable()
export class AnimatorAssets {
    readonly editor = inject(EditorService);
    private readonly viewport = inject(ViewportEditorService, {optional: true});
    readonly supported = computed(() => this.editor.capabilities()?.animator_assets === true);
    private readonly catalog = signal<readonly AnimatorAsset[]>([]);
    readonly assets = this.catalog.asReadonly();
    readonly pending = signal(false);
    readonly error = signal<string | null>(null);
    private context = '';
    private request = 0;

    constructor() {
        effect(() => {
            const scope = this.scope();
            const context = scope ? `${scope.workspaceGeneration}:${scope.nativeGeneration}` : '';
            if (context === this.context) return;
            this.context = context;
            untracked(() => {
                this.request++;
                this.catalog.set([]);
                this.error.set(null);
                this.pending.set(false);
                if (scope) void this.refresh();
            });
        });
    }

    scope(): WorkspaceAssetScope | null {
        const snapshot = this.editor.snapshot();
        const nativeGeneration = this.viewport?.state()?.workspace_generation;
        return this.supported() && snapshot && nativeGeneration != null ?
            {workspaceGeneration: this.editor.workspaceGeneration(), nativeGeneration, revision: snapshot.revision} : null;
    }

    current(scope: WorkspaceAssetScope): boolean {
        const latest = this.scope();
        return latest?.workspaceGeneration === scope.workspaceGeneration && latest.nativeGeneration === scope.nativeGeneration;
    }

    async refresh(): Promise<void> {
        const scope = this.scope();
        if (!scope || this.pending()) return;
        const request = ++this.request;
        const current = () => this.current(scope) && request === this.request;
        this.pending.set(true);
        this.error.set(null);
        try {
            const entries = await this.editor.listAnimators(scope);
            if (current()) this.catalog.set(entries);
        } catch (error) {
            if (current()) this.error.set(error instanceof Error ? error.message : String(error));
        } finally {
            if (current()) this.pending.set(false);
        }
    }

    accept(asset: AnimatorAsset, scope: WorkspaceAssetScope): void {
        if (!this.current(scope)) return;
        this.request++;
        this.pending.set(false);
        this.catalog.update(assets => [...assets.filter(prior => prior.id !== asset.id), asset]
            .sort((a, b) => a.path.localeCompare(b.path)));
    }
}
