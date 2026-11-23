import {Component, computed, inject, input, output, signal} from '@angular/core';
import type {WorkspaceAction} from '../../../../core/editor/editor-actions';
import {editorShortcutLabel} from '../../../../core/editor/editor-keymap';
import {CommandBuilder} from '../../../../core/editor/editor.models';
import {EditorService} from '../../../../core/editor/editor.service';
import type {WorkspaceViewKind} from '../../../../core/editor/workspace/workspace-view';
import {EditorIcon} from '../../../../shared/icons/editor-icon';
import {SceneEditingService} from '../scene-editing.service';
import {WorkspaceDraftReview} from '../workspace-draft-review';
import {ViewportEditorService} from '../../../../core/editor/viewport-editor.service';

@Component({
    selector: 'workspace-toolbar',
    imports: [EditorIcon],
    templateUrl: './workspace-toolbar.html',
    styleUrl: './workspace-toolbar.scss',
})
export class WorkspaceToolbar {
    readonly opening = input(false);
    readonly sceneActionsVisible = input(true);
    readonly paletteAvailable = input(false);
    readonly paletteRequested = output<void>();
    readonly viewSelected = output<WorkspaceViewKind>();

    protected readonly paletteShortcut = editorShortcutLabel('palette.open');
    protected readonly editorService = inject(EditorService);
    protected readonly sceneEditing = inject(SceneEditingService, {optional: true});
    private readonly drafts = inject(WorkspaceDraftReview);
    private readonly viewportEditor = inject(ViewportEditorService, {optional: true});
    protected readonly actionError = signal<string | null>(null);
    protected readonly canEditScene = computed(() => {
        const snapshot = this.editorService.snapshot();
        return this.canRunAction() && snapshot?.mode === 'edit' &&
            (snapshot.transaction === null || this.sceneEditing?.ownsTransaction());
    });
    private readonly pendingAction = signal(false);
    protected readonly canRunAction = computed(() =>
        this.editorService.nativeAvailable &&
        !this.opening() &&
        !this.pendingAction() &&
        (!this.editorService.busy() || this.sceneEditing?.ready() === true),
    );

    dispatch(action: WorkspaceAction): boolean {
        if (!this.canPerform(action)) return false;
        void this.runAction(action);
        return true;
    }

    canPerform(action: WorkspaceAction): boolean {
        if (action === 'stop') return this.canRunAction() && this.editorService.playing();
        if (!this.canEditScene() || this.sceneEditing?.ready() === false) return false;
        if (!this.sceneActionsVisible() && action !== 'play') return false;
        switch (action) {
            case 'save':
            case 'saveAll':
                return this.editorService.dirty() || this.sceneEditing?.hasChanges() === true;
            case 'undo':
                return this.editorService.snapshot()?.can_undo === true || this.sceneEditing?.hasChanges() === true;
            case 'redo':
                return this.editorService.snapshot()?.can_redo === true;
            case 'selectAll':
                return !!this.editorService.document()?.entities.length;
            case 'duplicate':
            case 'deleteSelection':
            case 'frameSelection':
                return this.editorService.selection().length > 0;
            case 'play':
                return true;
        }
    }

    private async runAction(action: WorkspaceAction): Promise<void> {
        this.pendingAction.set(true);
        this.actionError.set(null);
        const playTarget = action === 'play' ? this.drafts.capture() : null;
        try {
            const executeAction = async () => {
                const snapshot = this.editorService.snapshot();
                if (action !== 'stop' && (snapshot?.mode !== 'edit' || snapshot.transaction !== null)) return;
                if (action === 'save' || action === 'saveAll') {
                    await this.editorService[action]();
                    return;
                }
                if (action === 'frameSelection') {
                    if (this.viewportEditor) await this.viewportEditor.action({type: 'frame_selection'});
                    return;
                }
                if (action === 'selectAll') {
                    const limit = this.editorService.capabilities()?.max_selection ?? 4096;
                    const keys = snapshot?.document.entities.slice(0, limit).map(entity => entity.key) ?? [];
                    await this.editorService.execute(CommandBuilder.select(keys));
                    return;
                }
                if (playTarget) {
                    if (!snapshot || this.opening() || !this.drafts.current(playTarget)) return;
                    await this.editorService.execute(CommandBuilder.play(), playTarget.generation, snapshot.revision);
                    if (this.drafts.current(playTarget)) this.viewSelected.emit('game-preview');
                    return;
                }
                await this.editorService.execute(CommandBuilder[action]());
                if (action === 'stop') this.viewSelected.emit('scene-viewport');
            };
            const dispatch = async () => {
                if (this.sceneEditing && action !== 'stop') await this.sceneEditing.withCommittedEdit(executeAction);
                else await executeAction();
            };
            if (playTarget) await this.drafts.run(playTarget, dispatch);
            else await dispatch();
        } catch (error) {
            this.actionError.set(error instanceof Error ? error.message : String(error));
        } finally {
            this.pendingAction.set(false);
        }
    }
}
