import {Component, computed, inject, input, output, signal} from '@angular/core';
import {CommandBuilder} from '../../../../core/editor/editor.models';
import {EditorService} from '../../../../core/editor/editor.service';
import type {WorkspaceViewKind} from '../../../../core/editor/workspace/workspace-view';
import {EditorIcon} from '../../../../shared/icons/editor-icon';
import {SceneEditingService} from '../scene-editing.service';
import {ViewportEditorService} from '../../../../core/editor/viewport-editor.service';

type WorkspaceAction = 'save' | 'saveAll' | 'undo' | 'redo' | 'play' | 'stop' | 'duplicate' | 'deleteSelection' | 'frameSelection';

@Component({
    selector: 'workspace-toolbar',
    imports: [EditorIcon],
    templateUrl: './workspace-toolbar.html',
    styleUrl: './workspace-toolbar.scss',
})
export class WorkspaceToolbar {
    readonly opening = input(false);
    readonly viewSelected = output<WorkspaceViewKind>();

    protected readonly editorService = inject(EditorService);
    protected readonly sceneEditing = inject(SceneEditingService, {optional: true});
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

    protected async runAction(action: WorkspaceAction): Promise<void> {
        const available = action === 'stop'
            ? this.canRunAction() && this.editorService.playing()
            : this.canEditScene();
        if (!available) return;

        this.pendingAction.set(true);
        this.actionError.set(null);
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
                await this.editorService.execute(CommandBuilder[action]());
                if (action === 'play') this.viewSelected.emit('game-preview');
                if (action === 'stop') this.viewSelected.emit('scene-viewport');
            };
            if (this.sceneEditing && action !== 'stop') await this.sceneEditing.withCommittedEdit(executeAction);
            else await executeAction();
        } catch (error) {
            this.actionError.set(error instanceof Error ? error.message : String(error));
        } finally {
            this.pendingAction.set(false);
        }
    }
}
