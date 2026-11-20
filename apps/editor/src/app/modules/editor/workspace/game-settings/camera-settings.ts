import {Component, computed, effect, inject, input, signal} from '@angular/core';
import {form, FormField} from '@angular/forms/signals';
import {EditorService} from '../../../../core/editor/editor.service';
import {EditorDialog} from '../../../../shared/components/dialog/dialog';
import {SceneEditingService} from '../scene-editing.service';
import {cameraDraft, followFromDraft} from './follow-camera';

@Component({
    selector: 'editor-camera-settings',
    imports: [EditorDialog, FormField],
    templateUrl: './camera-settings.html',
    styleUrl: './settings.scss',
})
export class CameraSettings {
    readonly opening = input(false);
    protected readonly editor = inject(EditorService);
    private readonly editing = inject(SceneEditingService, {optional: true});
    protected readonly open = signal(false);
    protected readonly pending = signal(false);
    protected readonly error = signal<string | null>(null);
    protected readonly message = signal<string | null>(null);
    protected readonly draft = signal(cameraDraft(null));
    protected readonly fields = form(this.draft);
    protected readonly groups = [{name: 'offset', label: 'Camera offset'}, {name: 'look_offset', label: 'Look offset'}] as const;
    protected readonly axes = ['x', 'y', 'z'] as const;
    protected readonly available = computed(() => this.editor.nativeAvailable && !!this.editor.snapshot() &&
        this.editor.snapshot()?.mode === 'edit' && !this.opening() && !this.pending() &&
        (this.editing?.ready() ?? (!this.editor.busy() && !this.editor.snapshot()?.transaction)));
    protected readonly candidate = computed(() => {
        try {
            return {value: followFromDraft(this.draft(), this.editor.snapshot()?.document.entities.map(entity => entity.key) ?? []), error: null};
        } catch (error) {
            return {value: null, error: error instanceof Error ? error.message : String(error)};
        }
    });
    private context: {generation: number; sceneId: string; signature: string} | null = null;

    constructor() {
        effect(() => {
            if (this.open() && this.context && (this.context.generation !== this.editor.workspaceGeneration() ||
                this.context.sceneId !== this.editor.snapshot()?.active_scene_id || this.editor.playing())) this.open.set(false);
        });
    }

    protected show(): void {
        const snapshot = this.editor.snapshot();
        if (!snapshot || !this.available()) return;
        this.context = {generation: this.editor.workspaceGeneration(), sceneId: snapshot.active_scene_id,
            signature: JSON.stringify(snapshot.document.camera.follow ?? null)};
        this.draft.set(cameraDraft(snapshot.document.camera.follow, snapshot.selection[0] ?? snapshot.document.entities[0]?.key));
        this.error.set(null);
        this.message.set(null);
        this.open.set(true);
    }

    protected async apply(): Promise<void> {
        const context = this.context;
        if (!context || !this.open() || !this.available() || this.candidate().error) return;
        const follow = this.candidate().value;
        const current = () => this.open() && context.generation === this.editor.workspaceGeneration() &&
            context.sceneId === this.editor.snapshot()?.active_scene_id && !this.editor.playing();
        this.pending.set(true);
        this.error.set(null);
        try {
            const write = async () => {
                const snapshot = this.editor.snapshot();
                if (!snapshot || !current()) return;
                if (JSON.stringify(snapshot.document.camera.follow ?? null) !== context.signature) {
                    throw new Error('Camera settings changed. Reopen this dialog to review them.');
                }
                if (JSON.stringify(follow) === context.signature) return;
                await this.editor.execute({type: 'set_camera', camera: {...snapshot.document.camera, follow}},
                    context.generation, snapshot.revision);
                if (current()) {
                    context.signature = JSON.stringify(follow);
                    this.message.set('Game camera applied. Save Scene to persist it.');
                }
            };
            if (this.editing) {
                if (!await this.editing.withCommittedEdit(write) && current()) this.error.set(this.editing.error() ?? 'Finish the current edit and try again.');
            } else await write();
        } catch (error) {
            if (current()) this.error.set(error instanceof Error ? error.message : String(error));
        } finally {
            this.pending.set(false);
        }
    }
}
