import {Component, computed, effect, inject, input, signal} from '@angular/core';
import {disabled, form, FormField, required} from '@angular/forms/signals';
import {EditorService} from '../../../../core/editor/editor.service';
import {EditorDialog} from '../../../../shared/components/dialog/dialog';
import {SceneEditingService} from '../scene-editing.service';
import {SceneSwitchService} from './scene-switch.service';
import type {EditorCommand} from '../../../../core/editor/editor.models';
import {courtyardScene, readCourtyardSettings} from '../game-settings/courtyard-settings';

@Component({
    selector: 'editor-scene-actions',
    imports: [EditorDialog, FormField],
    templateUrl: './scene-actions.html',
    styleUrl: './scene-actions.scss',
})
export class SceneActions {
    readonly opening = input(false);
    protected readonly editor = inject(EditorService);
    protected readonly sceneSwitch = inject(SceneSwitchService);
    private readonly editing = inject(SceneEditingService, {optional: true});
    private readonly pending = signal(false);
    protected readonly expanded = signal(false);
    protected readonly message = signal<string | null>(null);
    protected readonly error = signal<string | null>(null);
    protected readonly confirmationConfig = {draggable: false, resizable: false, expandable: false};
    protected readonly available = computed(() => this.editor.nativeAvailable && !!this.editor.snapshot() &&
        !this.opening() && !this.pending() && !this.sceneSwitch.pending() && (this.editing?.ready() ??
            (!this.editor.busy() && !this.editor.playing() && !this.editor.snapshot()?.transaction)));
    private readonly pathDraft = signal({path: ''});
    protected readonly fields = form(this.pathDraft, path => {
        required(path.path);
        disabled(path.path, () => !this.available());
    });
    protected readonly validPath = computed(() => {
        const path = this.pathDraft().path.trim();
        return path.endsWith('.json') && !path.startsWith('/') && !/[:\\\p{Cc}]/u.test(path) &&
            path.split('/').every(part => part !== '' && part !== '.' && part !== '..');
    });

    protected readonly gameplayModules = computed(() => this.editor.capabilities()?.gameplay_modules);
    protected readonly selectedModuleId = computed(() => this.editor.snapshot()?.project.gameplay_module ?? '');
    protected readonly selectedModule = computed(() =>
        this.gameplayModules()?.find(module => module.id === this.selectedModuleId()),
    );
    protected readonly unavailableModule = computed(() => this.selectedModuleId() !== '' && !this.selectedModule());
    protected readonly gameplayRoles = computed(() => {
        const module = this.selectedModule();
        const sceneId = this.editor.snapshot()?.active_scene_id;
        if (!module || sceneId === undefined) return [];
        if (module.id === 'courtyard_run') {
            const settings = readCourtyardSettings(this.editor.snapshot()?.project.gameplay_settings);
            const configured = settings ? courtyardScene(settings, sceneId) : undefined;
            if (configured) {
                const names = configured.kind === 'finish' ? ['player', 'finish'] : ['player', 'key', 'exit'];
                const descriptors = configured.kind === 'finish'
                    ? [...Object.values(module.scene_roles).flat(), ...module.default_roles]
                    : [...module.default_roles, ...Object.values(module.scene_roles).flat()];
                return names.map(name => descriptors.find(role => role.name === name)).filter(role => role !== undefined);
            }
        }
        return Object.hasOwn(module.scene_roles, sceneId) ? module.scene_roles[sceneId] : module.default_roles;
    });

    constructor() {
        effect(() => {
            this.editor.workspaceGeneration();
            this.error.set(null);
            this.message.set(null);
        });
    }

    protected switchScene(select: HTMLSelectElement): void {
        const id = select.value;
        select.value = this.editor.snapshot()?.active_scene_id ?? '';
        if (!this.available()) return;
        this.error.set(null);
        this.message.set(null);
        void this.sceneSwitch.request(id);
    }

    protected async setStartup(): Promise<void> {
        await this.run(async () => {
            const id = this.editor.snapshot()!.active_scene_id;
            await this.editor.execute({type: 'set_startup_scene', scene_id: id});
            this.message.set('Startup scene changed. Save All to persist the project settings.');
        });
    }

    protected async sceneFile(action: 'register' | 'saveAs'): Promise<void> {
        if (!this.validPath()) return;
        const path = this.pathDraft().path.trim();
        await this.run(async () => {
            if (action === 'register') await this.editor.registerScene(path);
            else await this.editor.saveSceneAs(path);
            this.message.set(action === 'register'
                ? 'Scene registered. Save All to persist the project settings.'
                : 'Scene saved at the new path. Save All to persist the project settings.');
            this.pathDraft.set({path: ''});
        });
    }

    protected async setGameplayModule(select: HTMLSelectElement): Promise<void> {
        const selected = select.value;
        select.value = this.selectedModuleId();
        if (this.gameplayModules() === undefined || selected === this.selectedModuleId() ||
            (selected !== '' && !this.gameplayModules()?.some(module => module.id === selected))) return;
        const generation = this.editor.workspaceGeneration();
        await this.run(async () => {
            if (this.editor.workspaceGeneration() !== generation) return;
            const snapshot = this.editor.snapshot();
            if (!snapshot || snapshot.mode !== 'edit' || snapshot.transaction !== null) return;
            await this.editor.execute({
                type: 'set_project',
                project: {...snapshot.project, gameplay_module: selected || null},
            }, generation, snapshot.revision);
        });
    }

    protected async setRole(
        role: string,
        select: HTMLSelectElement,
    ): Promise<void> {
        const snapshot = this.editor.snapshot();
        if (!snapshot) return;

        // Capture the user's choice before resetting the control.
        const key = select.value;
        const generation = this.editor.workspaceGeneration();
        const sceneId = snapshot.active_scene_id;

        // Keep displaying the confirmed value until Rust accepts the edit.
        select.value = this.roleBinding(role) ?? '';
        if (!this.gameplayRoles().some(descriptor => descriptor.name === role)) return;

        const command: EditorCommand = {
            type: 'set_scene_binding',
            scene_id: sceneId,
            name: role,
            target: key === ''
                ? {kind: 'clear'}
                : {kind: 'assign', key},
        };

        await this.run(async () => {
            await this.editor.execute(command, generation);
        });
    }

    protected roleBinding(name: string): string | undefined {
        const bindings = this.editor.snapshot()?.document.bindings;
        return bindings && Object.hasOwn(bindings, name) ? bindings[name] : undefined;
    }

    private async run(action: () => Promise<void>): Promise<void> {
        if (!this.available()) return;
        const generation = this.editor.workspaceGeneration();
        this.pending.set(true);
        this.sceneSwitch.clearFeedback();
        this.error.set(null);
        this.message.set(null);
        try {
            if (this.editing) {
                if (!await this.editing.withCommittedEdit(action) && this.editor.workspaceGeneration() === generation) {
                    this.error.set(this.editing.error() ?? 'Finish the current edit and try again.');
                }
            }
            else await action();
        } catch (error) {
            if (this.editor.workspaceGeneration() === generation) {
                this.error.set(error instanceof Error ? error.message : String(error));
            }
        } finally {
            this.pending.set(false);
        }
    }
}
