import type {Camera} from '../../../../core/editor/editor.models';
import {cameraFromDraft, cameraSignature, gameCameraDraft} from './camera-draft';
import {minimumProbeRadius, orbitDraft, orbitFromDraft} from './orbit-camera';

const base: Camera = {position: [0, 2, 5], yaw: 0, pitch: 0, vertical_fov: Math.PI / 3, near: 0.1, far: 100};
const targets = ['actor'];
describe('authored orbit camera', () => {
    it.each(['', '-', '1junk', '1e-99', 'Infinity'])('rejects incomplete or unrepresentable distance %j', value => {
        expect(() => orbitFromDraft({
            ...orbitDraft(null, 'actor'),
            distance: value
        }, base, targets)).toThrow('distance');
    });

    it('rounds the 3:1 near-plane envelope upward and accepts only an adequate radius', () => {
        const r = minimumProbeRadius(base);
        const near = Math.fround(base.near), height = near * Math.tan(Math.fround(base.vertical_fov) / 2);
        const required = Math.sqrt(near ** 2 + height ** 2 + (height * 3) ** 2) + 0.001;
        expect(r).toBeGreaterThanOrEqual(required);
        const bits = new Float32Array([r]);
        new Uint32Array(bits.buffer)[0]--;
        expect(bits[0]).toBeLessThan(required);
        const draft = orbitDraft(null, 'actor');
        expect(() => orbitFromDraft({...draft, probeRadius: String(bits[0])}, base, targets)).toThrow('Probe radius');
        expect(orbitFromDraft({...draft, probeRadius: String(r)}, base, targets).probe_radius).toBe(r);
    });

    it('checks targets, nonzero u32 layer masks, distances and pitch relationships', () => {
        const d = orbitDraft(null, 'actor');
        for (const invalid of [{target: 'missing'}, {minimumDistance: '6'}, {pitchDegrees: '84'},
            {minimumPitchDegrees: '70'}, {obstructionMask: '0'}, {obstructionMask: '4294967296'},
            {obstructionMask: '1e2'}, {pivot: {x: '101', y: '0', z: '0'}}]) {
            expect(() => orbitFromDraft({...d, ...invalid}, base, targets)).toThrow();
        }
    });

    it('preserves native f32 camera values on reopen, including yaw and pitch endpoints', () => {
        for (const sign of [1, -1]) {
            const orbit = orbitFromDraft({
                ...orbitDraft(null, 'actor'),
                yawDegrees: String(sign * 180), minimumPitchDegrees: '-85', maximumPitchDegrees: '85',
                pitchDegrees: String(sign * 85)
            }, base, targets);
            const camera = {...base, orbit};
            expect(cameraSignature(cameraFromDraft(camera, gameCameraDraft(camera), targets))).toBe(cameraSignature(camera));
        }
        expect(cameraSignature({
            ...base,
            orbit: {...orbitFromDraft(orbitDraft(null, 'actor'), base, targets), obstruction_mask: 0xfffffffe}
        }))
            .not.toBe(cameraSignature({...base, orbit: orbitFromDraft(orbitDraft(null, 'actor'), base, targets)}));
    });
});
