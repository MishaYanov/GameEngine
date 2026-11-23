import {Component, computed, DestroyRef, inject, input, linkedSignal, signal} from '@angular/core';
import {form, FormField} from '@angular/forms/signals';
import {EditorService} from '../../../../core/editor/editor.service';
import {parseF32} from '../../../../core/editor/authoring-number';
import {DEFAULT_AMBIENT, SKY_EXPOSURE_LIMIT} from '../../../../core/editor/sky.models';
import {skyColorChannels, skyColorDraft, skyColorFromDraft} from '../components/sky-component-draft';
import {SceneEditingService} from '../scene-editing.service';

@Component({
    selector: 'editor-scene-environment-inspector',
    imports: [FormField],
    templateUrl: './scene-environment-inspector.html',
    styles: ['fieldset {display: grid; gap: .5rem} .ambient {display: flex; flex-wrap: wrap; gap: .5rem} .ambient input {width: 5rem}'],
})
export class SceneEnvironmentInspector {
    readonly editable = input(false);
    private readonly editor = inject(EditorService);
    private readonly editing = inject(SceneEditingService, {optional: true});
    private readonly destroy = inject(DestroyRef);
    private readonly sceneEnvironment = computed(() => {
        const snapshot = this.editor.snapshot();
        const source = snapshot?.document.environment;
        return {
            generation: this.editor.workspaceGeneration(),
            scene: snapshot?.active_scene_id ?? null,
            ambient: source?.ambient_linear ?? DEFAULT_AMBIENT,
            exposure: source?.sky_exposure_ev ?? 0,
        };
    }, {equal: (a, b) => a.generation === b.generation && a.scene === b.scene &&
        a.exposure === b.exposure && a.ambient.every((channel, index) => channel === b.ambient[index])});
    private readonly draft = linkedSignal(() => {
        const current = this.sceneEnvironment();
        return {ambient: skyColorDraft(current.ambient), exposure: String(current.exposure)};
    });
    protected readonly fields = form(this.draft);
    protected readonly channels = skyColorChannels;
    protected readonly busy = signal(false);
    protected readonly error = linkedSignal({
        source: this.sceneEnvironment, computation: (): string | null => null,
    });
    protected readonly candidate = computed(() => {
        try {
            const draft = this.draft();
            const ambient = skyColorFromDraft(draft.ambient, 'Ambient light');
            const exposure = parseF32(draft.exposure);
            if (exposure === null || Math.abs(exposure) > SKY_EXPOSURE_LIMIT) {
                throw new Error(`Sky exposure must be between −${SKY_EXPOSURE_LIMIT} and ${SKY_EXPOSURE_LIMIT} EV.`);
            }
            return {value: {ambient, exposure}, error: null};
        } catch (error) {
            return {value: null, error: error instanceof Error ? error.message : String(error)};
        }
    });
    protected readonly canApply = computed(() => {
        const value = this.candidate().value;
        const current = this.sceneEnvironment();
        return this.editable() && !this.busy() && !!value &&
            (current.exposure !== value.exposure ||
                value.ambient.some((channel, index) => channel !== current.ambient[index]));
    });

    protected async apply(): Promise<void> {
        const value = this.candidate().value;
        const context = this.sceneEnvironment();
        if (!value || context.scene === null || !this.canApply()) return;
        const owns = () => !this.destroy.destroyed && this.sceneEnvironment() === context;
        this.busy.set(true);
        this.error.set(null);
        const commit = async () => {
            const snapshot = this.editor.snapshot();
            if (!owns() || snapshot?.mode !== 'edit' || snapshot.transaction) return;
            await this.editor.execute({
                type: 'set_scene_environment', ambient_linear: value.ambient, sky_exposure_ev: value.exposure,
            }, context.generation, snapshot.revision);
        };
        try {
            if (this.editing) {
                const accepted = await this.editing.withCommittedEdit(commit);
                if (!accepted && owns()) this.error.set(this.editing.error() ?? 'Finish the current scene edit and try again.');
            } else {
                await commit();
            }
        } catch (error) {
            if (owns()) this.error.set(error instanceof Error ? error.message : String(error));
        } finally {
            if (!this.destroy.destroyed) this.busy.set(false);
        }
    }
}
