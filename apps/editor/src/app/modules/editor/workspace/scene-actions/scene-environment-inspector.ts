import {Component, computed, DestroyRef, inject, input, linkedSignal, signal} from '@angular/core';
import {form, FormField} from '@angular/forms/signals';
import type {EditorCommand} from '../../../../core/editor/editor.models';
import {EditorService} from '../../../../core/editor/editor.service';
import {parseF32} from '../../../../core/editor/authoring-number';
import {DEFAULT_AMBIENT, SKY_EXPOSURE_LIMIT, type SceneFog} from '../../../../core/editor/sky.models';
import {skyColorChannels, skyColorDraft, skyColorFromDraft} from '../components/sky-component-draft';
import {SceneEditingService} from '../scene-editing.service';

const DEFAULT_FOG = {
    color_linear: [0.1, 0.15, 0.22],
    density: 0.025,
    base_height: 0,
    height_falloff: 0.06,
    start_distance: 4,
} as const satisfies Required<SceneFog>;

@Component({
    selector: 'editor-scene-environment-inspector',
    imports: [FormField],
    templateUrl: './scene-environment-inspector.html',
    styles: [`
        fieldset {display: grid; gap: .5rem}
        fieldset + fieldset {margin-block-start: .75rem}
        .channels {display: flex; flex-wrap: wrap; gap: .5rem}
        .channels input {width: 5rem}
        .fog-settings {display: grid; grid-template-columns: repeat(auto-fit, minmax(min(8rem, 100%), 1fr)); gap: .5rem}
        .fog-settings input {width: 100%; box-sizing: border-box}
    `],
})
export class SceneEnvironmentInspector {
    readonly editable = input(false);
    private readonly editor = inject(EditorService);
    private readonly editing = inject(SceneEditingService, {optional: true});
    private readonly destroy = inject(DestroyRef);
    private readonly scope = computed(() => ({
        generation: this.editor.workspaceGeneration(),
        scene: this.editor.snapshot()?.active_scene_id ?? null,
    }), {equal: (a, b) => a.generation === b.generation && a.scene === b.scene});
    private readonly sceneEnvironment = computed(() => {
        const source = this.editor.snapshot()?.document.environment;
        return {
            scope: this.scope(),
            ambient: source?.ambient_linear ?? DEFAULT_AMBIENT,
            exposure: source?.sky_exposure_ev ?? 0,
        };
    }, {
        equal: (a, b) => a.scope === b.scope &&
        a.exposure === b.exposure && a.ambient.every((channel, index) => channel === b.ambient[index])});
    private readonly draft = linkedSignal(() => {
        const current = this.sceneEnvironment();
        return {ambient: skyColorDraft(current.ambient), exposure: String(current.exposure)};
    });
    private readonly sceneFog = computed(() => ({
        scope: this.scope(),
        value: this.editor.snapshot()?.document.environment?.fog ?? null,
    }), {equal: (a, b) => a.scope === b.scope && equalFog(a.value, b.value)});
    private readonly fogDraft = linkedSignal(() => {
        const fog = this.sceneFog().value;
        const source = fog ?? DEFAULT_FOG;
        return {
            enabled: fog !== null,
            color: skyColorDraft(source.color_linear),
            density: String(source.density),
            baseHeight: String(source.base_height),
            heightFalloff: String(source.height_falloff),
            startDistance: String(source.start_distance ?? 0),
        };
    });
    protected readonly fields = form(this.draft);
    protected readonly fogFields = form(this.fogDraft);
    protected readonly fogSupported = computed(() =>
        this.editor.capabilities()?.commands?.some(command => command.name === 'set_scene_fog') ?? false);
    protected readonly channels = skyColorChannels;
    protected readonly busy = signal(false);
    protected readonly error = linkedSignal({
        source: this.scope, computation: (): string | null => null,
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
    protected readonly fogCandidate = computed((): { value: SceneFog | null; error: string | null } => {
        const draft = this.fogDraft();
        if (!draft.enabled) return {value: null, error: null};
        try {
            return {
                value: {
                    color_linear: skyColorFromDraft(draft.color, 'Fog color'),
                    density: fogNumber(draft.density, 'Fog density', 0, 1),
                    base_height: fogNumber(draft.baseHeight, 'Fog base height'),
                    height_falloff: fogNumber(draft.heightFalloff, 'Fog height falloff', 0, 1),
                    start_distance: fogNumber(draft.startDistance, 'Fog start distance', 0),
                }, error: null
            };
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

    protected readonly canApplyFog = computed(() => {
        const candidate = this.fogCandidate();
        return this.editable() && this.fogSupported() && !this.busy() && candidate.error === null &&
            !equalFog(this.sceneFog().value, candidate.value);
    });

    protected async apply(): Promise<void> {
        const value = this.candidate().value;
        const context = this.sceneEnvironment();
        if (!value || !this.canApply()) return;
        await this.commit({
            type: 'set_scene_environment', ambient_linear: value.ambient, sky_exposure_ev: value.exposure,
        }, () => this.sceneEnvironment() === context);
    }

    protected async applyFog(): Promise<void> {
        if (!this.canApplyFog()) return;
        const context = this.sceneFog();
        await this.commit({type: 'set_scene_fog', fog: this.fogCandidate().value}, () => this.sceneFog() === context);
    }

    private async commit(command: EditorCommand, ownsDraft: () => boolean): Promise<void> {
        const scope = this.scope();
        if (scope.scene === null) return;
        const owns = () => !this.destroy.destroyed && ownsDraft();
        this.busy.set(true);
        this.error.set(null);
        const commit = async () => {
            const snapshot = this.editor.snapshot();
            if (!owns() || snapshot?.mode !== 'edit' || snapshot.transaction) return;
            await this.editor.execute(command, scope.generation, snapshot.revision);
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

function equalFog(left: SceneFog | null, right: SceneFog | null): boolean {
    if (left === null || right === null) return left === right;
    return left.density === right.density && left.height_falloff === right.height_falloff &&
        left.base_height === right.base_height && (left.start_distance ?? 0) === (right.start_distance ?? 0) &&
        left.color_linear.every((channel, index) => channel === right.color_linear[index]);
}

function fogNumber(draft: string, label: string, min = -Infinity, max = Infinity): number {
    const value = parseF32(draft);
    if (value === null || value < min || value > max) {
        if (max === 1) throw new Error(`${label} must be between 0 and 1.`);
        if (min === 0) throw new Error(`${label} must be a finite number of at least zero.`);
        throw new Error(`${label} must be a finite number.`);
    }
    return value;
}
