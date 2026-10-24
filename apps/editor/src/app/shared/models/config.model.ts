/** A component's serializable settings. Omitted fields use the component defaults. */
export type Config<T extends object> = Partial<T>;

export interface LayoutPreferences {
    readonly schema_version: 1;
    readonly components: Record<string, Record<string, unknown>>;
}

export function isConfigObject(value: unknown): value is Record<string, unknown> {
    return typeof value === 'object' && value !== null && !Array.isArray(value);
}
