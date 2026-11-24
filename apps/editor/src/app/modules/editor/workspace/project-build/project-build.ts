import {Component, computed, DestroyRef, effect, ElementRef, inject, input, signal, viewChild} from '@angular/core';
import type {ProjectBuildPreparation, ProjectBuildResult} from '../../../../core/editor/editor.models';
import {EditorService} from '../../../../core/editor/editor.service';
import {ViewportEditorService} from '../../../../core/editor/viewport-editor.service';
import {ProjectService} from '../../../../core/project/project.service';
import {SceneEditingService} from '../scene-editing.service';
import {WorkspaceDraftReview} from '../workspace-draft-review';
import {
    discardStagedProjectBuild, type ProjectBuildReadback, readProjectBuildStatus, watchProjectBuildStatus,
} from './project-build-status';

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
    private readonly viewport = inject(ViewportEditorService, {optional: true});
    private readonly nativeGeneration = computed(() => this.viewport?.state()?.workspace_generation ?? null);
    private readonly readback = signal<ProjectBuildReadback | null>(null);
    private readonly nativeJob = computed(() => this.readback()?.current ?? null);
    protected readonly pending = signal<'choose' | 'stage' | 'build' | 'run' | 'discard' | null>(null);
    private readonly result = signal<ProjectBuildResult | null>(null);
    private readonly retryPreparation = signal<ProjectBuildPreparation | null>(null);
    protected readonly error = signal<string | null>(null);
    private readonly exportError = signal<string | null>(null);
    readonly exportFailure = computed(() => this.exportError() ??
        (this.pending() === null ? this.nativeJob()?.diagnostic?.message ?? null : null));
    private readonly failureMessage = viewChild<ElementRef<HTMLElement>>('failureMessage');
    private readonly status = signal<string | null>(null);
    protected readonly stagedExport = computed<ProjectBuildPreparation | null>(() => {
        const job = this.nativeJob();
        if (!job) return this.retryPreparation();
        return job.output && (job.phase === 'staged' || job.retryable)
            ? {...job.output, build_id: job.build_id}
            : null;
    });
    protected readonly visibleResult = computed(() => {
        const job = this.nativeJob();
        return this.result() ?? (job?.phase === 'succeeded' ? job.output : null);
    });
    protected readonly buildInProgress = computed(() => {
        const phase = this.nativeJob()?.phase;
        return this.pending() === 'stage' || this.pending() === 'build' ||
            (this.readback()?.occupied === true && phase !== 'staged');
    });
    protected readonly canUseStagedExport = computed(() =>
        this.editor.nativeAvailable && !this.opening() && !this.pending() && !this.buildInProgress());
    protected readonly canDiscard = computed(() => {
        const job = this.nativeJob();
        return job?.phase === 'staged' || job?.retryable === true;
    });
    protected readonly announcement = computed(() => {
        switch (this.nativeJob()?.phase) {
            case 'validating': return 'Checking saved project files…';
            case 'staging': return 'Copying verified project files…';
            case 'finishing': return 'Finishing export. You can continue editing and saving.';
            case 'staged':
                if (!this.pending()) return 'Staged files are ready to finish.';
                break;
            case 'succeeded':
                if (!this.pending() && !this.status()) return 'Export complete.';
                break;
        }
        if (this.buildInProgress() && this.readback()?.occupied && !this.nativeJob()) {
            return 'Another project is still exporting.';
        }
        return this.status();
    });
    protected readonly destination = signal('');
    protected readonly unsaved = computed(() => this.editor.dirty() || this.editing?.hasChanges() === true);
    protected readonly available = computed(() => this.editor.nativeAvailable && !!this.editor.snapshot() &&
        !this.opening() && !this.pending() && !this.readback()?.occupied && !this.drafts.pending() && !this.unsaved() && (this.editing?.ready() ??
            (!this.editor.busy() && !this.editor.playing() && !this.editor.snapshot()?.transaction)));
    private readonly projectIdentity = this.editor.workspaceGeneration;
    private destroyed = false;
    private refreshNativeStatus: (() => void) | null = null;

    constructor() {
        effect(() => {
            this.projectIdentity();
            this.result.set(null);
            this.retryPreparation.set(null);
            this.error.set(null);
            this.exportError.set(null);
            this.status.set(null);
        });
        effect(onCleanup => {
            const generation = this.nativeGeneration();
            const identity = this.projectIdentity();
            this.readback.set(null);
            if (!this.editor.nativeAvailable || generation === null) return;
            let active = true;
            let readVersion = 0;
            let unlisten: (() => void) | undefined;
            const refresh = () => {
                const version = ++readVersion;
                void readProjectBuildStatus(generation).then(readback => {
                    if (active && version === readVersion && generation === this.nativeGeneration() &&
                        identity === this.projectIdentity()) {
                        this.readback.set({...readback, current:
                            readback.current?.workspace_generation === generation ? readback.current : null});
                    }
                }).catch(() => undefined);
            };
            this.refreshNativeStatus = refresh;
            void watchProjectBuildStatus(refresh).then(stop => {
                if (!active) stop();
                else {
                    unlisten = stop;
                    refresh();
                }
            }).catch(refresh);
            onCleanup(() => {
                active = false;
                this.refreshNativeStatus = null;
                unlisten?.();
            });
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
                    this.pending.set('stage');
                    this.status.set('Checking saved project files…');
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
            if (!this.destroyed) {
                this.pending.set(null);
                this.refreshNativeStatus?.();
            }
        }
    }

    protected async retryExport(): Promise<void> {
        const preparation = this.stagedExport();
        if (!preparation || !this.canUseStagedExport()) return;
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
            if (!this.destroyed) {
                this.pending.set(null);
                this.refreshNativeStatus?.();
            }
        }
    }

    protected async discardExport(): Promise<void> {
        const preparation = this.stagedExport();
        if (!preparation || !this.canDiscard() || !this.canUseStagedExport()) return;
        const identity = this.projectIdentity();
        this.pending.set('discard');
        this.error.set(null);
        this.exportError.set(null);
        try {
            await discardStagedProjectBuild(preparation.build_id);
            if (!this.destroyed && identity === this.projectIdentity()) {
                this.retryPreparation.set(null);
                if (this.nativeJob()?.build_id === preparation.build_id) this.readback.set(null);
                this.status.set('Staged export released. Its files are still in the output folder.');
            }
        } catch (error) {
            if (!this.destroyed && identity === this.projectIdentity()) this.reportExportFailure(error);
        } finally {
            if (!this.destroyed) {
                this.pending.set(null);
                this.refreshNativeStatus?.();
            }
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
        const result = this.visibleResult();
        if (!result || this.pending() || this.buildInProgress() || !this.editor.nativeAvailable) return;
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
