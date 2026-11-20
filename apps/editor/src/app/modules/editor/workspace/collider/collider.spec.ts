import {
    changeColliderInteraction, colliderLayerEnabled, defaultCollider, DEFAULT_COLLIDER_INTERACTION,
    normalizeCollider, normalizeColliderInteraction, parseColliderNumber, sameCollider,
} from './collider';

describe('collider values', () => {
    it.each(['', '-', '1e-', 'NaN', 'Infinity', '1e40', '1e-50', '1e-999'])('rejects incomplete or unrepresentable input %s', raw => {
        expect(parseColliderNumber(raw)).toBeNull();
    });

    it('keeps decimal display values and compares using the engine precision', () => {
        expect(parseColliderNumber('1.1', true)).toBe(1.1);
        expect(parseColliderNumber('-2.5')).toBe(-2.5);
        expect(parseColliderNumber('0', true)).toBeNull();
        expect(parseColliderNumber('-1', true)).toBeNull();
        expect(sameCollider(
            {kind: 'sphere', offset: [0, 0, 0], radius: 1.1},
            {kind: 'sphere', offset: [0, 0, 0], radius: Math.fround(1.1)},
        )).toBe(true);
    });

    it('copies defaults and rejects invalid numeric sizes and offsets', () => {
        const first = defaultCollider('box')!;
        const second = normalizeCollider(first)!;
        expect(first).not.toBe(second);
        expect(first.offset).not.toBe(second.offset);
        expect(defaultCollider('sphere', [1, 2, 3])).toEqual({kind: 'sphere', radius: 0.5, offset: [1, 2, 3]});
        expect(defaultCollider('none')).toBeNull();
        expect(() => normalizeCollider({kind: 'sphere', radius: 1e-50, offset: [0, 0, 0]})).toThrow();
        expect(() => normalizeCollider({kind: 'sphere', radius: 1, offset: [1e-50, 0, 0]})).toThrow();
        expect(() => normalizeCollider({kind: 'box', half_extents: [1, 0, 1], offset: [0, 0, 0]})).toThrow();
    });

    it('defaults legacy interactions and treats explicit defaults as unchanged', () => {
        expect(normalizeColliderInteraction()).toEqual(DEFAULT_COLLIDER_INTERACTION);
        expect(normalizeColliderInteraction({sensor: true})).toEqual({...DEFAULT_COLLIDER_INTERACTION, sensor: true});
        const collider = defaultCollider('sphere')!;
        expect(sameCollider(collider, {...collider, interaction: {...DEFAULT_COLLIDER_INTERACTION}})).toBe(true);
    });

    it('sets and clears the highest unsigned bit without losing other layers', () => {
        const collider = defaultCollider('sphere')!;
        const highest = changeColliderInteraction(collider, {kind: 'layer', field: 'layers', bit: 31, enabled: true});
        expect(highest.interaction?.layers).toBe(2147483649);
        expect(colliderLayerEnabled(highest.interaction!.layers, 31)).toBe(true);
        const cleared = changeColliderInteraction(highest, {kind: 'layer', field: 'layers', bit: 0, enabled: false});
        expect(cleared.interaction?.layers).toBe(2147483648);
        const zero = changeColliderInteraction(cleared, {kind: 'layer', field: 'layers', bit: 31, enabled: false});
        expect(zero.interaction?.layers).toBe(0);
        expect(normalizeColliderInteraction({layers: 0, mask: 0})).toEqual({sensor: false, layers: 0, mask: 0});
        expect(collider.interaction).toBeUndefined();
    });

    it.each([-1, 0.5, 4294967296, NaN, Infinity])('rejects an invalid unsigned layer or mask %s', value => {
        expect(() => normalizeColliderInteraction({layers: value})).toThrow('unsigned 32-bit');
        expect(() => normalizeColliderInteraction({mask: value})).toThrow('unsigned 32-bit');
    });

    it('preserves interaction values through shape normalization and dimensions', () => {
        const interaction = {sensor: true, layers: 0x80000000, mask: 0};
        const box = defaultCollider('box', [1, 2, 3], interaction)!;
        expect(normalizeCollider(box)?.interaction).toEqual(interaction);
        expect(defaultCollider('sphere', box.offset, box.interaction)?.interaction).toEqual(interaction);
        expect(() => colliderLayerEnabled(1, 32)).toThrow();
    });

    it('sets or clears one unsigned filter group while preserving the other settings', () => {
        const original = defaultCollider('box', [1, 2, 3], {sensor: true, layers: 0x80000001, mask: 7})!;
        const all = changeColliderInteraction(original, {kind: 'filter', field: 'mask', value: 0xffffffff});
        expect(all).toEqual({...original, interaction: {sensor: true, layers: 0x80000001, mask: 0xffffffff}});
        const none = changeColliderInteraction(all, {kind: 'filter', field: 'layers', value: 0});
        expect(none).toEqual({...original, interaction: {sensor: true, layers: 0, mask: 0xffffffff}});
        expect(() => changeColliderInteraction(original, {kind: 'filter', field: 'mask', value: -1})).toThrow();
    });
});
