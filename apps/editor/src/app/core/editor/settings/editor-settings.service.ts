import {Service, signal} from '@angular/core';
import type {EditorSettingsState} from './editor-settings.models';

/** Editor preferences UI state has an application lifetime, independent of the open project. */
@Service()
export class EditorSettingsService {
    private readonly current = signal<EditorSettingsState>({open: false, page: 'editor.keymap'});
    readonly state = this.current.asReadonly();

    setOpen(open: boolean): void {
        this.current.update(state => ({...state, open}));
    }

    select(page: string | null): void {
        if (page === 'editor' || page === 'editor.keymap') this.current.update(state => ({...state, page}));
    }
}
