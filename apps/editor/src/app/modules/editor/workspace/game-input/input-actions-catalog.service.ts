import {computed, effect, inject, Injectable, signal, untracked} from '@angular/core';
import {EditorService, type InputActionsScope} from '../../../../core/editor/editor.service';
import {ViewportEditorService} from '../../../../core/editor/viewport-editor.service';
import type {InputActionsAsset} from '../../../../core/game-input/input-actions.models';

@Injectable()
export class InputActionsCatalog {
    private readonly editor = inject(EditorService);
    private readonly viewport = inject(ViewportEditorService);
    private readonly entries = signal<readonly InputActionsAsset[]>([]);
    private readonly loading = signal(false);
    private readonly failure = signal<string | null>(null);
    private request = 0;
    private scopeKey = '';
    readonly assets = this.entries.asReadonly();
    readonly pending = this.loading.asReadonly();
    readonly error = this.failure.asReadonly();
    readonly supported = computed(() => this.editor.capabilities()?.input_action_assets === true);

    constructor() {
        effect(() => {
            const scope = this.scope();
            const key = scope ? `${scope.workspaceGeneration}:${scope.nativeGeneration}` : '';
            if (key === this.scopeKey) return;
            this.scopeKey = key;
            this.request++;
            this.entries.set([]);
            this.failure.set(null);
            this.loading.set(false);
            if (scope) untracked(() => void this.refresh());
        });
    }

    scope(): InputActionsScope | null {
        const snapshot = this.editor.snapshot();
        const nativeGeneration = this.viewport.state()?.workspace_generation;
        return this.supported() && snapshot && nativeGeneration != null ?
            {workspaceGeneration: this.editor.workspaceGeneration(), nativeGeneration, revision: snapshot.revision} : null;
    }

    isCurrent(scope: InputActionsScope): boolean {
        const current = this.scope();
        return current?.workspaceGeneration === scope.workspaceGeneration && current.nativeGeneration === scope.nativeGeneration;
    }

    async refresh(): Promise<void> {
        const scope = this.scope();
        if (!scope || this.pending()) return;
        const request = ++this.request;
        const current = () => request === this.request && this.isCurrent(scope);
        this.failure.set(null);
        this.loading.set(true);
        try {
            const assets = await this.editor.listInputActions(scope);
            if (current()) this.entries.set(assets);
        } catch (error) {
            if (current()) this.failure.set(error instanceof Error ? error.message : String(error));
        } finally {
            if (current()) this.loading.set(false);
        }
    }

    accept(asset: InputActionsAsset, scope: InputActionsScope): void {
        if (!this.isCurrent(scope)) return;
        this.request++;
        this.loading.set(false);
        this.entries.update(assets => [...assets.filter(item => item.id !== asset.id), asset]
            .sort((a, b) => a.path.localeCompare(b.path)));
    }
}
