import {computed, DestroyRef, effect, inject, Injectable, signal} from '@angular/core';
import {CommandBuilder, type EditorCommand, type EntityKey, type SceneEntity, type Vec3} from '../../../core/editor/editor.models';
import {EditorService} from '../../../core/editor/editor.service';
import {ViewportEditorService} from '../../../core/editor/viewport-editor.service';

export type InspectorField = 'name' | 'position' | 'rotation' | 'scale';
export interface TransformFieldEdit {
    readonly field: Exclude<InspectorField, 'name'>;
    readonly axis: 0 | 1 | 2;
    readonly value: number;
}

interface FieldEdit {
    readonly field: InspectorField;
    readonly sceneId: string;
    readonly generation: number;
    readonly original: SceneEntity;
    readonly label: string;
    entity: SceneEntity;
    pending: EditorCommand | null;
    draining: boolean;
    cancelled: boolean;
    failed: boolean;
    changed: boolean;
}

/** Workspace drafts and edit lifetimes. Rust remains the document/history owner. */
@Injectable()
export class SceneEditingService {
    private readonly editor = inject(EditorService);
    private readonly viewport = inject(ViewportEditorService, {optional: true});
    readonly blocked = signal(false);
    private readonly draft = signal<{edit: FieldEdit; entity: SceneEntity} | null>(null);
    private readonly owner = signal<FieldEdit | null>(null);
    private readonly ownRequests = signal(0);
    private readonly rollbackFailed = signal(false);
    private readonly transitioning = signal(false);
    private readonly errorState = signal<string | null>(null);
    readonly error = this.errorState.asReadonly();
    readonly selectedEntity = computed(() => {
        const snapshot = this.editor.snapshot();
        const key = snapshot?.selection[0];
        return snapshot?.document.entities.find(entity => entity.key === key) ?? null;
    });
    readonly entity = computed(() => {
        const selected = this.selectedEntity();
        const draft = this.draft();
        const state = this.viewport?.state();
        const preview = state?.gesture === 'transform' ? state.transform_preview : null;
        if (selected && preview?.key === selected.key) return {...selected, transform: preview.transform};
        return draft?.entity.key === selected?.key ? draft?.entity ?? selected : selected;
    });
    readonly ownsTransaction = computed(() => this.owner() !== null);
    readonly hasChanges = computed(() => this.draft()?.edit.changed ?? false);
    readonly ready = computed(() => {
        const snapshot = this.editor.snapshot();
        return this.editor.nativeAvailable && !this.blocked() && !this.transitioning() &&
            snapshot?.mode === 'edit' &&
            (snapshot.transaction === null || this.ownsTransaction()) &&
            (!this.editor.busy() || this.ownRequests() > 0);
    });
    readonly editable = computed(() => this.ready() && !this.rollbackFailed());
    private active: FieldEdit | null = null;
    private queue: Promise<void> = Promise.resolve();
    private destroyed = false;

    constructor() {
        effect(() => {
            const edit = this.draft()?.edit ?? this.owner();
            if (edit && !edit.cancelled && (this.blocked() || !this.isCurrent(edit))) void this.cancel();
        });
        inject(DestroyRef).onDestroy(() => {
            this.destroyed = true;
            void this.cancel();
        });
    }

    begin(field: InspectorField): void {
        if (this.destroyed || !this.editable()) return;
        const entity = this.entity();
        if (!entity) return;
        if (this.active?.field === field && this.active.entity.key === entity.key) return;
        void this.commit();
        this.errorState.set(null);
        const edit: FieldEdit = {
            field, sceneId: this.editor.snapshot()!.active_scene_id,
            generation: this.editor.workspaceGeneration(),
            original: entity, entity, label: `Edit ${field}`,
            pending: null, draining: false, cancelled: false, failed: false, changed: false,
        };
        this.active = edit;
        this.draft.set({edit, entity});
    }

    rename(name: string): void {
        this.begin('name');
        const edit = this.active;
        if (!edit || edit.field !== 'name' || !this.editable()) return;
        // Keep incomplete text in the draft without sending an invalid rename.
        edit.entity = {...edit.entity, name};
        this.draft.set({edit, entity: edit.entity});
        if (!name.trim() || /\p{Cc}/u.test(name) || new TextEncoder().encode(name).length > 1024) {
            edit.pending = null;
            this.errorState.set('Use a visible name without control characters (up to 1,024 UTF-8 bytes).');
            return;
        }
        this.errorState.set(null);
        this.update(edit, CommandBuilder.rename(edit.entity.key, name));
    }

    transform(change: TransformFieldEdit): void {
        if (!Number.isFinite(change.value) || !Number.isFinite(Math.fround(change.value))) {
            this.errorState.set('Transform values must fit a finite 32-bit number.');
            return;
        }
        this.begin(change.field);
        const edit = this.active;
        if (!edit || edit.field !== change.field || !this.editable()) return;
        const values: [number, number, number] = [...edit.entity.transform[change.field]];
        values[change.axis] = change.value;
        edit.entity = {...edit.entity, transform: {...edit.entity.transform, [change.field]: values as Vec3}};
        this.errorState.set(null);
        this.update(edit, CommandBuilder.transforms([{key: edit.entity.key, transform: edit.entity.transform}], 'local'));
    }

    /** Finish queued preview updates before Save, Play, Undo, or selection changes. */
    commit(): Promise<boolean> {
        const edit = this.active;
        if (!edit) return this.enqueue(async () => !this.owner()?.failed);
        this.active = null;
        if (edit.field === 'name' && (!edit.entity.name.trim() || /\p{Cc}/u.test(edit.entity.name) ||
            new TextEncoder().encode(edit.entity.name).length > 1024)) edit.cancelled = true;
        return this.enqueue(async () => {
            if (edit.cancelled || this.destroyed || !this.isCurrent(edit)) {
                await this.rollback(edit);
                return !edit.failed;
            }
            if (edit.failed) return false;
            try {
                if (this.owner() === edit) {
                    await this.execute(CommandBuilder.commit());
                    this.owner.set(null);
                }
                this.clearDraft(edit);
                return true;
            } catch (error) {
                await this.rejectEdit(edit, error);
                return false;
            }
        });
    }

    cancel(): Promise<boolean> {
        const edit = this.active ?? this.draft()?.edit ?? this.owner();
        if (!edit) return this.enqueue(async () => true);
        if (this.active === edit) this.active = null;
        edit.cancelled = true;
        edit.pending = null;
        if (this.draft()?.edit === edit) this.draft.set({edit, entity: edit.original});
        return this.enqueue(async () => {
            try {
                await this.rollback(edit);
            } catch (error) {
                edit.failed = true;
                this.rollbackFailed.set(true);
                throw error;
            }
            return this.owner() !== edit;
        });
    }

    async select(key: EntityKey): Promise<void> {
        await this.withCommittedEdit(async () => {
            const snapshot = this.editor.snapshot();
            if (snapshot?.mode !== 'edit' || snapshot.transaction !== null ||
                !snapshot.document.entities.some(entity => entity.key === key)) return;
            if (snapshot.selection.length === 1 && snapshot.selection[0] === key) return;
            await this.execute(CommandBuilder.select([key]));
            this.errorState.set(null);
        });
    }

    /** Reserve the editing surface through the commit and the following action. */
    async withCommittedEdit(action: () => Promise<void>): Promise<boolean> {
        if (!this.ready()) return false;
        const completion = this.commit();
        this.transitioning.set(true);
        try {
            if (!await completion || this.destroyed || this.blocked()) return false;
            await action();
            return true;
        } catch (error) {
            this.report(error);
            return false;
        } finally {
            this.transitioning.set(false);
        }
    }

    private update(edit: FieldEdit, command: EditorCommand): void {
        edit.changed = edit.entity.name !== edit.original.name ||
            (['position', 'rotation', 'scale'] as const).some(field =>
                edit.entity.transform[field].some((value, axis) => value !== edit.original.transform[field][axis]));
        this.draft.set({edit, entity: edit.entity});
        // At most one preview command waits behind the command in flight.
        edit.pending = command;
        if (edit.draining) return;
        edit.draining = true;
        void this.enqueue(async () => {
            try {
                if (!this.isCurrent(edit) || this.destroyed) edit.cancelled = true;
                if (edit.cancelled || edit.failed) return true;
                if (this.owner() && this.owner() !== edit) {
                    throw new Error('Discard the previous edit before changing another property.');
                }
                if (!this.owner()) {
                    this.owner.set(edit);
                    // A rejected response may still have opened the transaction;
                    // rollback checks the recovered authoritative snapshot.
                    await this.execute(CommandBuilder.begin(edit.label));
                }
                while (edit.pending && !edit.cancelled && !edit.failed) {
                    const pending = edit.pending;
                    edit.pending = null;
                    await this.execute(pending);
                }
                return true;
            } catch (error) {
                await this.rejectEdit(edit, error);
                return false;
            } finally {
                edit.draining = false;
            }
        });
    }

    private isCurrent(edit: FieldEdit): boolean {
        const snapshot = this.editor.snapshot();
        return this.editor.workspaceGeneration() === edit.generation &&
            snapshot?.mode === 'edit' && snapshot.active_scene_id === edit.sceneId &&
            snapshot.selection[0] === edit.entity.key &&
            snapshot.document.entities.some(entity => entity.key === edit.entity.key);
    }

    private async rollback(edit: FieldEdit): Promise<void> {
        if (this.owner() === edit) {
            const snapshot = this.editor.snapshot();
            if (this.editor.workspaceGeneration() === edit.generation && snapshot?.mode === 'edit' &&
                snapshot.active_scene_id === edit.sceneId && snapshot.transaction === edit.label) {
                await this.execute(CommandBuilder.cancel());
            }
            this.owner.set(null);
            this.rollbackFailed.set(false);
        }
        this.clearDraft(edit);
    }

    private async rejectEdit(edit: FieldEdit, error: unknown): Promise<void> {
        edit.failed = true;
        edit.pending = null;
        if (this.active === edit) this.active = null;
        try {
            await this.rollback(edit);
        } catch (rollbackError) {
            this.rollbackFailed.set(true);
            this.report(`${error instanceof Error ? error.message : String(error)} Cancellation failed: ${String(rollbackError)}`);
            return;
        }
        this.report(error);
    }

    private clearDraft(edit: FieldEdit): void {
        if (this.draft()?.edit === edit) this.draft.set(null);
    }

    private async execute(command: EditorCommand): Promise<void> {
        this.ownRequests.update(count => count + 1);
        try {
            await this.editor.execute(command);
        } finally {
            this.ownRequests.update(count => count - 1);
        }
    }

    private enqueue(action: () => Promise<boolean>): Promise<boolean> {
        const result = this.queue.then(action).catch(error => {
            this.report(error);
            return false;
        });
        this.queue = result.then(() => undefined);
        return result;
    }

    private report(error: unknown): void {
        this.errorState.set(error instanceof Error ? error.message : String(error));
    }
}
