import {Component, computed, effect, inject, input, signal} from '@angular/core';
import {form, FormField} from '@angular/forms/signals';
import {EditorService} from '../../../../core/editor/editor.service';
import {EditorDialog} from '../../../../shared/components/dialog/dialog';
import {SceneEditingService} from '../scene-editing.service';
import {courtyardScene, readCourtyardSettings, updateCourtyardScene, type CourtyardSceneSettings} from './courtyard-settings';
import {normalizeColliderInteraction} from '../collider/collider';

interface InteractionDraft {
    mode: 'legacy' | 'course' | 'finish';
    key: string;
    exit: string;
    destination: string;
    blocker: string;
    finish: string;
}

@Component({
    selector: 'editor-gameplay-settings',
    imports: [EditorDialog, FormField],
    templateUrl: './gameplay-settings.html',
    styleUrl: './settings.scss',
})
export class GameplaySettings {
    readonly opening = input(false);
    protected readonly editor = inject(EditorService);
    private readonly editing = inject(SceneEditingService, {optional: true});
    protected readonly open = signal(false);
    protected readonly pending = signal(false);
    protected readonly error = signal<string | null>(null);
    protected readonly message = signal<string | null>(null);
    protected readonly draft = signal<InteractionDraft>({mode: 'legacy', key: '', exit: '', destination: '', blocker: '', finish: ''});
    protected readonly fields = form(this.draft);
    protected readonly supported = computed(() => this.editor.snapshot()?.project.gameplay_module === 'courtyard_run');
    protected readonly settings = computed(() => readCourtyardSettings(this.editor.snapshot()?.project.gameplay_settings));
    protected readonly sensors = computed(() => this.editor.snapshot()?.document.entities.filter(entity =>
        entity.collision_enabled !== false && entity.collider?.interaction?.sensor) ?? []);
    protected readonly blockers = computed(() => this.editor.snapshot()?.document.entities.filter(entity => entity.collider && !entity.collider.interaction?.sensor) ?? []);
    protected readonly available = computed(() => this.supported() && !!this.settings() && !this.opening() && !this.pending() &&
        this.editor.nativeAvailable && this.editor.snapshot()?.mode === 'edit' &&
        (this.editing?.ready() ?? (!this.editor.busy() && !this.editor.snapshot()?.transaction)));
    protected readonly validation = computed(() => {
        const draft = this.draft();
        if (draft.mode === 'legacy') return null;
        const snapshot = this.editor.snapshot();
        if (!snapshot) return 'Open a scene to configure its interactions.';
        const role = (name: string) => Object.hasOwn(snapshot.document.bindings, name) ? snapshot.document.bindings[name] : undefined;
        const player = snapshot.document.entities.find(entity => entity.key === role('player'));
        if (!player?.collider || player.collision_enabled === false) return 'Assign a Player with an enabled collider in Gameplay Roles.';
        const playerInteraction = normalizeColliderInteraction(player.collider.interaction);
        const sensor = (key: string) => {
            const entity = this.sensors().find(entity => entity.key === key && entity.key !== player.key);
            if (!entity?.collider) return false;
            const interaction = normalizeColliderInteraction(entity.collider.interaction);
            return (interaction.mask & playerInteraction.layers) !== 0 && (playerInteraction.mask & interaction.layers) !== 0;
        };
        if (draft.mode === 'finish') return sensor(draft.finish) ? null : 'Choose an enabled finish sensor whose filters accept Player.';
        if (!sensor(draft.key) || !sensor(draft.exit)) return 'Choose enabled key and exit sensors whose filters accept Player.';
        if (draft.key === draft.exit) return 'Choose different key and exit sensors.';
        if (!snapshot.scenes.some(scene => scene.id === draft.destination) || draft.destination === snapshot.active_scene_id) {
            return 'Choose a different registered destination scene.';
        }
        const settings = this.settings();
        const destination = settings ? courtyardScene(settings, draft.destination) : undefined;
        if (destination?.kind !== 'finish' && !(draft.destination === 'finish' && destination === undefined)) {
            return 'Configure Finish sensor settings on the destination scene first.';
        }
        if (draft.blocker && !this.blockers().some(entity => entity.key === draft.blocker)) return 'Choose a blocking collider for the exit blocker.';
        if (draft.blocker && [player.key, role('key'), draft.key, draft.exit].includes(draft.blocker)) {
            return 'The exit blocker must be separate from Player, Key, and both sensors.';
        }
        return null;
    });
    private context: {generation: number; sceneId: string; signature: string} | null = null;

    constructor() {
        effect(() => {
            const context = this.context;
            if (this.open() && context && (this.editor.workspaceGeneration() !== context.generation ||
                this.editor.snapshot()?.active_scene_id !== context.sceneId || this.editor.playing())) this.open.set(false);
        });
    }

    protected show(): void {
        const snapshot = this.editor.snapshot();
        const settings = this.settings();
        if (!snapshot || !settings || !this.available()) return;
        const current = courtyardScene(settings, snapshot.active_scene_id);
        this.context = {generation: this.editor.workspaceGeneration(), sceneId: snapshot.active_scene_id, signature: JSON.stringify(current ?? null)};
        this.draft.set({
            mode: current?.kind ?? 'legacy',
            key: current?.kind === 'course' ? current.key_trigger : '',
            exit: current?.kind === 'course' ? current.exit_trigger : '',
            destination: current?.kind === 'course' ? current.destination_scene : '',
            blocker: current?.kind === 'course' ? current.exit_blocker ?? '' : '',
            finish: current?.kind === 'finish' ? current.finish_trigger : '',
        });
        this.error.set(null);
        this.message.set(null);
        this.open.set(true);
    }

    protected async apply(): Promise<void> {
        const context = this.context;
        if (!context || !this.open() || !this.available() || this.validation()) return;
        const draft = this.draft();
        const value: CourtyardSceneSettings | null = draft.mode === 'course'
            ? {kind: 'course', key_trigger: draft.key, exit_trigger: draft.exit, destination_scene: draft.destination, exit_blocker: draft.blocker || null}
            : draft.mode === 'finish' ? {kind: 'finish', finish_trigger: draft.finish} : null;
        this.pending.set(true);
        this.error.set(null);
        const current = () => this.open() && this.editor.workspaceGeneration() === context.generation &&
            this.editor.snapshot()?.active_scene_id === context.sceneId && !this.editor.playing();
        try {
            const write = async () => {
                const snapshot = this.editor.snapshot();
                const settings = this.settings();
                if (!current() || !snapshot || !settings) return;
                if (JSON.stringify(courtyardScene(settings, context.sceneId) ?? null) !== context.signature) {
                    throw new Error('Interaction settings changed. Reopen this dialog to review them.');
                }
                if (JSON.stringify(value) === context.signature) return;
                await this.editor.execute({type: 'set_project', project: {
                    ...snapshot.project, gameplay_settings: updateCourtyardScene(settings, context.sceneId, value),
                }}, context.generation, snapshot.revision);
                if (current()) {
                    context.signature = JSON.stringify(value);
                    this.message.set('Interaction settings applied. Save All to persist the project.');
                }
            };
            if (this.editing) {
                if (!await this.editing.withCommittedEdit(write) && current()) this.error.set(this.editing.error() ?? 'Finish the current edit and try again.');
            } else await write();
        } catch (error) {
            if (current()) this.error.set(error instanceof Error ? error.message : String(error));
        } finally {
            this.pending.set(false);
        }
    }
}
