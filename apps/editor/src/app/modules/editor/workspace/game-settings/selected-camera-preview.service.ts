import {afterNextRender, computed, DestroyRef, effect, inject, Injectable, Injector, signal, untracked} from '@angular/core';
import type {PreviewDefinition} from '../../../../core/editor/camera-preview';
import {EditorService} from '../../../../core/editor/editor.service';
import type {EntityKey} from '../../../../core/editor/editor.models';
import {ViewportEditorService} from '../../../../core/editor/viewport-editor.service';
import {SceneEditingService} from '../scene-editing.service';
import type {WorkspaceViewKind} from '../../../../core/editor/workspace/workspace-view';
import {WorkspaceViewService} from '../scene-viewport/workspace-view.service';

interface SelectedCameraLease {
    readonly name: string;
    readonly generation: number;
    readonly origin: HTMLElement | null;
    readonly view: WorkspaceViewKind;
    readonly definition: PreviewDefinition;
}

@Injectable()
export class SelectedCameraPreviewService {
    private readonly editor = inject(EditorService);
    private readonly editing = inject(SceneEditingService);
    private readonly views = inject(WorkspaceViewService);
    private readonly viewport = inject(ViewportEditorService);
    private readonly injector = inject(Injector);
    private readonly lease = signal<SelectedCameraLease | null>(null);
    readonly active = this.lease.asReadonly();
    private readonly legacyActive = signal(false);
    readonly available = computed(() => {
        const snapshot = this.editor.snapshot();
        const view = this.viewport.state();
        return this.editor.nativeAvailable && this.editing.ready() && !this.legacyActive() && !this.active() &&
            snapshot?.mode === 'edit' && snapshot.selection.length === 1 && this.views.activeViewKind() !== 'prop-workspace' &&
            view?.workspace_generation != null && !view.gesture && !view.placement && !this.viewport.pending();
    });

    constructor() {
        effect(() => {
            const lease = this.lease();
            if (!lease) return;
            const snapshot = this.editor.snapshot();
            const native = this.viewport.state();
            if (this.editor.workspaceGeneration() !== lease.generation ||
                snapshot?.mode !== 'edit' || snapshot.active_scene_id !== lease.definition.sceneId ||
                snapshot.revision !== lease.definition.expectedRevision || snapshot.transaction ||
                snapshot.selection.length !== 1 || snapshot.selection[0] !== lease.definition.cameraKey ||
                native?.workspace_generation !== lease.definition.expectedGeneration ||
                this.views.activeViewKind() !== lease.view) untracked(() => this.close(false));
        });
        inject(DestroyRef).onDestroy(() => this.close(false));
    }

    setLegacyActive(active: boolean): void {
        this.legacyActive.set(active);
    }

    async open(key: EntityKey, origin: HTMLElement | null): Promise<void> {
        if (!this.available()) return;
        const generation = this.editor.workspaceGeneration();
        const sceneId = this.editor.snapshot()?.active_scene_id;
        const view = this.views.activeViewKind();
        await this.editing.withCommittedEdit(async () => {
            const snapshot = this.editor.snapshot();
            const native = this.viewport.state();
            if (generation !== this.editor.workspaceGeneration() || snapshot?.mode !== 'edit' || snapshot.active_scene_id !== sceneId ||
                snapshot.transaction || snapshot.selection.length !== 1 || snapshot.selection[0] !== key ||
                native?.workspace_generation == null || native.gesture || native.placement ||
                this.views.activeViewKind() !== view || this.legacyActive() || this.active()) return;
            const camera = snapshot.document.entities.find(entity => entity.key === key && entity.camera?.enabled);
            if (!camera) return;
            const definition: PreviewDefinition = {
                cameraKey: key, expectedRevision: snapshot.revision, expectedGeneration: native.workspace_generation,
                sceneId: snapshot.active_scene_id,
            };
            this.lease.set({name: camera.name, generation, origin, view, definition});
            this.editor.cameraPreview.open(definition);
        });
    }

    close(restoreFocus = true): void {
        const lease = this.lease();
        if (!lease) return;
        this.lease.set(null);
        this.editor.cameraPreview.close(lease.definition);
        if (restoreFocus) afterNextRender(() => {
            if (lease.origin?.isConnected) lease.origin.focus({preventScroll: true});
        }, {injector: this.injector});
    }
}
