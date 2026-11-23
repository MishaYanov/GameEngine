import {Component, computed, DestroyRef, inject, signal} from '@angular/core';
import {FormControl, FormGroup, ReactiveFormsModule, Validators} from '@angular/forms';
import {ActivatedRoute, Router, RouterLink} from '@angular/router';
import {EditorService} from '../../core/editor/editor.service';
import {ProjectService, type ProjectCreationResult} from '../../core/project/project.service';
import {ProjectSetupPreview} from './project-setup-preview';

@Component({
    selector: 'app-project-setup',
    imports: [ReactiveFormsModule, RouterLink, ProjectSetupPreview],
    templateUrl: './project-setup.html',
    styleUrl: './project-setup.scss',
})
export class ProjectSetup {
    private readonly projects = inject(ProjectService);
    private readonly editor = inject(EditorService);
    private readonly router = inject(Router);
    private readonly creating = signal(false);
    private readonly opening = signal(false);
    protected readonly busy = computed(() => this.creating() || this.opening());
    protected readonly choosingDestination = signal(false);
    protected readonly error = signal('');
    protected readonly result = signal<ProjectCreationResult | null>(null);
    protected readonly openMessage = signal('');
    protected readonly desktop = this.projects.nativeAvailable;
    protected readonly fromEditor = inject(ActivatedRoute).snapshot.queryParamMap.get('from') === 'editor';
    protected readonly buttonLabel = computed(() =>
        this.creating() ? 'Creating project…' : this.result() ? 'Project created' : 'Create project',
    );
    protected readonly form = new FormGroup({
        name: new FormControl('', {
            nonNullable: true,
            validators: [
                Validators.required,
                Validators.maxLength(80),
                Validators.pattern(/^[\p{L}\p{N}_ -]+$/u),
            ],
        }),
        destination: new FormControl('', {nonNullable: true, validators: [Validators.required]}),
    });
    private destroyed = false;

    constructor() {
        inject(DestroyRef).onDestroy(() => this.destroyed = true);
    }

    protected async chooseDestination(): Promise<void> {
        if (!this.desktop || this.busy() || this.choosingDestination() || this.result()) return;
        this.choosingDestination.set(true);
        this.error.set('');
        try {
            const destination = await this.projects.chooseDestination(this.form.controls.destination.value);
            if (destination !== null && !this.destroyed) {
                this.form.controls.destination.setValue(destination);
                this.form.controls.destination.markAsDirty();
            }
        } catch (error: unknown) {
            if (!this.destroyed) this.error.set(this.message(error));
        } finally {
            this.choosingDestination.set(false);
        }
    }

    protected async createProject(): Promise<void> {
        if (this.busy() || this.choosingDestination() || this.result()) return;
        this.form.markAllAsTouched();
        if (this.form.invalid || !this.desktop) return;
        this.creating.set(true);
        this.error.set('');
        this.openMessage.set('Creating the project and copying starter assets…');
        this.form.disable();
        try {
            const values = this.form.getRawValue();
            const project = await this.projects.create(values.name, values.destination);
            if (this.destroyed) return;
            this.result.set(project);
            this.creating.set(false);
            await this.openCreatedProject();
        } catch (error: unknown) {
            if (!this.destroyed) this.error.set(this.message(error));
        } finally {
            this.creating.set(false);
            if (!this.result()) this.form.enable();
            if (!this.result() && !this.destroyed) this.openMessage.set('');
        }
    }

    protected async openCreatedProject(): Promise<void> {
        const project = this.result();
        if (!project || !this.desktop || this.busy() || this.choosingDestination()) return;
        this.opening.set(true);
        this.error.set('');
        this.openMessage.set('Opening your project in the editor…');
        try {
            // Keep the exact revision reviewed before an asynchronous discard decision.
            const reviewed = this.editor.snapshot();
            if (reviewed?.mode === 'play') throw new Error('Stop Play in the editor before opening this project.');
            if (reviewed?.transaction) throw new Error('Finish the current edit in the editor before opening this project.');
            if (this.editor.busy()) throw new Error('Wait for the current editor operation to finish, then open this project.');

            const discardUnsaved = Boolean(reviewed && (
                reviewed.dirty || reviewed.project_dirty || reviewed.scenes.some(scene => scene.dirty)
            ));
            if (discardUnsaved) {
                this.openMessage.set('Waiting for your decision about unsaved changes…');
                const confirmed = await this.editor.confirmProjectDiscard();
                if (!confirmed || this.destroyed) {
                    if (!this.destroyed) this.openMessage.set('Project created. Your current project remains open.');
                    return;
                }
            }
            if (this.destroyed) return;
            this.openMessage.set('Opening your project in the editor…');
            await this.editor.openProject(project.manifestPath, {
                expectedRevision: reviewed?.revision ?? null,
                discardUnsaved,
            });
            if (!this.destroyed && !await this.router.navigateByUrl('/editor')) {
                throw new Error('Your project is open. Select Open in Editor to return to it.');
            }
        } catch (error: unknown) {
            if (!this.destroyed) {
                this.error.set(this.message(error));
                this.openMessage.set('Your project was created and can be opened later.');
            }
        } finally {
            this.opening.set(false);
        }
    }

    private message(error: unknown): string {
        return error instanceof Error ? error.message : String(error);
    }
}
