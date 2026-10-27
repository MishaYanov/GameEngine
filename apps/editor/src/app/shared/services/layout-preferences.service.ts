import {DestroyRef, inject, Service, signal} from '@angular/core';
import {Config, isConfigObject, LayoutPreferences} from '../models/config.model';
import {LAYOUT_PREFERENCES_STORAGE} from './layout-preferences-storage';

/** Shared cache and serialized, debounced persistence for all editor layout components. */
@Service()
export class LayoutPreferencesService {
    private readonly storage = inject(LAYOUT_PREFERENCES_STORAGE);
    private preferences: LayoutPreferences = {schema_version: 1, components: {}};
    private readonly serializedComponents = new Map<string, string>();
    private loadRequest: Promise<void> | undefined;
    private timer: ReturnType<typeof setTimeout> | undefined;
    private queue: Promise<void> = Promise.resolve();
    private version = 0;
    private savedVersion = 0;
    private submittedVersion = 0;
    private readonly readyState = signal(false);
    readonly ready = this.readyState.asReadonly();
    private readonly dirtyState = signal(false);
    readonly dirty = this.dirtyState.asReadonly();
    private readonly savingState = signal(false);
    readonly saving = this.savingState.asReadonly();
    private readonly errorState = signal<string | null>(null);
    readonly error = this.errorState.asReadonly();

    constructor() {
        inject(DestroyRef).onDestroy(() => {
            void this.flush().catch(() => undefined);
        });
    }

    load(): Promise<void> {
        this.loadRequest ??= Promise.resolve()
            .then(() => this.storage.load())
            .then((value) => {
                if (value === null || value === undefined) return;
                if (
                    !isConfigObject(value) ||
                    value['schema_version'] !== 1 ||
                    !isConfigObject(value['components'])
                ) {
                    throw new Error('Invalid layout preferences; component defaults will be used.');
                }
                this.preferences = {
                    schema_version: 1,
                    components: structuredClone(
                        Object.fromEntries(
                            Object.entries(value['components']).filter(
                                (entry): entry is [string, Record<string, unknown>] => isConfigObject(entry[1]),
                            ),
                        ),
                    ),
                };
            })
            .catch((cause: unknown) => {
                this.errorState.set(cause instanceof Error ? cause.message : String(cause));
            })
            .finally(() => this.readyState.set(true));
        return this.loadRequest;
    }

    async initialize<T extends object>(
        key: string,
        defaults: T,
        decode: (value: unknown) => Config<T>,
    ): Promise<T> {
        await this.load();
        return {...defaults, ...decode(structuredClone(this.preferences.components[key]))};
    }

    /** Return the comparison baseline so component owners can reuse this serialization. */
    saveConfig<T extends object>(key: string, value: T, baseline?: string): string {
        if (!this.ready()) throw new Error('Initialize layout preferences before changing them.');
        if (!key.trim()) throw new Error('A stable layout preference key is required.');
        const serialized = JSON.stringify(value);
        if (serialized === undefined) throw new Error('Layout config must be a JSON object.');
        if (serialized === baseline) return serialized;
        let previous = this.serializedComponents.get(key);
        if (previous === undefined && this.preferences.components[key] !== undefined) {
            previous = JSON.stringify(this.preferences.components[key]);
            this.serializedComponents.set(key, previous);
        }
        if (serialized === previous) return serialized;
        const copy: unknown = JSON.parse(serialized);
        if (!isConfigObject(copy)) throw new Error('Layout config must be a JSON object.');
        this.preferences = {
            ...this.preferences,
            components: {...this.preferences.components, [key]: copy},
        };
        this.serializedComponents.set(key, serialized);
        this.version += 1;
        this.dirtyState.set(true);
        clearTimeout(this.timer);
        this.timer = setTimeout(() => {
            void this.flush().catch(() => undefined);
        }, 300);
        return serialized;
    }

    flush(): Promise<void> {
        clearTimeout(this.timer);
        this.timer = undefined;
        if (this.version === this.savedVersion || this.version === this.submittedVersion)
            return this.queue;
        const version = this.version;
        const preferences = structuredClone(this.preferences);
        this.submittedVersion = version;
        this.queue = this.queue
            .catch(() => undefined)
            .then(async () => {
                this.savingState.set(true);
                try {
                    await this.storage.save(preferences);
                    this.savedVersion = version;
                    this.dirtyState.set(this.version !== version);
                    this.errorState.set(null);
                } catch (cause: unknown) {
                    if (this.submittedVersion === version) this.submittedVersion = this.savedVersion;
                    this.errorState.set(cause instanceof Error ? cause.message : String(cause));
                    throw cause;
                } finally {
                    this.savingState.set(false);
                }
            });
        return this.queue;
    }
}
