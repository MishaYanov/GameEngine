import {Component, computed, inject, input, signal} from '@angular/core';
import {disabled, form, FormField, required} from '@angular/forms/signals';
import {EditorService} from '../../../../core/editor/editor.service';
import {EditorDialog} from '../../../../shared/components/dialog/dialog';
import {SceneEditingService} from '../scene-editing.service';
import {SceneSwitchService} from './scene-switch.service';

@Component({
    selector: 'editor-scene-actions',
    imports: [EditorDialog, FormField],
    templateUrl: './scene-actions.html',
    styleUrl: './scene-actions.scss',
})
export class SceneActions {
    readonly opening = input(false);
    protected readonly editor = inject(EditorService);
    protected readonly sceneSwitch = inject(SceneSwitchService);
    private readonly editing = inject(SceneEditingService, {optional: true});
    private readonly pending = signal(false);
    protected readonly expanded = signal(false);
    protected readonly message = signal<string | null>(null);
    protected readonly error = signal<string | null>(null);
    protected readonly confirmationConfig = {draggable: false, resizable: false, expandable: false};
    protected readonly available = computed(() => this.editor.nativeAvailable && !!this.editor.snapshot() &&
        !this.opening() && !this.pending() && !this.sceneSwitch.pending() && (this.editing?.ready() ??
            (!this.editor.busy() && !this.editor.playing() && !this.editor.snapshot()?.transaction)));
    private readonly pathDraft = signal({path: ''});
    protected readonly fields = form(this.pathDraft, path => {
        required(path.path);
        disabled(path.path, () => !this.available());
    });
    protected readonly validPath = computed(() => {
        const path = this.pathDraft().path.trim();
        return path.endsWith('.json') && !path.startsWith('/') && !/[:\\\p{Cc}]/u.test(path) &&
            path.split('/').every(part => part !== '' && part !== '.' && part !== '..');
    });

    protected switchScene(select: HTMLSelectElement): void {
        const id = select.value;
        select.value = this.editor.snapshot()?.active_scene_id ?? '';
        if (!this.available()) return;
        this.error.set(null);
        this.message.set(null);
        void this.sceneSwitch.request(id);
    }

    protected async setStartup(): Promise<void> {
        await this.run(async () => {
            const id = this.editor.snapshot()!.active_scene_id;
            await this.editor.execute({type: 'set_startup_scene', scene_id: id});
            this.message.set('Startup scene changed. Save All to persist the project settings.');
        });
    }

    protected async sceneFile(action: 'register' | 'saveAs'): Promise<void> {
        if (!this.validPath()) return;
        const path = this.pathDraft().path.trim();
        await this.run(async () => {
            if (action === 'register') await this.editor.registerScene(path);
            else await this.editor.saveSceneAs(path);
            this.message.set(action === 'register'
                ? 'Scene registered. Save All to persist the project settings.'
                : 'Scene saved at the new path. Save All to persist the project settings.');
            this.pathDraft.set({path: ''});
        });
    }

    private async run(action: () => Promise<void>): Promise<void> {
        if (!this.available()) return;
        this.pending.set(true);
        this.sceneSwitch.clearFeedback();
        this.error.set(null);
        this.message.set(null);
        try {
            if (this.editing) {
                if (!await this.editing.withCommittedEdit(action)) this.error.set(this.editing.error() ?? 'Finish the current edit and try again.');
            }
            else await action();
        } catch (error) {
            this.error.set(error instanceof Error ? error.message : String(error));
        } finally {
            this.pending.set(false);
        }
    }
}
