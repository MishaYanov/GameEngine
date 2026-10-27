import {DestroyRef, effect, inject, Service, signal, untracked, WritableSignal,} from '@angular/core';
import type {Config} from '../models/config.model';
import {LayoutPreferencesService} from './layout-preferences.service';

/** Component-scoped behavior, composed through injection instead of base classes. */
@Service({autoProvided: false})
export class EditorComponentService {
    private readonly preferences = inject(LayoutPreferencesService);
    readonly error = this.preferences.error;
    private readonly destroyRef = inject(DestroyRef);
    private readonly initializedState = signal(false);
    readonly initialized = this.initializedState.asReadonly();

    connectConfig<T extends object>(
        key: () => string,
        config: WritableSignal<Config<T>>,
        defaults: T,
        decode: (value: unknown) => Config<T>,
    ): void {
        let activeKey: string | undefined;
        let baseline: string | undefined;
        let destroyed = false;
        effect((onCleanup) => {
            const id = key();
            const initial = untracked(() => ({...defaults, ...decode(config())}));
            let active = true;
            this.initializedState.set(false);
            onCleanup(() => {
                active = false;
            });
            void this.preferences.initialize(id, initial, decode).then((saved) => {
                if (!active || destroyed) return;
                // Edits made while preferences are loading take precedence over the response.
                const current = {...defaults, ...decode(config())};
                const edited = Object.fromEntries(
                    Object.entries(current).filter(
                        ([name, value]) => JSON.stringify(value) !== JSON.stringify(initial[name as keyof T]),
                    ),
                ) as Config<T>;
                baseline = JSON.stringify(saved);
                activeKey = id;
                config.set({...saved, ...edited});
                this.initializedState.set(true);
            });
        });
        const save = () => {
            if (!this.initialized() || activeKey !== key()) return;
            const current = {...defaults, ...decode(config())};
            baseline = this.preferences.saveConfig(activeKey, current, baseline);
        };
        effect(save);
        this.destroyRef.onDestroy(() => {
            destroyed = true;
            save();
            void this.preferences.flush().catch(() => undefined);
        });
    }
}
