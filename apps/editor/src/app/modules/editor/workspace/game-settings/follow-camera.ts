import {parseF32} from '../../../../core/editor/authoring-number';
import type {FollowCamera, Vec3} from '../../../../core/editor/editor.models';

export interface CameraDraft {
    mode: 'fixed' | 'follow';
    target: string;
    offset: {x: string; y: string; z: string};
    look_offset: {x: string; y: string; z: string};
    smoothing: string;
}

export function cameraDraft(follow: FollowCamera | null | undefined, target = ''): CameraDraft {
    const values = (vector: Vec3) => ({x: String(vector[0]), y: String(vector[1]), z: String(vector[2])});
    return {
        mode: follow ? 'follow' : 'fixed', target: follow?.target ?? target,
        offset: values(follow?.offset ?? [0, 2, 5]),
        look_offset: values(follow?.look_offset ?? [0, 0, 0]),
        smoothing: String(follow?.smoothing_seconds ?? 0.15),
    };
}

export function followFromDraft(draft: CameraDraft, targets: readonly string[]): FollowCamera | null {
    if (draft.mode === 'fixed') return null;
    if (!targets.includes(draft.target)) throw new Error('Choose a camera target in this scene.');
    const vector = (input: CameraDraft['offset']): Vec3 => {
        const x = parseF32(input.x);
        const y = parseF32(input.y);
        const z = parseF32(input.z);
        if (x === null || y === null || z === null) throw new Error('Camera offsets must fit finite 32-bit numbers.');
        return [x, y, z];
    };
    const offset = vector(draft.offset);
    const look = vector(draft.look_offset);
    const direction = look.map((value, axis) => Math.fround(value - offset[axis]));
    const squared = Math.fround(direction.reduce((sum, value) => sum + value * value, 0));
    if (!Number.isFinite(squared) || squared <= 0 ||
        Math.abs(direction[1]) / Math.hypot(...direction) > Math.sin(89 * Math.PI / 180)) {
        throw new Error('The camera needs a finite viewing direction within 89 degrees of horizontal.');
    }
    const smoothing = parseF32(draft.smoothing);
    if (smoothing === null || smoothing < 0 || smoothing > 10) throw new Error('Smoothing must be between 0 and 10 seconds.');
    return {target: draft.target, offset, look_offset: look, smoothing_seconds: smoothing};
}
