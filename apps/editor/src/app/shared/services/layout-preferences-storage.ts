import {InjectionToken, type Signal} from '@angular/core';
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

/** Null is global, undefined is waiting for a project, and false allows a read-only fallback. */
export type WorkspaceLayoutScope = string | null | undefined | false;

export const WORKSPACE_LAYOUT_SCOPE = new InjectionToken<Signal<WorkspaceLayoutScope>>('WORKSPACE_LAYOUT_SCOPE');

export async function readWorkspaceLayoutScope(workspaceGeneration: number): Promise<string> {
    const identity = await invoke<unknown>('editor_layout_preferences_project_scope', {workspaceGeneration});
    if (typeof identity !== 'string' || !/^[0-9a-f]{64}$/.test(identity)) {
        throw new Error('The editor returned an invalid workspace layout identity.');
    }
    return identity;
}

export const LAYOUT_PREFERENCES_STORAGE = new InjectionToken<LayoutPreferencesStorage>(
    'LAYOUT_PREFERENCES_STORAGE',
    {
        providedIn: 'root',
        factory: () => createLayoutPreferencesStorage(isTauri()),
    },
);
