import {describe, expect, it} from 'vitest';
import type {ComponentRecord} from '../../../../core/editor/editor-components';
import {checkedComponentChange, sceneComponentCommand} from './component-actions';

const component: ComponentRecord = {id: 'collider-1', schema_version: 1, enabled: true,
    data: {kind: 'collider', collider: {kind: 'box', half_extents: [1, 1, 1], offset: [0, 0, 0]}}};

describe('Component lifecycle', () => {
    it('keeps the attachment identity and metadata when enabling and disabling', () => {
        const disable = {kind: 'enabled', component_id: component.id, enabled: false} as const;
        const change = checkedComponentChange([component], disable);
        expect(change).toEqual({id: component.id, next: {...component, enabled: false}});
        expect(sceneComponentCommand('object-1', disable)).toEqual({
            type: 'set_component_enabled', key: 'object-1', component_id: component.id, enabled: false,
        });
        expect(checkedComponentChange([change!.next!], disable)).toBeNull();
    });

    it('rejects a stale removal and a component outside the document’s supported types', () => {
        const remove = {kind: 'remove', component_id: component.id, expected: JSON.stringify(component)} as const;
        expect(() => checkedComponentChange([{...component, enabled: false}], remove)).toThrow('component changed');
        expect(() => checkedComponentChange([component], remove, () => false)).toThrow('current document');
        expect(checkedComponentChange([component], remove)).toEqual({id: component.id, next: null});
    });
});
