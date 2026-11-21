import {Component, computed, input, model, output} from '@angular/core';
import {EDITOR_ACTIONS, type EditorActionId} from '../../../../core/editor/editor-actions';
import {editorShortcutLabel} from '../../../../core/editor/editor-keymap';
import {EditorDialog} from '../../../../shared/components/dialog/dialog';
import type {DialogSettings} from '../../../../shared/components/dialog/dialog-settings';
import {EditorSearchPicker, type SearchPickerOption} from '../../../../shared/components/search-picker/search-picker';
import type {Config} from '../../../../shared/models/config.model';

const commandIds = (Object.keys(EDITOR_ACTIONS) as EditorActionId[]).filter(id => id !== 'palette.open');

@Component({
    selector: 'editor-command-palette',
    imports: [EditorDialog, EditorSearchPicker],
    template: `
        <editor-dialog title="Command Palette" [(open)]="open" [config]="config"
                       [renderFields]="false" (closed)="runPending()">
            @if (open()) {
                <editor-search-picker label="Search commands" placeholder="Type a command…"
                                      [options]="commands()" [expanded]="true" [autofocus]="true"
                                      (picked)="choose($event)"/>
            }
        </editor-dialog>
    `,
})
export class CommandPalette {
    readonly open = model(false);
    readonly canRun = input.required<(id: EditorActionId) => boolean>();
    readonly commandRequested = output<EditorActionId>();
    protected readonly config: Config<DialogSettings> = {
        draggable: false, resizable: false, expandable: false, closeOnBackdrop: true,
    };
    protected readonly commands = computed<readonly SearchPickerOption[]>(() => {
        const canRun = this.canRun();
        return commandIds.map(id => {
            const shortcut = editorShortcutLabel(id);
            const disabled = !canRun(id);
            const action = EDITOR_ACTIONS[id];
            const description = [shortcut, disabled ? 'Unavailable' : ''].filter(Boolean).join(' · ');
            return {
                id,
                label: action.label,
                description,
                keywords: id.replace(/[.-]/g, ' '),
                disabled,
            };
        });
    });
    private pendingCommand: EditorActionId | null = null;

    protected choose(value: string): void {
        const id = commandIds.find(command => command === value);
        if (!id || !this.open() || this.pendingCommand || !this.canRun()(id)) return;
        this.pendingCommand = id;
        this.open.set(false);
    }

    protected runPending(): void {
        const id = this.pendingCommand;
        this.pendingCommand = null;
        if (id && this.canRun()(id)) this.commandRequested.emit(id);
    }
}
