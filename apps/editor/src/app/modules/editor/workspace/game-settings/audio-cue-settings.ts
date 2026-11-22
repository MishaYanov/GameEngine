import {Component, computed, DestroyRef, effect, inject, input, signal} from '@angular/core';
import {applyEach, form, FormField, max, maxLength, min} from '@angular/forms/signals';
import type {SceneId} from '../../../../core/editor/editor.models';
import {
    type FeedbackBus,
    feedbackBuses,
    type FeedbackCueDraft,
    type FeedbackCueEvent,
    validCuePath,
    validCueSlug,
    validFeedbackPercent,
} from '../../../../core/editor/feedback-audio.models';
import {EditorService} from '../../../../core/editor/editor.service';
import {EditorDialog} from '../../../../shared/components/dialog/dialog';
import {SceneEditingService} from '../scene-editing.service';
import {audioMixDraft, checkAudioMix} from './audio-mix-draft';
import {auditionGain, type AuditionMix, combinedFeedbackPercent, CuePreview} from './cue-preview';

type CuePaths = Record<FeedbackCueEvent, string>;

let nextAudioDialogId = 0;

interface CueRow {
    readonly event: FeedbackCueEvent;
    readonly bus: FeedbackBus;
    readonly label: string;
}

interface Audition extends CueRow {
    readonly path: string;
    readonly scene: SceneId;
    readonly mix: AuditionMix;
}

interface DialogScope {
    readonly clientGeneration: number;
}

const cueRows: readonly CueRow[] = [
    {event: 'key_collected', bus: 'collection', label: 'Key collected'},
    {event: 'gate_opened', bus: 'mechanisms', label: 'Gate opened'},
    {event: 'run_completed', bus: 'completion', label: 'Run completed'},
];

const defaultPreviewMix = {levelPercent: 100, muted: false} satisfies AuditionMix;

function cuePaths(catalog?: FeedbackCueDraft): CuePaths {
    const paths: CuePaths = {key_collected: '', gate_opened: '', run_completed: ''};
    for (const cue of catalog?.cues ?? []) paths[cue.event] = cue.path;
    return paths;
}

@Component({
    selector: 'editor-audio-cue-settings',
    imports: [EditorDialog, FormField],
    templateUrl: './audio-cue-settings.html',
    styleUrl: './settings.scss',
})
export class AudioCueSettings {
    readonly opening = input(false);

    private readonly editor = inject(EditorService);
    private readonly editing = inject(SceneEditingService, {optional: true});
    private readonly player = new CuePreview();
    private readonly scope = signal<DialogScope | null>(null);
    private readonly catalog = signal<FeedbackCueDraft | null>(null);
    private readonly paths = signal(cuePaths());
    protected readonly buses = feedbackBuses;
    private readonly busDraft = signal(audioMixDraft());
    protected readonly sceneChoices = computed(() => this.busDraft().scenes);
    private readonly importDraft = signal<{ slug: string; event: FeedbackCueEvent }>({slug: '', event: 'gate_opened'});
    protected readonly previewMix = signal<AuditionMix>(defaultPreviewMix);

    protected readonly rows = cueRows;
    protected readonly mixFields = form(this.busDraft, path => {
        for (const bus of feedbackBuses) {
            min(path.category[bus], 0);
            max(path.category[bus], 100);
        }
        applyEach(path.scenes, scene => {
            for (const bus of feedbackBuses) {
                min(scene.levels[bus], 0);
                max(scene.levels[bus], 100);
            }
        });
    });
    private readonly savedMix = signal('');
    private readonly sceneSelection = signal({id: '' as SceneId});
    protected readonly sceneField = form(this.sceneSelection).id;
    protected readonly selectedSceneIndex = computed(() =>
        this.sceneChoices().findIndex(scene => scene.id === this.sceneSelection().id));
    private readonly checkedMix = computed(() => checkAudioMix(this.busDraft()));
    protected readonly mixError = computed(() => {
        const result = this.checkedMix();
        return 'error' in result ? result.error : null;
    });
    protected readonly descriptionPrefix = 'feedback-audio-' + nextAudioDialogId++;
    protected readonly fields = form(this.paths, path => {
        for (const {event} of cueRows) maxLength(path[event], 46);
    });
    protected readonly imports = form(this.importDraft, path => maxLength(path.slug, 32));
    protected readonly previewFields = form(this.previewMix, path => {
        min(path.levelPercent, 0);
        max(path.levelPercent, 100);
    });
    protected readonly open = computed(() => this.scope() !== null);
    protected readonly supported = computed(() => this.editor.snapshot()?.project.gameplay_module === 'courtyard_run');
    protected readonly available = computed(() => this.supported() && this.editor.nativeAvailable && this.editMode() &&
        (this.editing?.ready() ?? !this.editor.busy()));
    protected readonly pending = signal(false);
    protected readonly publishing = signal(false);
    protected readonly ready = computed(() => this.catalog() !== null && !this.pending());
    protected readonly error = signal<string | null>(null);
    protected readonly status = signal<string | null>(null);
    protected readonly audition = signal<Audition | null>(null);
    protected readonly validImport = computed(() => validCueSlug(this.importDraft().slug));
    protected readonly invalidImport = computed(() => this.importDraft().slug !== '' && !this.validImport());
    protected readonly invalidPath = (event: FeedbackCueEvent): boolean =>
        this.paths()[event] !== '' && !validCuePath(this.paths()[event]);
    protected readonly canApply = computed(() => {
        const catalog = this.catalog();
        const checked = this.checkedMix();
        if (!catalog || !this.ready() || 'error' in checked || this.rows.some(row => this.invalidPath(row.event))) return false;
        const original = cuePaths(catalog);
        return catalog.catalog_digest === null || JSON.stringify(checked.mix) !== this.savedMix() ||
            this.rows.some(row => original[row.event] !== this.paths()[row.event]);
    });

    protected readonly invalidBusLevel = (level: number): boolean => !validFeedbackPercent(level);

    protected readonly canPreview = (row: CueRow): boolean =>
        this.ready() && validCuePath(this.paths()[row.event]) && this.authoredPercent(row.bus) !== null;

    protected readonly authoredPercent = (bus: FeedbackBus): number | null => {
        const {categoryPercent, scenePercent} = this.auditionMix(bus);
        return validFeedbackPercent(categoryPercent) && validFeedbackPercent(scenePercent)
            ? combinedFeedbackPercent(100, categoryPercent, scenePercent)
            : null;
    };

    constructor() {
        effect(() => {
            const scope = this.scope();
            if (scope && !this.active(scope)) this.dismiss();
            else if (this.auditionChanged()) this.stopPreview();
        });
        inject(DestroyRef).onDestroy(() => this.dismiss());
    }

    protected async show(): Promise<void> {
        if (!this.available() || this.open()) return;
        const scope = {clientGeneration: this.editor.workspaceGeneration()};
        this.scope.set(scope);
        this.pending.set(true);
        try {
            const catalog = await this.editor.feedbackCues(scope.clientGeneration);
            if (!this.active(scope)) return;
            this.catalog.set(catalog);
            this.paths.set(cuePaths(catalog));
            const snapshot = this.editor.snapshot();
            const sceneIds = (snapshot?.scenes ?? []).map(scene => scene.id);
            this.installMix(catalog, sceneIds);
            const active = snapshot?.active_scene_id;
            this.sceneSelection.set({id: active && sceneIds.includes(active) ? active : sceneIds[0] ?? ''});
            if (catalog.catalog_digest === null) {
                this.status.set('The Player uses its built-in gate tone. Apply with all rows empty to turn it off.');
            }
        } catch (cause) {
            this.report(scope, cause);
        } finally {
            if (this.active(scope)) this.pending.set(false);
        }
    }

    protected async importCue(): Promise<void> {
        const scope = this.scope();
        const catalog = this.catalog();
        if (!scope || !catalog || !this.ready() || !this.validImport() || !this.active(scope)) return;
        this.stopPreview();
        const {event, slug} = this.importDraft();
        this.pending.set(true);
        this.error.set(null);
        this.status.set(null);
        try {
            const path = await this.editor.importFeedbackCue(catalog.workspace_generation, slug, scope.clientGeneration);
            if (!path || !this.active(scope)) return;
            this.paths.update(paths => ({...paths, [event]: path}));
            this.status.set('WAV imported. Apply to save its cue assignment.');
        } catch (cause) {
            this.report(scope, cause);
        } finally {
            if (this.active(scope)) this.pending.set(false);
        }
    }

    protected async apply(): Promise<void> {
        const scope = this.scope();
        const catalog = this.catalog();
        const checked = this.checkedMix();
        if (!scope || !catalog || !this.canApply() || !this.active(scope) || 'error' in checked) return;
        this.stopPreview();
        const cues = this.rows.flatMap(({event}) => {
            const path = this.paths()[event];
            return path ? [{event, path}] : [];
        });
        this.publishing.set(true);
        this.pending.set(true);
        this.error.set(null);
        this.status.set(null);
        try {
            const saved = await this.editor.applyFeedbackCues(
                catalog.workspace_generation, catalog.catalog_digest, cues, scope.clientGeneration, checked.mix,
            );
            if (!this.active(scope)) return;
            this.catalog.set(saved);
            this.paths.set(cuePaths(saved));
            this.installMix(saved, this.sceneChoices().map(scene => scene.id));
            this.status.set('Feedback audio saved.');
        } catch (cause) {
            this.report(scope, cause);
        } finally {
            if (this.active(scope)) {
                this.publishing.set(false);
                this.pending.set(false);
            }
        }
    }

    protected togglePreview(row: CueRow): void {
        if (this.audition()?.event === row.event) this.stopPreview();
        else void this.preview(row);
    }

    protected dismiss(): void {
        this.player.close();
        this.audition.set(null);
        this.scope.set(null);
        this.catalog.set(null);
        this.paths.set(cuePaths());
        this.busDraft.set(audioMixDraft());
        this.savedMix.set('');
        this.sceneSelection.set({id: ''});
        this.importDraft.set({slug: '', event: 'gate_opened'});
        this.previewMix.set(defaultPreviewMix);
        this.pending.set(false);
        this.publishing.set(false);
        this.error.set(null);
        this.status.set(null);
    }

    private stopPreview(): void {
        this.audition.set(null);
        this.player.stop();
    }

    private async preview(row: CueRow): Promise<void> {
        const scope = this.scope();
        const catalog = this.catalog();
        if (!scope || !catalog || !this.canPreview(row) || !this.active(scope)) return;
        // Use a fresh request identity even when replaying the same cue.
        const path = this.paths()[row.event];
        const mix = this.auditionMix(row.bus);
        const request = {...row, path, mix, scene: this.sceneSelection().id};
        this.audition.set(request);
        this.error.set(null);
        this.status.set(auditionGain(mix) === 0 ? 'Preview skipped at zero volume.' : null);
        const current = () => this.active(scope) && this.audition() === request && !this.auditionChanged();
        try {
            await this.player.play(
                () => this.editor.previewFeedbackCue(catalog.workspace_generation, path, scope.clientGeneration),
                mix,
                current,
                () => {
                    if (current()) this.audition.set(null);
                },
            );
        } catch (cause) {
            if (current()) {
                this.stopPreview();
                this.report(scope, cause);
            }
        }
    }

    private active(scope: DialogScope): boolean {
        return this.scope() === scope && this.supported() && this.editMode() &&
            this.editor.workspaceGeneration() === scope.clientGeneration;
    }

    private auditionChanged(): boolean {
        const selected = this.audition();
        if (!selected) return false;
        const mix = this.auditionMix(selected.bus);
        return this.paths()[selected.event] !== selected.path || this.sceneSelection().id !== selected.scene ||
            mix.levelPercent !== selected.mix.levelPercent || mix.muted !== selected.mix.muted ||
            mix.categoryPercent !== selected.mix.categoryPercent || mix.scenePercent !== selected.mix.scenePercent;
    }

    private auditionMix(bus: FeedbackBus): AuditionMix {
        const draft = this.busDraft();
        const scene = draft.scenes[this.selectedSceneIndex()];
        return {
            ...this.previewMix(),
            categoryPercent: draft.category[bus],
            scenePercent: scene ? scene.levels[bus] : 100,
        };
    }

    private installMix(catalog: FeedbackCueDraft, sceneIds: readonly SceneId[]): void {
        const draft = audioMixDraft(catalog.mix, sceneIds);
        const checked = checkAudioMix(draft);
        this.busDraft.set(draft);
        this.savedMix.set('mix' in checked ? JSON.stringify(checked.mix) : '');
    }

    private editMode(): boolean {
        const snapshot = this.editor.snapshot();
        return !this.opening() && snapshot?.mode === 'edit' && !snapshot.transaction;
    }

    private report(scope: DialogScope, cause: unknown): void {
        if (this.active(scope)) {
            this.error.set(cause instanceof Error ? cause.message :
                typeof cause === 'string' ? cause : 'The feedback audio operation failed.');
        }
    }
}
