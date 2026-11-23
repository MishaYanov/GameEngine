import {inject, Service, signal} from '@angular/core';
import {isConfigObject} from '../../../shared/models/config.model';
import {LayoutPreferencesService} from '../../../shared/services/layout-preferences.service';
import type {EditorWarmingMode} from '../viewport-editor.models';
import {type EditorSettingsState, isEditorWarmingMode} from './editor-settings.models';

const SCENE_WARMING_PREFERENCE_KEY = 'editor:scene-warming';

/** Editor preferences UI state has an application lifetime, independent of the open project. */
@Service()
export class EditorSettingsService {
    private readonly preferences = inject(LayoutPreferencesService);
    private readonly current = signal<EditorSettingsState>({open: false, page: 'editor.appearance'});
    readonly state = this.current.asReadonly();
    readonly persistenceError = this.preferences.error;
    private readonly warmingModeState = signal<EditorWarmingMode>('off');
    readonly sceneWarmingMode = this.warmingModeState.asReadonly();
    private readonly warmingReadyState = signal(false);
    readonly sceneWarmingReady = this.warmingReadyState.asReadonly();

    constructor() {
        void this.preferences.initialize<{ mode: EditorWarmingMode }>(
            SCENE_WARMING_PREFERENCE_KEY,
            {mode: 'off'},
            value => isConfigObject(value) && isEditorWarmingMode(value['mode']) ? {mode: value['mode']} : {},
        ).then(({mode}) => {
            this.warmingModeState.set(mode);
            this.warmingReadyState.set(true);
        });
    }

    setSceneWarmingMode(mode: string): void {
        if (!this.sceneWarmingReady() || !isEditorWarmingMode(mode) || mode === this.sceneWarmingMode()) return;
        this.preferences.saveConfig(SCENE_WARMING_PREFERENCE_KEY, {mode});
        this.warmingModeState.set(mode);
        void this.preferences.flush().catch(() => undefined);
    }

    setOpen(open: boolean): void {
        this.current.update(state => ({...state, open}));
    }

    select(page: string | null): void {
        if (page === 'editor' || page === 'editor.appearance' || page === 'editor.keymap') {
            this.current.update(state => ({...state, page}));
        }
    }
}
