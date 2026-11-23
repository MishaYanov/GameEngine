import {Component, computed, effect, inject, input, signal, viewChild} from '@angular/core';
import {applyEach, form, FormField, maxLength} from '@angular/forms/signals';
import {EditorService} from '../../../../core/editor/editor.service';
import {
    DEFAULT_GAME_INPUT,
    evaluateGameInput,
    GAME_KEYS,
    type GameInputProfile,
    gameKeyFromCode,
    MAX_GAME_ACTIONS,
    MOVEMENT_AXES,
    type MovementAxis,
    validateGameInput,
} from '../../../../core/game-input/game-input.models';
import {EditorDialog} from '../../../../shared/components/dialog/dialog';
import {SceneEditingService} from '../scene-editing.service';
import {InputActionsCatalog} from './input-actions-catalog.service';
import {InputActionsEditor} from './input-actions-editor';

interface ActionDraft {
    row: number;
    id: string;
    key: string;
    parameter: string;
}

interface InputDraft {
    movement: Record<MovementAxis, string>;
    actions: ActionDraft[];
}

@Component({
    selector: 'editor-game-input',
    imports: [EditorDialog, FormField, InputActionsEditor],
    templateUrl: './game-input-manager.html',
    styleUrl: './game-input-manager.scss',
    host: {'(window:blur)': 'clearPreview()'},
})
export class GameInputManager {
    readonly opening = input(false);
    protected readonly open = signal(false);
    protected readonly showingAssets = signal(false);
    protected readonly preferredAsset = signal<string | null>(null);
    protected readonly inputActions = inject(InputActionsCatalog, {optional: true});
    protected readonly pending = signal(false);
    protected readonly message = signal<string | null>(null);
    protected readonly error = signal<string | null>(null);
    protected readonly axes = MOVEMENT_AXES;
    protected readonly keys = GAME_KEYS;
    protected readonly actionLimit = MAX_GAME_ACTIONS;
    protected readonly dialogConfig = {width: '48rem'};
    protected readonly draft = signal<InputDraft>({movement: {...DEFAULT_GAME_INPUT.movement}, actions: []});
    protected readonly fields = form(this.draft, path => {
        applyEach(path.actions, action => {
            maxLength(action.id, 32);
            maxLength(action.parameter, 32);
        });
    });
    private readonly editor = inject(EditorService);
    private readonly editing = inject(SceneEditingService, {optional: true});
    readonly available = computed(() => {
        const snapshot = this.editor.snapshot();
        return this.editor.nativeAvailable && !this.opening() && !this.pending() && snapshot?.mode === 'edit' &&
            (this.editing?.ready() ?? (!this.editor.busy() && !snapshot.transaction));
    });
    private readonly heldKeys = signal<ReadonlySet<string>>(new Set());
    private readonly savedProfile = signal('');
    private projectGeneration = 0;
    private nextRow = 0;
    private readonly profile = computed<GameInputProfile>(() => {
        const {movement, actions} = this.draft();
        return {
            schema_version: 1,
            movement,
            actions: actions.map(({id, key, parameter}) => ({
                id: id.trim(), key, animation_parameter: parameter.trim() || null,
            })),
        };
    });
    protected readonly validation = computed(() => validateGameInput(this.profile()));
    protected readonly changed = computed(() => this.profileSignature(this.profile()) !== this.savedProfile());
    protected readonly preview = computed(() => evaluateGameInput(this.profile(), this.heldKeys()));
    protected readonly parameters = computed(() => [...this.preview().animation]);
    private readonly assetEditor = viewChild(InputActionsEditor);
    protected readonly editingPending = computed(() => this.pending() || this.assetEditor()?.pending() === true);
    protected readonly dirty = computed(() => this.showingAssets() ?
        this.assetEditor()?.dirty() === true : this.changed());
    protected readonly canSaveDraft = computed(() => this.showingAssets() ?
        this.assetEditor()?.savable() === true : this.available() && this.changed() && !this.validation());
    protected readonly closeReview = signal(false);

    constructor() {
        effect(() => {
            const generation = this.editor.workspaceGeneration();
            if (this.open() && (generation !== this.projectGeneration || this.editor.playing())) {
                this.open.set(false);
                this.clearPreview();
            }
        });
    }

    show(): void {
        const snapshot = this.editor.snapshot();
        if (!snapshot || this.open() || !this.available()) return;
        const profile = snapshot.project.input_profile ?? DEFAULT_GAME_INPUT;
        this.projectGeneration = this.editor.workspaceGeneration();
        this.savedProfile.set(this.profileSignature(profile));
        this.draft.set({
            movement: {...profile.movement},
            actions: (profile.actions ?? []).map(binding => ({
                row: this.nextRow++, id: binding.id, key: binding.key, parameter: binding.animation_parameter ?? '',
            })),
        });
        this.message.set(null);
        this.error.set(null);
        this.closeReview.set(false);
        this.clearPreview();
        const controller = snapshot.document?.entities.find(entity =>
            entity.key === snapshot.selection?.[0] && entity.player_controller?.enabled)?.player_controller
            ?? snapshot.document?.entities.find(entity =>
                entity.player_controller?.enabled && entity.player_controller.settings.local)?.player_controller;
        const input = this.inputActions?.supported() ? controller?.settings.input : undefined;
        const preferred = input?.kind === 'asset' ? input.asset_id : null;
        this.preferredAsset.set(preferred);
        this.showingAssets.set(preferred !== null);
        this.open.set(true);
    }

    protected showAssetTools(active: boolean): void {
        if (active === this.showingAssets() || this.dirty() || this.editingPending()) return;
        this.clearPreview();
        this.showingAssets.set(active);
    }

    protected reviewClose(): void {
        if (!this.editingPending()) this.closeReview.set(true);
    }

    protected discardAndClose(): void {
        if (this.editingPending()) return;
        if (this.showingAssets()) {
            this.assetEditor()?.discard();
            if (this.dirty()) return;
        }
        this.open.set(false);
    }

    protected async saveAndClose(): Promise<void> {
        if (!this.canSaveDraft()) return;
        if (this.showingAssets()) await this.assetEditor()?.save();
        else await this.apply();
        if (!this.dirty()) this.open.set(false);
    }

    protected didClose(): void {
        this.clearPreview();
        this.closeReview.set(false);
    }

    protected addAction(): void {
        if (this.draft().actions.length >= MAX_GAME_ACTIONS || this.pending()) return;
        this.draft.update(draft => ({
            ...draft, actions: [...draft.actions, {row: this.nextRow++, id: '', key: 'space', parameter: ''}],
        }));
    }

    protected removeAction(row: number): void {
        if (!this.pending()) this.draft.update(draft => ({
            ...draft,
            actions: draft.actions.filter(action => action.row !== row)
        }));
    }

    protected unlistedKey(key: string): boolean {
        return !GAME_KEYS.some(option => option.key === key);
    }

    protected previewKey(event: KeyboardEvent, pressed: boolean): void {
        if (event.isComposing || event.code === 'Escape' || event.code === 'Tab') return;
        const key = gameKeyFromCode(event.code);
        const profile = this.profile();
        if (!Object.values(profile.movement).includes(key) && !profile.actions?.some(action => action.key === key)) return;
        event.preventDefault();
        if (this.heldKeys().has(key) === pressed) return;
        this.heldKeys.update(held => {
            const next = new Set(held);
            if (pressed) next.add(key);
            else next.delete(key);
            return next;
        });
    }

    protected clearPreview(): void {
        if (this.heldKeys().size) this.heldKeys.set(new Set());
    }

    protected async apply(): Promise<void> {
        if (!this.open() || !this.available() || this.validation() || !this.changed()) return;
        const profile = this.profile();
        const generation = this.projectGeneration;
        const savedProfile = this.savedProfile();
        const current = () => this.open() && this.projectGeneration === generation &&
            this.editor.workspaceGeneration() === generation && this.editor.snapshot()?.mode === 'edit';
        this.pending.set(true);
        this.error.set(null);
        this.message.set(null);
        try {
            const write = async () => {
                const snapshot = this.editor.snapshot();
                if (!current() || !snapshot) {
                    throw new Error('Open the Game Input Manager again for the current project.');
                }
                if (this.profileSignature(snapshot.project.input_profile ?? DEFAULT_GAME_INPUT) !== savedProfile) {
                    throw new Error('Game bindings changed while this dialog was open. Reopen it to review the latest bindings.');
                }
                const accepted = await this.editor.execute({
                    type: 'set_project',
                    project: {...snapshot.project, input_profile: profile}
                }, generation, snapshot.revision);
                if (!current()) return;
                this.savedProfile.set(this.profileSignature(accepted.project.input_profile ?? DEFAULT_GAME_INPUT));
                this.message.set('Bindings applied. Use Save All to keep them with the project.');
            };
            if (this.editing) {
                if (!await this.editing.withCommittedEdit(write) && current()) {
                    this.error.set(this.editing.error() ?? 'Finish the current edit and try again.');
                }
            } else {
                await write();
            }
        } catch (error: unknown) {
            if (current()) this.error.set(error instanceof Error ? error.message : String(error));
        } finally {
            this.pending.set(false);
        }
    }

    private profileSignature(profile: GameInputProfile): string {
        return JSON.stringify([
            MOVEMENT_AXES.map(axis => profile.movement[axis.field]),
            (profile.actions ?? []).map(({id, key, animation_parameter}) => [id, key, animation_parameter || null]),
        ]);
    }
}
