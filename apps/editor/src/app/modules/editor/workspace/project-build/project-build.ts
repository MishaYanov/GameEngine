import {Component, computed, DestroyRef, effect, ElementRef, inject, input, signal, viewChild} from '@angular/core';
import type {ProjectBuildPreparation, ProjectBuildResult} from '../../../../core/editor/editor.models';
import {EditorService} from '../../../../core/editor/editor.service';
import {ProjectService} from '../../../../core/project/project.service';
import {SceneEditingService} from '../scene-editing.service';
import {WorkspaceDraftReview} from '../workspace-draft-review';

@Component({
    selector: 'editor-project-build',
    templateUrl: './project-build.html',
    styleUrl: './project-build.scss',
})
export class ProjectBuild {
    readonly opening = input(false);
    protected readonly editor = inject(EditorService);
    private readonly projects = inject(ProjectService);
    private readonly drafts = inject(WorkspaceDraftReview);
    private readonly editing = inject(SceneEditingService, {optional: true});
    protected readonly pending = signal<'choose' | 'build' | 'run' | null>(null);
    protected readonly result = signal<ProjectBuildResult | null>(null);
    protected readonly retryPreparation = signal<ProjectBuildPreparation | null>(null);
    protected readonly error = signal<string | null>(null);
    private readonly exportError = signal<string | null>(null);
    readonly exportFailure = this.exportError.asReadonly();
    private readonly failureMessage = viewChild<ElementRef<HTMLElement>>('failureMessage');
    protected readonly status = signal<string | null>(null);
    protected readonly destination = signal('');
    protected readonly unsaved = computed(() => this.editor.dirty() || this.editing?.hasChanges() === true);
    protected readonly available = computed(() => this.editor.nativeAvailable && !!this.editor.snapshot() &&
        !this.opening() && !this.pending() && !this.drafts.pending() && !this.unsaved() && (this.editing?.ready() ??
            (!this.editor.busy() && !this.editor.playing() && !this.editor.snapshot()?.transaction)));
    private readonly projectIdentity = this.editor.workspaceGeneration;
    private destroyed = false;

    constructor() {
        effect(() => {
            this.projectIdentity();
            this.result.set(null);
            this.retryPreparation.set(null);
            this.error.set(null);
            this.exportError.set(null);
            this.status.set(null);
        });
        inject(DestroyRef).onDestroy(() => this.destroyed = true);
    }

    revealExportFailure(): void {
        const message = this.failureMessage()?.nativeElement;
        message?.scrollIntoView?.({block: 'nearest'});
        message?.focus({preventScroll: true});
    }

    protected async exportProject(): Promise<void> {
        if (!this.available()) return;
        this.pending.set('choose');
        this.error.set(null);
        this.exportError.set(null);
        const target = this.drafts.capture();
        try {
            const destination = await this.projects.chooseDestination(this.destination() || null);
            if (destination === null || this.destroyed || this.opening() || !this.drafts.current(target)) return;
            this.destination.set(destination);
            const captured: {preparation?: ProjectBuildPreparation} = {};
            const reviewed = await this.drafts.run(target, async () => {
                const capture = async () => {
                    const snapshot = this.editor.snapshot();
                    if (!snapshot || this.opening() || !this.drafts.current(target)) return;
                    if (this.editor.dirty() || this.editor.playing()) throw new Error('Save All and stop Play before exporting.');
                    captured.preparation = await this.editor.prepareBuild(destination, target.generation, snapshot.revision);
                };
                if (this.editing && !await this.editing.withCommittedEdit(capture)) {
                    this.reportExportFailure(this.editing.error() ?? 'Finish the current edit and try exporting again.');
                } else if (!this.editing) await capture();
            });
            if (!reviewed || this.destroyed || !this.drafts.current(target) || !captured.preparation) return;
            this.pending.set('build');
            this.retryPreparation.set(captured.preparation);
            await this.finish(captured.preparation, target.generation);
        } catch (error) {
            if (!this.destroyed && this.drafts.current(target)) {
                this.reportExportFailure(error);
                this.status.set(null);
            }
        } finally {
            if (!this.destroyed) this.pending.set(null);
        }
    }

    protected async retryExport(): Promise<void> {
        const preparation = this.retryPreparation();
        if (!preparation || this.pending() || this.opening() || !this.editor.nativeAvailable) return;
        this.pending.set('build');
        this.error.set(null);
        this.exportError.set(null);
        const identity = this.projectIdentity();
        try {
            await this.finish(preparation, identity);
        } catch (error) {
            if (!this.destroyed && identity === this.projectIdentity()) {
                this.reportExportFailure(error);
                this.status.set(null);
            }
        } finally {
            if (!this.destroyed) this.pending.set(null);
        }
    }

    private reportExportFailure(error: unknown): void {
        const message = error instanceof Error ? error.message : String(error);
        this.error.set(message);
        this.exportError.set(message);
    }

    private async finish(preparation: ProjectBuildPreparation, identity: number): Promise<void> {
        this.status.set('Finishing export. You can continue editing and saving while the standalone game is prepared.');
        const result = await this.editor.finishBuild(preparation.build_id);
        if (this.destroyed) return;
        if (identity !== this.projectIdentity()) return;
        this.retryPreparation.set(null);
        this.result.set(result);
        this.status.set('Export complete. The output folder contains the standalone player and project assets.');
    }

    protected async runBuild(): Promise<void> {
        const result = this.result();
        if (!result || this.pending() || !this.editor.nativeAvailable) return;
        this.pending.set('run');
        this.error.set(null);
        const identity = this.projectIdentity();
        try {
            const launched = await this.editor.runBuild(result.path);
            if (!this.destroyed && identity === this.projectIdentity())
                this.status.set(`Standalone player started from ${launched.path}.`);
        } catch (error) {
            if (!this.destroyed && identity === this.projectIdentity())
                this.error.set(error instanceof Error ? error.message : String(error));
        } finally {
            if (!this.destroyed) this.pending.set(null);
        }
    }
}
