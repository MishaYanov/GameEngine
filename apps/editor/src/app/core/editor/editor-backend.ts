import {InjectionToken} from '@angular/core';
import {invoke, isTauri} from '@tauri-apps/api/core';
import {listen, type UnlistenFn} from '@tauri-apps/api/event';

/** Transport boundary shared by the native editor and development fixtures. */
export interface EditorBackend {
    readonly available: boolean;

    request<T>(operation: string, args?: Record<string, unknown>): Promise<T>;

    subscribe<T>(event: string, handler: (payload: T) => void): Promise<UnlistenFn>;
}

export const EDITOR_BACKEND = new InjectionToken<EditorBackend>('EDITOR_BACKEND', {
    providedIn: 'root',
    factory: () => ({
        available: typeof window !== 'undefined' && isTauri(),
        request: <T>(operation: string, args?: Record<string, unknown>) => invoke<T>(operation, args),
        subscribe: <T>(event: string, handler: (payload: T) => void) =>
            listen<T>(event, (message) => handler(message.payload)),
    }),
});
