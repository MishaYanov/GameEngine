import type {ScriptProperty, ScriptReference, ScriptSettings} from '../../../../core/editor/editor-components';
import type {ScriptDescriptor, ScriptReferenceField} from '../../../../core/editor/object-scripts.models';
import {parseF32} from '../../../../core/editor/authoring-number';
import type {ComponentTarget} from './component-draft';

export interface ScriptDraft {
    id: string;
    properties: Record<string, string | boolean>;
    references: Record<string, ScriptReference>;
}

export function scriptDraft(settings?: ScriptSettings): ScriptDraft {
    return {
        id: settings?.script_id ?? '',
        properties: Object.fromEntries(Object.entries(settings?.properties ?? {}).map(
            ([key, value]) => [key, typeof value === 'number' ? String(value) : value],
        )),
        references: {...settings?.references},
    };
}

export function servesScriptReference(target: ComponentTarget, requires: ScriptReferenceField['requires']): boolean {
    if (requires === 'entity') return true;
    const components = target.component_views ?? target.components;
    if (components) return components.some(component => component.enabled && component.data.kind === 'collider' &&
        component.data.collider.interaction?.sensor === true);
    return target.collision_enabled !== false && target.collider?.interaction?.sensor === true;
}

export function scriptSettings(
    draft: ScriptDraft,
    descriptors: readonly ScriptDescriptor[],
    targets: readonly ComponentTarget[],
    previous?: ScriptSettings,
    slots: readonly string[] = [],
): ScriptSettings {
    const descriptor = descriptors.find(script => script.id === draft.id);
    if (!descriptor) {
        if (previous?.script_id === draft.id) return previous;
        throw new Error('Choose a script registered by this project.');
    }
    if (previous?.script_id === draft.id && previous.config_version !== descriptor.config_version) {
        throw new Error('This attachment uses an unsupported configuration version. Replace it with a current Script attachment.');
    }
    const properties: Record<string, ScriptProperty> = Object.create(null);
    for (const field of descriptor.fields) {
        const raw = draft.properties[field.key] ?? field.default;
        if (field.kind === 'number') {
            const value = parseF32(String(raw));
            if (value === null || value < field.min || value > field.max) {
                throw new Error(field.label + ' must be from ' + field.min + ' to ' + field.max + '.');
            }
            properties[field.key] = value;
        } else if (field.kind === 'boolean' && typeof raw === 'boolean') properties[field.key] = raw;
        else if (field.kind === 'text' && typeof raw === 'string' && new TextEncoder().encode(raw).length <= field.max_bytes) {
            properties[field.key] = raw;
        } else throw new Error(field.label + ' has an invalid value.');
    }
    const references: Record<string, ScriptReference> = Object.create(null);
    for (const field of descriptor.references) {
        const reference = draft.references[field.key];
        const compatible = reference && ('slot' in reference
            ? slots.includes(reference.slot)
            : targets.some(target => target.key === reference.entity && servesScriptReference(target, field.requires)));
        if (!compatible) {
            throw new Error('Choose a compatible ' + field.label + '.');
        }
        references[field.key] = reference;
    }
    return {script_id: descriptor.id, config_version: descriptor.config_version, properties, references};
}
