import {Component, computed, DestroyRef, effect, inject, input, signal} from '@angular/core';
import {form, FormField, maxLength} from '@angular/forms/signals';
import {
    type FeedbackCueDraft,
    type FeedbackCueEvent,
    validCuePath,
    validCueSlug
} from '../../../../core/editor/feedback-audio.models';
import {EditorService} from '../../../../core/editor/editor.service';
import {EditorDialog} from '../../../../shared/components/dialog/dialog';
import {SceneEditingService} from '../scene-editing.service';
import {CuePreview} from './cue-preview';

type CuePaths = Record<FeedbackCueEvent, string>;

let nextAudioDialogId = 0;

interface CueRow {
    readonly event: FeedbackCueEvent;
    readonly label: string;
}

interface Audition extends CueRow {
    readonly path: string;
}

interface DialogScope {
    readonly clientGeneration: number;
}

const cueRows: readonly CueRow[] = [
    {event: 'key_collected', label: 'Key collected'},
    {event: 'gate_opened', label: 'Gate opened'},
    {event: 'run_completed', label: 'Run completed'},
];

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
    private readonly importDraft = signal<{ slug: string; event: FeedbackCueEvent }>({slug: '', event: 'gate_opened'});

    protected readonly rows = cueRows;
    protected readonly descriptionPrefix = 'feedback-audio-' + nextAudioDialogId++;
    protected readonly fields = form(this.paths, path => {
        for (const {event} of cueRows) maxLength(path[event], 46);
    });
    protected readonly imports = form(this.importDraft, path => maxLength(path.slug, 32));
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
    protected readonly canPreview = (event: FeedbackCueEvent): boolean => this.ready() && validCuePath(this.paths()[event]);
    protected readonly canApply = computed(() => {
        const catalog = this.catalog();
        if (!catalog || !this.ready() || this.rows.some(row => this.invalidPath(row.event))) return false;
        const original = cuePaths(catalog);
        return catalog.catalog_digest === null || this.rows.some(row => original[row.event] !== this.paths()[row.event]);
    });

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
        if (!scope || !catalog || !this.canApply() || !this.active(scope)) return;
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
                catalog.workspace_generation, catalog.catalog_digest, cues, scope.clientGeneration,
            );
            if (!this.active(scope)) return;
            this.catalog.set(saved);
            this.paths.set(cuePaths(saved));
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

    private async preview(row: CueRow): Promise<void> {
        const scope = this.scope();
        const catalog = this.catalog();
        if (!scope || !catalog || !this.canPreview(row.event) || !this.active(scope)) return;
        // Use a fresh request identity even when replaying the same cue.
        const path = this.paths()[row.event];
        const request = {...row, path};
        this.audition.set(request);
        this.error.set(null);
        const current = () => this.active(scope) && this.audition() === request && !this.auditionChanged();
        try {
            await this.player.play(
                () => this.editor.previewFeedbackCue(catalog.workspace_generation, path, scope.clientGeneration),
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

    private stopPreview(): void {
        this.audition.set(null);
        this.player.stop();
    }

    protected dismiss(): void {
        this.player.close();
        this.audition.set(null);
        this.scope.set(null);
        this.catalog.set(null);
        this.paths.set(cuePaths());
        this.importDraft.set({slug: '', event: 'gate_opened'});
        this.pending.set(false);
        this.publishing.set(false);
        this.error.set(null);
        this.status.set(null);
    }

    private active(scope: DialogScope): boolean {
        return this.scope() === scope && this.supported() && this.editMode() &&
            this.editor.workspaceGeneration() === scope.clientGeneration;
    }

    private auditionChanged(): boolean {
        const selected = this.audition();
        return selected !== null && this.paths()[selected.event] !== selected.path;
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
