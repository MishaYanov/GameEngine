import type {ComponentRecord} from '../../../../core/editor/editor-components';
import type {EditorCommand, EntityKey} from '../../../../core/editor/editor.models';
import type {ComponentEditIntent} from './component-editor';

export function checkedComponentChange(
    components: readonly ComponentRecord[],
    intent: ComponentEditIntent,
    supported: (record: ComponentRecord) => boolean = () => true,
): {readonly id: string; readonly next: ComponentRecord | null} | null {
    const id = 'component' in intent ? intent.component.id : intent.component_id;
    const existing = components.find(component => component.id === id);
    if ('expected' in intent && intent.expected !== (existing ? JSON.stringify(existing) : null)) {
        throw new Error('This component changed. Discard the draft and review its current values.');
    }
    if (intent.kind === 'add' ? !!existing : !existing) {
        throw new Error('The attachment changed. Review the object’s current components.');
    }
    const next = 'component' in intent ? intent.component :
        intent.kind === 'enabled' && existing ? {...existing, enabled: intent.enabled} : null;
    if ((existing && !supported(existing)) || (next && !supported(next))) {
        throw new Error('This attachment cannot be changed in the current document.');
    }
    return intent.kind === 'enabled' && existing?.enabled === intent.enabled ? null : {id, next};
}

export function sceneComponentCommand(key: EntityKey, intent: ComponentEditIntent): EditorCommand {
    switch (intent.kind) {
        case 'add': return {type: 'add_component', key, component: intent.component};
        case 'replace': return {type: 'replace_component', key, component: intent.component};
        case 'remove': return {type: 'remove_component', key, component_id: intent.component_id};
        case 'enabled': return {type: 'set_component_enabled', key, component_id: intent.component_id, enabled: intent.enabled};
    }
}
