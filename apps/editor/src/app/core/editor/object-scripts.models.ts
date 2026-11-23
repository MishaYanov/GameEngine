export type ScriptField =
    | {readonly kind: 'number'; readonly key: string; readonly label: string;
        readonly default: number; readonly min: number; readonly max: number}
    | {readonly kind: 'boolean'; readonly key: string; readonly label: string; readonly default: boolean}
    | {readonly kind: 'text'; readonly key: string; readonly label: string; readonly default: string; readonly max_bytes: number};

export interface ScriptReferenceField {
    readonly key: string;
    readonly label: string;
    readonly requires: 'entity' | 'trigger_collider';
}

export interface ScriptDescriptor {
    readonly id: string;
    readonly label: string;
    readonly config_version: number;
    readonly fields: readonly ScriptField[];
    readonly references: readonly ScriptReferenceField[];
    readonly receive_local_actions?: boolean;
    readonly write_owner_transform?: boolean;
}

function record(value: unknown): value is Record<string, unknown> {
    return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function text(value: unknown): value is string {
    return typeof value === 'string' && value.length > 0 && value.length <= 128 && !/\p{Cc}/u.test(value);
}

function keyed(value: unknown): value is {readonly key: string; readonly label: string} & Record<string, unknown> {
    return record(value) && typeof value['key'] === 'string' && /^[A-Za-z0-9_-]{1,64}$/.test(value['key']) &&
        text(value['label']);
}

function fields(value: unknown): value is readonly ScriptField[] {
    if (!Array.isArray(value) || value.length > 32 || !value.every(keyed) ||
        new Set(value.map(field => field.key)).size !== value.length) return false;
    return value.every(field => {
        const initial = field['default'];
        switch (field['kind']) {
            case 'boolean': return typeof initial === 'boolean';
            case 'text': {
                const max = field['max_bytes'];
                return typeof initial === 'string' && typeof max === 'number' && Number.isInteger(max) &&
                    max >= 0 && max <= 256 && new TextEncoder().encode(initial).length <= max;
            }
            case 'number': {
                const min = field['min'];
                const max = field['max'];
                return typeof initial === 'number' && typeof min === 'number' && typeof max === 'number' &&
                    [initial, min, max].every(number => Number.isFinite(Math.fround(number))) &&
                    initial >= min && initial <= max;
            }
            default: return false;
        }
    });
}

export function validScriptDescriptors(value: unknown): value is readonly ScriptDescriptor[] {
    if (!Array.isArray(value) || value.length > 64) return false;
    return value.every(item => {
        if (!record(item) || !text(item['id']) || !item['id'].split('.').every(part => /^[A-Za-z0-9_-]{1,64}$/.test(part)) ||
            !text(item['label']) || typeof item['config_version'] !== 'number' ||
            !Number.isInteger(item['config_version']) || item['config_version'] < 1 || item['config_version'] > 4_294_967_295 ||
            !fields(item['fields'])) return false;
        const references = item['references'];
        return Array.isArray(references) && references.length <= 32 && references.every(keyed) &&
            new Set(references.map(reference => reference.key)).size === references.length &&
            references.every(reference => reference['requires'] === 'entity' || reference['requires'] === 'trigger_collider');
    }) && new Set(value.map(item => item.id)).size === value.length;
}
