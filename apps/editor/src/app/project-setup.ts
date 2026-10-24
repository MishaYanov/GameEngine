import {Component, computed, inject, signal} from '@angular/core';
import {FormControl, FormGroup, ReactiveFormsModule, Validators} from '@angular/forms';
import {invoke, isTauri} from '@tauri-apps/api/core';
import {EngineService} from './core/engine/engine.service';
import {EngineStatus} from './core/engine/engine.models';

interface ProjectResult {
    path: string;
    launched: boolean;
    launchError: string | null;
}

@Component({
    selector: 'app-project-setup',
    imports: [ReactiveFormsModule],
    templateUrl: './app.html',
    styleUrl: './app.scss',
})
export class ProjectSetup {
    protected readonly status = signal<EngineStatus | null>(null);
    protected readonly busy = signal(false);
    protected readonly error = signal('');
    protected readonly result = signal<ProjectResult | null>(null);
    protected readonly desktop = isTauri();
    protected readonly buttonLabel = computed(() =>
        this.busy() ? 'Building project…' : 'Create & launch project',
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
    private readonly engine = inject(EngineService);

    async ngOnInit(): Promise<void> {
        if (this.desktop) {
            try {
                this.status.set(await this.engine.status());
            } catch {
                /* Project setup remains available if GPU status cannot be read. */
            }
        }
    }

    protected async createProject(): Promise<void> {
        if (this.busy()) return;
        this.form.markAllAsTouched();
        if (this.form.invalid || !this.desktop) return;
        this.busy.set(true);
        this.error.set('');
        this.result.set(null);
        this.form.disable();
        try {
            const values = this.form.getRawValue();
            this.result.set(
                await invoke<ProjectResult>('create_project', {
                    name: values.name.trim(),
                    destination: values.destination.trim(),
                }),
            );
        } catch (error: unknown) {
            this.error.set(String(error));
        } finally {
            this.busy.set(false);
            this.form.enable();
        }
    }
}
