import {
    afterNextRender, Component, computed, DestroyRef, effect, ElementRef, inject, Injector, input, linkedSignal, signal,
    viewChild,
} from '@angular/core';
import {disabled, form, FormField, required} from '@angular/forms/signals';
import {EditorService} from '../../../../core/editor/editor.service';
import {EditorSettingsService} from '../../../../core/editor/settings/editor-settings.service';
import {ViewportEditorService} from '../../../../core/editor/viewport-editor.service';
import type {EditorWarmingMode} from '../../../../core/editor/viewport-editor.models';
import {EditorDialog} from '../../../../shared/components/dialog/dialog';
import {SceneEditingService} from '../scene-editing.service';
import {SceneSwitchService} from './scene-switch.service';
import {SceneVerification} from './scene-verification';
import {rolesForScene} from '../../../../core/editor/gameplay-modules';
import type {EditorCommand, SceneId} from '../../../../core/editor/editor.models';

interface WarmingContext {
    readonly generation: number;
    readonly nativeGeneration: number | null;
    readonly mode: EditorWarmingMode;
    readonly supported: boolean;
}

@Component({
    selector: 'editor-scene-actions',
    imports: [EditorDialog, FormField, SceneVerification],
    templateUrl: './scene-actions.html',
    styleUrl: './scene-actions.scss',
})
export class SceneActions {
    readonly opening = input(false);
    protected readonly editor = inject(EditorService);
    protected readonly sceneSwitch = inject(SceneSwitchService);
    private readonly editing = inject(SceneEditingService, {optional: true});
    private readonly injector = inject(Injector);
    private readonly informationBody = viewChild<ElementRef<HTMLElement>>('informationBody');
    protected readonly informationOpen = linkedSignal({
        source: () => this.editor.workspaceGeneration(),
        computation: () => false,
    });
    protected readonly informationConfig = {width: '44rem'};
    private readonly pending = signal(false);
    private readonly viewport = inject(ViewportEditorService);
    private readonly settings = inject(EditorSettingsService);
    private readonly nativeGeneration = computed(() => this.viewport.state()?.workspace_generation ?? null);
    private readonly warmingContext = computed<WarmingContext>(() => ({
        generation: this.editor.workspaceGeneration(),
        nativeGeneration: this.nativeGeneration(),
        mode: this.settings.sceneWarmingMode(),
        supported: this.editor.capabilities()?.scene_warming_supported === true &&
            this.settings.sceneWarmingReady(),
    }));
    protected readonly warming = linkedSignal({source: this.warmingContext, computation: () => false});
    private readonly appliedMode = signal<{ context: WarmingContext; revision: number } | null>(null);
    private readonly warmingFailure = signal<{ context: WarmingContext; message: string } | null>(null);
    protected readonly warmingError = computed(() => {
        const failure = this.warmingFailure();
        return failure?.context === this.warmingContext() ? failure.message : null;
    });
    protected readonly warmTarget = linkedSignal({
        source: this.warming,
        computation: (): SceneId | null => null,
    });
    private warmRequest = Promise.resolve();
    private readonly modeRevision = computed(() => {
        const applied = this.appliedMode();
        return applied?.context === this.warmingContext() ? applied.revision : null;
    });
    private warmSequence = 0;
    protected readonly prepareAvailable = computed(() => this.available() && this.modeRevision() !== null &&
        this.warmingContext().mode !== 'off');
    protected readonly reportAvailable = computed(() => this.editor.nativeAvailable &&
        this.editor.capabilities()?.scene_warming_supported === true && this.nativeGeneration() !== null);
    protected readonly reportPending = signal(false);
    protected readonly reportStatus = linkedSignal({
        source: () => [this.editor.workspaceGeneration(), this.nativeGeneration()],
        computation: (): string | null => null,
    });
    protected readonly expanded = signal(false);
    protected readonly message = signal<string | null>(null);
    protected readonly error = signal<string | null>(null);
    protected readonly confirmationConfig = {draggable: false, resizable: false, expandable: false};
    protected readonly available = computed(() => this.editor.nativeAvailable && !!this.editor.snapshot() &&
        !this.opening() && !this.pending() && !this.sceneSwitch.pending() && (this.editing?.ready() ??
            (!this.editor.busy() && !this.editor.playing() && !this.editor.snapshot()?.transaction)));
    protected readonly warmingResult = computed(() => {
        const state = this.viewport.state();
        const snapshot = this.editor.snapshot();
        const result = state?.scene_warming_result;
        return result && snapshot?.mode === 'edit' && !this.sceneSwitch.pending() &&
        result.scene_id === snapshot.active_scene_id && result.scene_revision === snapshot.scene_revision &&
        result.mode_revision === this.modeRevision() ? result : null;
    });
    private modeRequest = Promise.resolve();
    private destroyed = false;
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
        const snapshot = this.editor.snapshot();
        return module && snapshot ? rolesForScene(module, snapshot.active_scene_id, snapshot.project.gameplay_settings) : [];
    });

    constructor() {
        effect(() => {
            this.editor.workspaceGeneration();
            this.warmSequence += 1;
            this.warming.set(false);
            this.warmTarget.set(null);
            this.error.set(null);
            this.message.set(null);
        });
        effect(() => this.syncWarmingMode(this.warmingContext()));
        inject(DestroyRef).onDestroy(() => {
            if (this.warming()) this.queueWarmScene(null);
            this.destroyed = true;
        });
    }

    show(): boolean {
        if (this.opening() || !this.editor.snapshot()) return false;
        this.informationOpen.set(true);
        return true;
    }

    revealBinding(name: string): void {
        const sceneId = this.editor.snapshot()?.active_scene_id;
        const generation = this.editor.workspaceGeneration();
        if (!this.show()) return;
        afterNextRender({
            read: () => {
                if (!this.informationOpen() || generation !== this.editor.workspaceGeneration() ||
                    sceneId !== this.editor.snapshot()?.active_scene_id) return;
                const body = this.informationBody()?.nativeElement;
                if (!body?.closest('dialog[open]')) return;
                const control = [...body.querySelectorAll<HTMLElement>('[data-scene-binding]')]
                        .find(element => element.dataset['sceneBinding'] === name) ??
                    body.querySelector<HTMLElement>('[data-gameplay-settings]');
                control?.scrollIntoView?.({block: 'nearest'});
                control?.focus({preventScroll: true});
            },
        }, {injector: this.injector});
    }

    protected switchScene(select: HTMLSelectElement): void {
        const id = select.value;
        const active = this.editor.snapshot()?.active_scene_id ?? '';
        if (!this.available()) {
            select.value = this.warmTarget() ?? active;
            return;
        }
        if (this.warming()) {
            this.error.set(null);
            this.message.set(null);
            this.warmTarget.set(id === active ? null : id);
            this.queueWarmScene(this.warmTarget());
            return;
        }
        select.value = active;
        this.error.set(null);
        this.message.set(null);
        void this.sceneSwitch.request(id);
    }

    protected toggleWarming(): void {
        if (!this.prepareAvailable()) return;
        const enabled = !this.warming();
        this.warming.set(enabled);
        this.warmTarget.set(null);
        if (!enabled) this.queueWarmScene(null);
    }

    protected async openPreparedScene(): Promise<void> {
        const target = this.warmTarget();
        const generation = this.editor.workspaceGeneration();
        if (!target || !this.available()) return;
        this.error.set(null);
        this.message.set(null);
        this.warmingFailure.set(null);
        this.warmSequence += 1;
        const opened = await this.sceneSwitch.request(target);
        if (this.editor.workspaceGeneration() !== generation || this.warmTarget() !== target) return;
        if (opened) {
            this.warmTarget.set(null);
        } else {
            this.queueWarmScene(null);
        }
    }

    protected async exportWarmingReport(): Promise<void> {
        const nativeGeneration = this.nativeGeneration();
        if (!this.reportAvailable() || this.reportPending() || nativeGeneration === null) return;
        const generation = this.editor.workspaceGeneration();
        const current = () => !this.destroyed && this.editor.workspaceGeneration() === generation &&
            this.nativeGeneration() === nativeGeneration;
        this.reportPending.set(true);
        this.reportStatus.set(null);
        try {
            const rows = await this.editor.exportWarmingReport(nativeGeneration);
            if (current() && rows !== null) {
                this.reportStatus.set(`Warming report exported with ${rows} ${rows === 1 ? 'row' : 'rows'}.`);
            }
        } catch (error) {
            if (current()) this.reportStatus.set(error instanceof Error ? error.message : String(error));
        } finally {
            this.reportPending.set(false);
        }
    }

    private queueWarmScene(target: SceneId | null): void {
        const context = this.warmingContext();
        const expectedGeneration = context.nativeGeneration;
        const modeRevision = this.modeRevision();
        const sequence = ++this.warmSequence;
        if (expectedGeneration === null || modeRevision === null) return;
        this.warmingFailure.set(null);
        const current = () => sequence === this.warmSequence && context === this.warmingContext();
        this.warmRequest = this.warmRequest.then(async () => {
            if (!current()) return;
            const snapshot = this.editor.snapshot();
            if (!snapshot) return;
            await this.editor.warmScene(target, snapshot.revision, expectedGeneration, modeRevision);
        }).catch(error => {
            if (current() && target !== null) {
                this.warmingFailure.set({context, message: error instanceof Error ? error.message : String(error)});
            }
        });
    }

    private syncWarmingMode(context: WarmingContext): void {
        const generation = context.nativeGeneration;
        if (!context.supported || generation === null) return;
        const current = () => !this.destroyed && context === this.warmingContext();
        this.modeRequest = this.modeRequest.then(async () => {
            if (!current()) return;
            const revision = await this.editor.setSceneWarmingMode(context.mode, generation);
            if (current()) this.appliedMode.set({context, revision});
        }).catch(error => {
            if (current()) {
                this.warmingFailure.set({context, message: error instanceof Error ? error.message : String(error)});
            }
        });
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
