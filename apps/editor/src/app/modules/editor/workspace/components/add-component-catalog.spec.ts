import {describe, expect, it} from 'vitest';
import type {ComponentRecord, ComponentTypeDescriptor} from '../../../../core/editor/editor-components';
import {addComponentCatalog} from './add-component-catalog';

const descriptor = (kind: ComponentTypeDescriptor['kind'], max = 1): ComponentTypeDescriptor =>
    ({kind, type_id: 'engine.' + kind, label: kind, description: 'Test type', max_per_object: max});

describe('Add Component catalog', () => {
    it('includes inspector adapters and explains why an action is unavailable', () => {
        const collider: ComponentRecord = {id: 'box', schema_version: 1, enabled: true,
            data: {kind: 'collider', collider: {
                kind: 'box', half_extents: [1, 1, 1], offset: [0, 0, 0],
            }}};
        const choices = addComponentCatalog(
            [descriptor('collider'), descriptor('audio_source')],
            [collider], kind => kind === 'audio_source',
            [{id: 'animator'}, {id: 'controls_graph', unavailableReason: 'Add a Player Controller first.'}],
        );
        expect(choices.find(choice => choice.id === 'collider')?.unavailableReason).toContain('maximum');
        expect(choices.find(choice => choice.id === 'audio_source')?.unavailableReason).toContain('Prop source');
        expect(choices.find(choice => choice.id === 'animator')?.unavailableReason).toBeUndefined();
        expect(choices.find(choice => choice.id === 'controls_graph')?.unavailableReason).toContain('Player Controller');
    });
});
