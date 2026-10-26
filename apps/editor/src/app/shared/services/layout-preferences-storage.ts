import {InjectionToken} from '@angular/core';
import {invoke, isTauri} from '@tauri-apps/api/core';
import type {LayoutPreferences} from '../models/config.model';

export interface LayoutPreferencesStorage {
    load(): Promise<unknown>;

    save(preferences: LayoutPreferences): Promise<void>;
}

export const LAYOUT_STORAGE_KEY = 'game-engine.editor.layout.v1';

export function createLayoutPreferencesStorage(
    native: boolean,
    nativeInvoke: (operation: string, args?: Record<string, unknown>) => Promise<unknown> = invoke,
): LayoutPreferencesStorage {
    return native
        ? {
            load: () => nativeInvoke('editor_layout_preferences_load'),
            async save(preferences) {
                await nativeInvoke('editor_layout_preferences_save', {preferences});
            },
        }
        : {
            async load() {
                const saved = globalThis.localStorage.getItem(LAYOUT_STORAGE_KEY);
                return saved === null ? null : (JSON.parse(saved) as unknown);
            },
            async save(preferences) {
                globalThis.localStorage.setItem(LAYOUT_STORAGE_KEY, JSON.stringify(preferences));
            },
        };
}

export const LAYOUT_PREFERENCES_STORAGE = new InjectionToken<LayoutPreferencesStorage>(
    'LAYOUT_PREFERENCES_STORAGE',
    {
        providedIn: 'root',
        factory: () => createLayoutPreferencesStorage(isTauri()),
    },
);
