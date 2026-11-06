import {Component, computed, DestroyRef, effect, inject, input, signal} from '@angular/core';
import type {ProjectBuildPreparation, ProjectBuildResult} from '../../../../core/editor/editor.models';
import {EditorService} from '../../../../core/editor/editor.service';
import {ProjectService} from '../../../../core/project/project.service';
import {SceneEditingService} from '../scene-editing.service';

@Component({
    selector: 'editor-project-build',
    templateUrl: './project-build.html',
    styleUrl: './project-build.scss',
})
export class ProjectBuild {
    readonly opening = input(false);
    protected readonly editor = inject(EditorService);
    private readonly projects = inject(ProjectService);
    private readonly editing = inject(SceneEditingService, {optional: true});
    protected readonly pending = signal<'choose' | 'build' | 'run' | null>(null);
    protected readonly result = signal<ProjectBuildResult | null>(null);
    protected readonly retryPreparation = signal<ProjectBuildPreparation | null>(null);
    protected readonly error = signal<string | null>(null);
    protected readonly status = signal<string | null>(null);
    protected readonly destination = signal('');
    protected readonly unsaved = computed(() => this.editor.dirty() || this.editing?.hasChanges() === true);
    protected readonly available = computed(() => this.editor.nativeAvailable && !!this.editor.snapshot() &&
        !this.opening() && !this.pending() && !this.unsaved() && (this.editing?.ready() ??
            (!this.editor.busy() && !this.editor.playing() && !this.editor.snapshot()?.transaction)));
    private readonly projectIdentity = this.editor.workspaceGeneration;
    private destroyed = false;

    constructor() {
        effect(() => {
            this.projectIdentity();
            this.result.set(null);
            this.retryPreparation.set(null);
            this.status.set(null);
        });
        inject(DestroyRef).onDestroy(() => this.destroyed = true);
    }

    protected async exportProject(): Promise<void> {
        if (!this.available()) return;
        this.pending.set('choose');
        this.error.set(null);
        const identity = this.projectIdentity();
        try {
            const destination = await this.projects.chooseDestination(this.destination() || null);
            if (destination === null || this.destroyed || this.opening() || identity !== this.projectIdentity()) return;
            this.destination.set(destination);
            const captured: {preparation?: ProjectBuildPreparation} = {};
            const capture = async () => {
                if (this.editor.dirty() || this.editor.playing()) throw new Error('Save All and stop Play before exporting.');
                captured.preparation = await this.editor.prepareBuild(destination);
            };
            const accepted = this.editing ? await this.editing.withCommittedEdit(capture) : (await capture(), true);
            if (!accepted || !captured.preparation) {
                this.error.set(this.editing?.error() ?? 'Finish the current edit and try exporting again.');
                return;
            }
            this.pending.set('build');
            this.retryPreparation.set(captured.preparation);
            await this.compile(captured.preparation, identity);
        } catch (error) {
            if (!this.destroyed) {
                this.error.set(error instanceof Error ? error.message : String(error));
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
        try {
            await this.compile(preparation, this.projectIdentity());
        } catch (error) {
            if (!this.destroyed) {
                this.error.set(error instanceof Error ? error.message : String(error));
                this.status.set(null);
            }
        } finally {
            if (!this.destroyed) this.pending.set(null);
        }
    }

    private async compile(preparation: ProjectBuildPreparation, identity: number): Promise<void> {
        this.status.set('Building the staged project. You can continue editing and saving while the standalone player compiles.');
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
        try {
            const launched = await this.editor.runBuild(result.path);
            if (!this.destroyed) this.status.set(`Standalone player started from ${launched.path}.`);
        } catch (error) {
            if (!this.destroyed) this.error.set(error instanceof Error ? error.message : String(error));
        } finally {
            if (!this.destroyed) this.pending.set(null);
        }
    }
}
