export interface PhysicsLayer {
    readonly bit: number;
    readonly id: string;
    readonly label: string;
    readonly archived?: boolean;
}

export type LayerPair = readonly [number, number];

export interface PhysicsLayers {
    readonly layers: readonly PhysicsLayer[];
    readonly ignored_pairs: readonly LayerPair[];
}

export const PHYSICS_LAYER_COUNT = 32;
export const EMPTY_PHYSICS_LAYERS: PhysicsLayers = {layers: [], ignored_pairs: []};

const STANDARD_LAYERS = [
    {id: 'default', label: 'Default'}, {id: 'terrain', label: 'Terrain'},
    {id: 'world', label: 'World'}, {id: 'player', label: 'Player'},
    {id: 'prop', label: 'Prop'}, {id: 'trigger', label: 'Trigger'},
] as const;

// A disabled interaction between a pair of IDs. The remaining pairs are allowed.
const STANDARD_IGNORED = [
    [0, 5], [1, 1], [1, 2], [1, 5], [2, 2], [2, 5], [4, 5], [5, 5],
] as const;

export function layerPair(a: number, b: number): LayerPair {
    return a <= b ? [a, b] : [b, a];
}

export function layerPairKey(a: number, b: number): number {
    const [first, last] = layerPair(a, b);
    return first * PHYSICS_LAYER_COUNT + last;
}

export function standardWorldPreview(policy: PhysicsLayers): PhysicsLayers {
    const layers = [...policy.layers];
    const occupied = new Set(layers.map(layer => layer.bit));
    const matched = STANDARD_LAYERS.map((template, preferredBit) => {
        const same = layers.find(layer => layer.id === template.id);
        if (same?.archived) throw new Error('Reactivate ' + same.label + ' before applying Standard World.');
        if (same) return same.bit;
        let bit = preferredBit;
        if (occupied.has(bit)) {
            bit = 0;
            while (occupied.has(bit)) bit++;
        }
        if (bit >= PHYSICS_LAYER_COUNT) throw new Error('Standard World requires free collision layer bits.');
        occupied.add(bit);
        layers.push({...template, bit});
        return bit;
    });
    const presetBits = new Set(matched);
    const ignored: LayerPair[] = policy.ignored_pairs.filter(([a, b]) => !presetBits.has(a) || !presetBits.has(b));
    ignored.push(...STANDARD_IGNORED.map(([a, b]) => layerPair(matched[a], matched[b])));
    return {
        layers: layers.sort((a, b) => a.bit - b.bit),
        ignored_pairs: ignored.sort(([a, b], [c, d]) => layerPairKey(a, b) - layerPairKey(c, d)),
    };
}
