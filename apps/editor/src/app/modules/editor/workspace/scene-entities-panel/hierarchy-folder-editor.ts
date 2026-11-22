import {
    afterRenderEffect, Component, ElementRef, computed, input, linkedSignal, output, viewChild,
} from '@angular/core';
import type {EditorCommand, EntityKey, FolderId} from '../../../../core/editor/editor.models';

export type HierarchyFolderCommand = Extract<EditorCommand, {
    readonly type: 'create_folder' | 'rename_folder' | 'move_folder' | 'move_roots_to_folder' | 'remove_folder';
}>;

export type FolderEdit =
    | {readonly kind: 'create'; readonly parent: FolderId | null}
    | {readonly kind: 'rename'; readonly id: FolderId; readonly name: string}
    | {readonly kind: 'move-folder'; readonly id: FolderId; readonly name: string}
    | {readonly kind: 'move-objects'; readonly roots: readonly EntityKey[]};

export interface FolderDestination {
    readonly id: FolderId | null;
    readonly label: string;
}

@Component({
    selector: 'hierarchy-folder-editor',
    template: `
        <form (submit)="apply($event)" (keydown.escape)="close($event)">
            @if (isNameEdit()) {
                <label>
                    {{ edit().kind === 'create' ? 'New folder name' : 'Folder name' }}
                    <input #field [value]="name()" (input)="name.set(field.value)" type="text"/>
                </label>
            } @else {
                <label>
                    {{ edit().kind === 'move-folder' ? 'New parent folder' : 'Destination folder' }}
                    <select #field [value]="destination()" (change)="destination.set(field.value)">
                        @for (option of destinations(); track option.id) {
                            <option [value]="option.id ?? ''">{{ option.label }}</option>
                        }
                    </select>
                </label>
            }
            <div class="actions">
                <button [disabled]="disabled() || !valid()" type="submit">
                    {{ edit().kind === 'create' ? 'Create' : edit().kind === 'rename' ? 'Rename' : 'Move' }}
                </button>
                <button (click)="cancelled.emit()" type="button">Cancel</button>
            </div>
        </form>
    `,
    styles: `
        form {
            display: grid;
            gap: var(--space-1);
            padding: var(--space-2);
            border-top: 1px solid var(--color-pane-edge);
        }
        label {
            display: grid;
            gap: var(--space-1);
            color: var(--color-text-secondary);
        }
        input, select, button {
            min-height: 28px;
            padding: var(--space-1) var(--space-2);
            border: 1px solid var(--color-border-strong);
            border-radius: var(--radius-sm);
            background: var(--color-surface);
            color: var(--color-text);
            font: inherit;
            font-size: var(--font-size-xs);
        }
        input, select {min-width: 0;}
        button:disabled {color: var(--color-text-disabled);}
        .actions {display: flex; flex-wrap: wrap; gap: var(--space-1);}
    `,
})
export class HierarchyFolderEditor {
    readonly edit = input.required<FolderEdit>();
    readonly destinations = input<readonly FolderDestination[]>([]);
    readonly disabled = input(false);
    readonly cancelled = output<void>();
    readonly submitted = output<HierarchyFolderCommand>();

    private readonly field = viewChild<ElementRef<HTMLInputElement | HTMLSelectElement>>('field');
    protected readonly isNameEdit = computed(() => this.edit().kind === 'create' || this.edit().kind === 'rename');
    protected readonly name = linkedSignal(() => {
        const edit = this.edit();
        return edit.kind === 'rename' ? edit.name : '';
    });
    protected readonly destination = linkedSignal(() => this.destinations()[0]?.id ?? '');
    protected readonly valid = computed(() => {
        if (!this.isNameEdit()) return this.destinations().some(option => (option.id ?? '') === this.destination());
        const name = this.name().trim();
        return name.length > 0 && !/\p{Cc}/u.test(name) && new TextEncoder().encode(name).length <= 128;
    });

    constructor() {
        afterRenderEffect(() => {
            this.edit();
            const field = this.field()?.nativeElement;
            field?.focus();
            if (field instanceof HTMLInputElement) field.select();
        });
    }

    protected apply(event: SubmitEvent): void {
        event.preventDefault();
        if (this.disabled() || !this.valid()) return;
        const edit = this.edit();
        switch (edit.kind) {
            case 'create':
                this.submitted.emit({type: 'create_folder', name: this.name().trim(), parent: edit.parent});
                break;
            case 'rename':
                this.submitted.emit({type: 'rename_folder', id: edit.id, name: this.name().trim()});
                break;
            case 'move-folder':
                this.submitted.emit({type: 'move_folder', id: edit.id, parent: this.destination() || null});
                break;
            case 'move-objects':
                this.submitted.emit({type: 'move_roots_to_folder', roots: edit.roots, folder: this.destination() || null});
                break;
        }
    }

    protected close(event: Event): void {
        event.preventDefault();
        event.stopPropagation();
        this.cancelled.emit();
    }
}
