import {InjectionToken} from '@angular/core';
import {invoke, isTauri} from '@tauri-apps/api/core';
import {listen} from '@tauri-apps/api/event';

export interface EditorTransport {
    readonly available: boolean;
    onChangeReady?(handler: (cursor: number) => void): Promise<() => void>;
    invoke<T>(operation: string, arguments_?: Record<string, unknown>): Promise<T>;
}

export const EDITOR_TRANSPORT = new InjectionToken<EditorTransport>('Editor transport', {
    providedIn: 'root',
    factory: () => ({
        available: typeof window !== 'undefined' && isTauri(),
        invoke: (operation, arguments_) => invoke(operation, arguments_),
        onChangeReady: handler => listen<number>('editor://changes-ready', event => handler(event.payload)),
    }),
});
