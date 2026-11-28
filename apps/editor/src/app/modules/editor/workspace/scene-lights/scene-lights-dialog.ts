import {Component, computed, effect, inject, input, signal} from '@angular/core';
import {MAX_AUTHORING_NUMBER_LENGTH, parseF32} from '../../../../core/editor/authoring-number';
import {
    CommandBuilder,
    type EditorCommand,
    type EntityKey,
    type Light,
    type Vec3
} from '../../../../core/editor/editor.models';
import {EditorService} from '../../../../core/editor/editor.service';
import {EditorDialog} from '../../../../shared/components/dialog/dialog';
import {EditorSearchPicker, type SearchPickerOption} from '../../../../shared/components/search-picker/search-picker';
import {SceneEditingService} from '../scene-editing.service';

type VectorField = 'position' | 'direction' | 'color';
type ScalarField = 'intensity' | 'range';
type VectorText = readonly [string, string, string];

interface LightDraft extends Pick<Light, 'key' | 'name' | 'kind'> {
    readonly position: VectorText;
    readonly direction: VectorText;
    readonly color: VectorText;
    readonly intensity: string;
    readonly range: string;
}

const AXES = [0, 1, 2] as const;

function parseNumber(text: string): number {
    return parseF32(text) ?? NaN;
}

function parseVector([first, second, third]: VectorText): Vec3 {
    return [parseNumber(first), parseNumber(second), parseNumber(third)];
}

function formatVector([first, second, third]: Vec3): VectorText {
    return [String(first), String(second), String(third)];
}

function sameLight(first: Light | null, second: Light | null): boolean {
    return JSON.stringify(first) === JSON.stringify(second);
}

function finite(value: number): boolean {
    return Number.isFinite(Math.fround(value));
}

function valid(light: Light): boolean {
    const directionLength = light.direction.reduce((sum, axis) => sum + axis * axis, 0);
    return light.name.trim().length > 0 && light.name.length <= 1024 && !/[\p{Cc}]/u.test(light.name) &&
        light.position.every(finite) && light.direction.every(finite) &&
        (light.kind === 'point' || finite(directionLength) && directionLength >= 1e-12) &&
        light.color.every(value => finite(value) && value >= 0 && value <= 1) &&
        finite(light.intensity) && light.intensity >= 0 && light.intensity <= 1_000_000 &&
        finite(light.range) && Math.fround(light.range) > 0;
}

@Component({
    selector: 'editor-scene-lights',
    imports: [EditorDialog, EditorSearchPicker],
    templateUrl: './scene-lights-dialog.html',
    styleUrl: './scene-lights-dialog.scss',
})
export class SceneLightsDialog {
    readonly opening = input(false);
    private readonly editor = inject(EditorService);
    private readonly editing = inject(SceneEditingService);
    private context: { readonly generation: number; readonly scene: string } | null = null;
    private readonly baseline = signal<Light | null>(null);
    protected readonly open = signal(false);
    protected readonly draft = signal<LightDraft | null>(null);
    protected readonly pending = signal(false);
    protected readonly error = signal<string | null>(null);
    protected readonly axes = AXES;
    protected readonly maxNumberLength = MAX_AUTHORING_NUMBER_LENGTH;
    protected readonly vectorNames = ['X', 'Y', 'Z'] as const;
    protected readonly colorNames = ['Red', 'Green', 'Blue'] as const;
    protected readonly dialogConfig = {width: '32rem'};
    protected readonly lights = computed(() => this.editor.document()?.lights ?? []);
    protected readonly lightOptions = computed<readonly SearchPickerOption[]>(() => this.lights().map(light => ({
        id: light.key, label: light.name, description: light.kind === 'point' ? 'Point light' : 'Directional light',
    })));
    private readonly parsed = computed<Light | null>(() => {
        const draft = this.draft();
        return draft && {
            ...draft, position: parseVector(draft.position), direction: parseVector(draft.direction),
            color: parseVector(draft.color), intensity: parseNumber(draft.intensity), range: parseNumber(draft.range)
        };
    });
    protected readonly changed = computed(() => !sameLight(this.parsed(), this.baseline()));
    protected readonly canApply = computed(() => {
        const light = this.parsed();
        return this.changed() && light !== null && valid(light);
    });
    protected readonly canRemove = computed(() =>
        this.editor.capabilities()?.commands?.some(command => command.name === 'remove_light') === true);

    constructor() {
        effect(() => {
            if (this.open() && !this.current()) this.open.set(false);
        });
    }

    show(key?: EntityKey): boolean {
        const snapshot = this.editor.snapshot();
        if (this.opening() || snapshot?.mode !== 'edit' || !this.editing.ready()) return false;
        this.context = {generation: this.editor.workspaceGeneration(), scene: snapshot.active_scene_id};
        this.load(key ?? this.lights().at(0)?.key ?? '');
        this.open.set(true);
        return true;
    }

    canLeaveProject(): boolean {
        if (!this.open()) return true;
        if (this.pending() || this.changed()) {
            this.error.set('Apply your Scene Lights changes or cancel the dialog before leaving the project.');
            return false;
        }
        this.open.set(false);
        return true;
    }

    protected select(key: string): void {
        if (this.pending() || this.changed() || !this.current() || !this.lights().some(light => light.key === key)) return;
        this.load(key);
    }

    protected revert(): void {
        if (!this.pending() && this.current()) this.load(this.draft()?.key ?? '');
    }

    protected updateScalar(field: ScalarField, value: string): void {
        this.draft.update(light => light && {...light, [field]: value});
    }

    protected updateName(name: string): void {
        this.draft.update(light => light && {...light, name});
    }

    protected updateVector(field: VectorField, axis: number, value: string): void {
        this.draft.update(light => {
            if (!light) return null;
            const vector = light[field];
            const next: VectorText = [axis === 0 ? value : vector[0], axis === 1 ? value : vector[1], axis === 2 ? value : vector[2]];
            return {...light, [field]: next};
        });
    }

    protected async apply(): Promise<void> {
        const light = this.parsed();
        if (light && this.canApply() && await this.execute(CommandBuilder.light({...light, name: light.name.trim()}))) {
            this.load(light.key);
        }
    }

    protected async remove(): Promise<void> {
        const light = this.draft();
        if (light && this.canRemove() && !this.changed() && await this.execute(CommandBuilder.removeLight(light.key))) {
            this.load(this.lights().at(0)?.key ?? '');
        }
    }

    private load(key: string): void {
        const light = this.lights().find(item => item.key === key) ?? null;
        this.baseline.set(light);
        this.draft.set(light && {
            ...light, position: formatVector(light.position), direction: formatVector(light.direction),
            color: formatVector(light.color), intensity: String(light.intensity), range: String(light.range)
        });
        this.error.set(null);
    }

    private current(): boolean {
        const snapshot = this.editor.snapshot();
        return snapshot?.mode === 'edit' && this.context?.generation === this.editor.workspaceGeneration()
            && this.context.scene === snapshot.active_scene_id;
    }

    private async execute(command: EditorCommand): Promise<boolean> {
        if (!this.open() || this.pending() || this.opening() || !this.current()) return false;
        const context = this.context!;
        const baseline = this.baseline();
        if (!baseline) return false;
        this.pending.set(true);
        this.error.set(null);
        let applied = false;
        try {
            const completed = await this.editing.withCommittedEdit(async () => {
                const snapshot = this.editor.snapshot();
                if (!snapshot || !this.current() || this.opening()) return;
                const latest = snapshot.document.lights.find(light => light.key === baseline.key) ?? null;
                if (!sameLight(latest, baseline)) throw new Error('This light changed. Revert the draft to load its latest values.');
                await this.editor.execute(command, context.generation, snapshot.revision);
                applied = true;
            });
            if (!completed && this.current()) {
                this.error.set(this.editing.error() ?? 'Finish the current edit before changing Scene Lights.');
            }
            return applied && this.current();
        } finally {
            this.pending.set(false);
        }
    }
}
