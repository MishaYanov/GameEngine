import {DestroyRef, effect, inject, Service, signal, untracked, WritableSignal,} from '@angular/core';
import type {Config} from '../models/config.model';
import {LayoutPreferencesService} from './layout-preferences.service';
import {WORKSPACE_LAYOUT_SCOPE, type WorkspaceLayoutScope} from './layout-preferences-storage';

/** Component-scoped behavior, composed through injection instead of base classes. */
@Service({autoProvided: false})
export class EditorComponentService {
    private readonly preferences = inject(LayoutPreferencesService);
    private readonly workspaceScope = inject(WORKSPACE_LAYOUT_SCOPE, {optional: true});
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
        const scope = () => this.workspaceScope ? this.workspaceScope() : null;
        let activeTarget: {key: string; scope: WorkspaceLayoutScope} | undefined;
        let originalDefaults: T | undefined;
        let baseline: string | undefined;
        let destroyed = false;
        effect((onCleanup) => {
            const id = key();
            const project = scope();
            const before = untracked(() => ({...defaults, ...decode(config())}));
            // Each project starts from the component's configured defaults, not the previous project's layout.
            originalDefaults ??= before;
            this.initializedState.set(false);
            if (project === undefined) return;
            let active = true;
            onCleanup(() => {
                active = false;
            });
            void this.preferences.initialize(id, originalDefaults, decode, project || null).then(saved => {
                if (!active || destroyed || project !== scope() || id !== key()) return;
                // Edits made while this scope is loading take precedence over its response.
                const current = {...defaults, ...decode(config())};
                const edited = Object.fromEntries(
                    Object.entries(current).filter(
                        ([name, value]) => JSON.stringify(value) !== JSON.stringify(before[name as keyof T]),
                    ),
                ) as Config<T>;
                baseline = JSON.stringify(saved);
                activeTarget = {key: id, scope: project};
                config.set({...saved, ...edited});
                this.initializedState.set(true);
            });
        });
        const save = () => {
            const project = scope();
            const id = key();
            if (!this.initialized() || project === false || project === undefined ||
                activeTarget?.key !== id || activeTarget.scope !== project) return;
            const current = {...defaults, ...decode(config())};
            baseline = this.preferences.saveConfig(id, current, baseline, project);
        };
        effect(save);
        this.destroyRef.onDestroy(() => {
            destroyed = true;
            save();
            void this.preferences.flush().catch(() => undefined);
        });
    }
}
