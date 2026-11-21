import {
    afterRenderEffect,
    Component,
    computed,
    effect,
    ElementRef,
    inject,
    input,
    signal,
    viewChild
} from '@angular/core';
import {form, FormField} from '@angular/forms/signals';
import {EditorService} from '../../../../core/editor/editor.service';
import {EditorDialog} from '../../../../shared/components/dialog/dialog';
import {EditorSearchPicker} from '../../../../shared/components/search-picker/search-picker';
import {SceneEditingService} from '../scene-editing.service';
import {sceneReferenceOptions} from '../scene-reference-options';
import {cameraDraft, followFromDraft} from './follow-camera';

@Component({
    selector: 'editor-camera-settings',
    imports: [EditorDialog, EditorSearchPicker, FormField],
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
    protected readonly focusTarget = signal(false);
    protected readonly entityOptions = computed(() => sceneReferenceOptions(this.editor.snapshot()?.document.entities ?? []));
    protected readonly orbit = computed(() => this.editor.snapshot()?.document.camera.orbit ?? null);
    protected readonly orbitTarget = computed(() => this.entityOptions().find(entity => entity.id === this.orbit()?.target));
    private readonly orbitField = viewChild<ElementRef<HTMLElement>>('orbitField');
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
    protected readonly canRevealTarget = computed(() => {
        const follow = this.editor.snapshot()?.document.camera.follow;
        const candidate = this.candidate();
        return !!this.editing && this.available() && !!follow && candidate.error === null &&
            JSON.stringify(candidate.value) === JSON.stringify(follow);
    });
    private context: {generation: number; sceneId: string; signature: string} | null = null;

    constructor() {
        effect(() => {
            if (this.open() && this.context && (this.context.generation !== this.editor.workspaceGeneration() ||
                this.context.sceneId !== this.editor.snapshot()?.active_scene_id || this.editor.playing())) this.open.set(false);
        });
        afterRenderEffect(() => {
            const field = this.orbitField()?.nativeElement;
            if (this.open() && this.focusTarget() && !field?.closest('dialog:not([open])')) field?.focus();
        });
    }

    show(focusTarget = false, mode: 'follow' | 'orbit' = 'follow'): boolean {
        const snapshot = this.editor.snapshot();
        if (!snapshot || !this.available() || (mode === 'orbit' && !snapshot.document.camera.orbit)) return false;
        this.focusTarget.set(focusTarget);
        if (this.open()) return true;
        this.context = {generation: this.editor.workspaceGeneration(), sceneId: snapshot.active_scene_id,
            signature: JSON.stringify(snapshot.document.camera.follow ?? null)};
        this.draft.set(cameraDraft(snapshot.document.camera.follow, snapshot.selection[0] ?? snapshot.document.entities[0]?.key));
        this.error.set(null);
        this.message.set(null);
        this.open.set(true);
        return true;
    }

    protected async apply(): Promise<void> {
        const context = this.context;
        if (!context || !this.open() || !this.available() || this.orbit() || this.candidate().error) return;
        const follow = this.candidate().value;
        const current = () => this.open() && context.generation === this.editor.workspaceGeneration() &&
            context.sceneId === this.editor.snapshot()?.active_scene_id && !this.editor.playing();
        this.pending.set(true);
        this.error.set(null);
        try {
            const write = async () => {
                const snapshot = this.editor.snapshot();
                if (!snapshot || !current()) return;
                if (snapshot.document.camera.orbit || JSON.stringify(snapshot.document.camera.follow ?? null) !== context.signature) {
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

    protected async revealTarget(): Promise<void> {
        const editing = this.editing;
        const context = this.context;
        if (!editing || !context || !this.canRevealTarget()) return;
        const target = this.draft().target;
        this.pending.set(true);
        this.error.set(null);
        const current = () => this.open() && this.context === context &&
            context.generation === this.editor.workspaceGeneration() &&
            context.sceneId === this.editor.snapshot()?.active_scene_id;
        try {
            await editing.select(target);
            if (current()) {
                if (this.editor.snapshot()?.selection[0] === target) this.open.set(false);
                else this.error.set(editing.error() ?? 'The target could not be selected.');
            }
        } catch (error) {
            if (current()) this.error.set(error instanceof Error ? error.message : String(error));
        } finally {
            this.pending.set(false);
        }
    }
}
