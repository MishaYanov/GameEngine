import {computed, effect, inject, signal, type Signal} from '@angular/core';
import {EditorService} from '../../../core/editor/editor.service';
import {ViewportEditorService} from '../../../core/editor/viewport-editor.service';
import {
    readWorkspaceLayoutScope, WORKSPACE_LAYOUT_SCOPE, type WorkspaceLayoutScope,
} from '../../../shared/services/layout-preferences-storage';

function workspaceLayoutScope(): Signal<WorkspaceLayoutScope> {
    const editor = inject(EditorService);
    const viewport = inject(ViewportEditorService);
    const hasWorkspace = computed(() => editor.snapshot() != null);
    const nativeGeneration = computed(() => viewport.state()?.workspace_generation ?? null);
    const resolved = signal<{local: number; native: number; scope: string | false} | null>(null);

    effect(onCleanup => {
        const local = editor.workspaceGeneration();
        const native = nativeGeneration();
        if (!editor.nativeAvailable || !hasWorkspace() || native === null) return;
        let active = true;
        onCleanup(() => { active = false; });
        void readWorkspaceLayoutScope(native).then(
            scope => { if (active) resolved.set({local, native, scope}); },
            () => { if (active) resolved.set({local, native, scope: false}); },
        );
    });

    return computed(() => {
        if (!editor.nativeAvailable || !hasWorkspace()) return null;
        const native = nativeGeneration();
        if (native === null) return false;
        const result = resolved();
        return result?.local === editor.workspaceGeneration() && result.native === native
            ? result.scope
            : undefined;
    });
}

export const WORKSPACE_LAYOUT_SCOPE_PROVIDER = {
    provide: WORKSPACE_LAYOUT_SCOPE,
    useFactory: workspaceLayoutScope,
};
