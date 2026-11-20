import type {Collider, ColliderInteraction, Vec3} from '../../../../core/editor/editor.models';
import {parseF32} from '../../../../core/editor/authoring-number';

export type ColliderKind = 'none' | Collider['kind'];
export type ColliderNumberField = 'offset' | 'half_extents' | 'radius';
export type ColliderInteractionChange =
    | {readonly kind: 'sensor'; readonly sensor: boolean}
    | {readonly kind: 'filter'; readonly field: 'layers' | 'mask'; readonly value: number}
    | {readonly kind: 'layer'; readonly field: 'layers' | 'mask'; readonly bit: number; readonly enabled: boolean};

export const DEFAULT_COLLIDER_INTERACTION: ColliderInteraction = {sensor: false, layers: 1, mask: 0xffffffff};

export const COLLIDER_NUMBER_ERROR = 'Enter finite 32-bit values. Collider sizes must be positive.';

function uint32(value: number): boolean {
    return Number.isInteger(value) && value >= 0 && value <= 0xffffffff;
}

export function normalizeColliderInteraction(value?: Partial<ColliderInteraction>): ColliderInteraction {
    const sensor = value?.sensor === undefined ? DEFAULT_COLLIDER_INTERACTION.sensor : value.sensor;
    const layers = value?.layers === undefined ? DEFAULT_COLLIDER_INTERACTION.layers : value.layers;
    const mask = value?.mask === undefined ? DEFAULT_COLLIDER_INTERACTION.mask : value.mask;
    if (typeof sensor !== 'boolean' || !uint32(layers) || !uint32(mask)) {
        throw new Error('Collider layers and masks must be unsigned 32-bit integers.');
    }
    return {sensor, layers, mask};
}

export function colliderLayerEnabled(value: number, bit: number): boolean {
    if (!uint32(value) || !Number.isInteger(bit) || bit < 0 || bit > 31) {
        throw new Error('Choose a collision layer from 1 to 32.');
    }
    return ((value >>> bit) & 1) === 1;
}

export function changeColliderInteraction(collider: Collider, change: ColliderInteractionChange): Collider {
    const interaction = normalizeColliderInteraction(collider.interaction);
    if (change.kind === 'sensor') {
        return normalizeCollider({...collider, interaction: {...interaction, sensor: change.sensor}})!;
    }
    if (change.kind === 'filter') {
        return {...collider, interaction: normalizeColliderInteraction({...interaction, [change.field]: change.value})};
    }
    // Validate before JavaScript's bitwise operators coerce the layer and its index.
    colliderLayerEnabled(interaction[change.field], change.bit);
    const flag = (1 << change.bit) >>> 0;
    const value = change.enabled ? (interaction[change.field] | flag) >>> 0 : (interaction[change.field] & ~flag) >>> 0;
    return {...collider, interaction: {...interaction, [change.field]: value}};
}

function validF32(value: number, positive = false): boolean {
    const rounded = Math.fround(value);
    return Number.isFinite(value) && Number.isFinite(rounded) &&
        (value === 0 || rounded !== 0) && (!positive || rounded > 0);
}

export function parseColliderNumber(raw: string, positive = false): number | null {
    const value = parseF32(raw);
    return value !== null && (!positive || value > 0) ? value : null;
}

function vector(value: Vec3, positive = false): Vec3 {
    if (!value.every(component => validF32(component, positive))) throw new Error(COLLIDER_NUMBER_ERROR);
    return [...value];
}

export function normalizeCollider(value: Collider | null): Collider | null {
    if (value === null) return null;
    const offset = vector(value.offset);
    const interaction = value.interaction === undefined ? {} : {interaction: normalizeColliderInteraction(value.interaction)};
    if (value.kind === 'box') return {kind: 'box', half_extents: vector(value.half_extents, true), offset, ...interaction};
    if (!validF32(value.radius, true)) throw new Error(COLLIDER_NUMBER_ERROR);
    return {kind: 'sphere', radius: value.radius, offset, ...interaction};
}

export function defaultCollider(kind: ColliderKind, offset: Vec3 = [0, 0, 0], interaction?: ColliderInteraction): Collider | null {
    if (kind === 'none') return null;
    const settings = interaction === undefined ? {} : {interaction};
    if (kind === 'box') return normalizeCollider({kind, half_extents: [0.5, 0.5, 0.5], offset, ...settings});
    return normalizeCollider({kind, radius: 0.5, offset, ...settings});
}

export function sameCollider(first: Collider | null | undefined, second: Collider | null | undefined): boolean {
    if (first == null || second == null) return first == null && second == null;
    const a = normalizeColliderInteraction(first.interaction);
    const b = normalizeColliderInteraction(second.interaction);
    if (a.sensor !== b.sensor || a.layers !== b.layers || a.mask !== b.mask) return false;
    if (first.kind !== second.kind || first.offset.some((value, axis) => Math.fround(value) !== Math.fround(second.offset[axis]))) return false;
    if (first.kind === 'box' && second.kind === 'box') {
        return first.half_extents.every((value, axis) => Math.fround(value) === Math.fround(second.half_extents[axis]));
    }
    return first.kind === 'sphere' && second.kind === 'sphere' && Math.fround(first.radius) === Math.fround(second.radius);
}

export function colliderNumber(value: Collider, field: ColliderNumberField, axis: 0 | 1 | 2): number {
    if (field === 'offset') return value.offset[axis];
    if (field === 'half_extents' && value.kind === 'box') return value.half_extents[axis];
    return value.kind === 'sphere' ? value.radius : 0;
}

export function changeColliderNumber(
    collider: Collider, field: ColliderNumberField, axis: 0 | 1 | 2, value: number,
): Collider {
    if (field === 'radius' && collider.kind === 'sphere') return {...collider, radius: value};
    if (field === 'half_extents' && collider.kind === 'box') {
        const halfExtents: [number, number, number] = [...collider.half_extents];
        halfExtents[axis] = value;
        return {...collider, half_extents: halfExtents};
    }
    const offset: [number, number, number] = [...collider.offset];
    offset[axis] = value;
    return {...collider, offset};
}
