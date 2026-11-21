import type {Camera} from '../../../../core/editor/editor.models';
import {cameraDraft as legacyDraft, type CameraDraft as FollowDraft, followFromDraft,} from './follow-camera';
import {orbitDraft, type OrbitDraft, orbitFromDraft} from './orbit-camera';

export interface GameCameraDraft {
    mode: 'fixed' | 'follow' | 'orbit';
    follow: FollowDraft;
    orbit: OrbitDraft;
}

export function gameCameraDraft(camera: Camera, target = ''): GameCameraDraft {
    return {
        mode: camera.orbit ? 'orbit' : camera.follow ? 'follow' : 'fixed',
        follow: legacyDraft(camera.follow, target),
        orbit: orbitDraft(camera.orbit, target),
    };
}

export function cameraFromDraft(base: Camera, draft: GameCameraDraft,
                                targets: readonly string[]): Camera {
    if (draft.mode === 'fixed') return {...base, follow: null, orbit: null};
    if (draft.mode === 'follow') {
        return {
            ...base,
            follow: followFromDraft({...draft.follow, mode: 'follow'}, targets),
            orbit: null,
        };
    }
    return {
        ...base, follow: null,
        orbit: orbitFromDraft(draft.orbit, base, targets),
    };
}

export function cameraSignature(camera: Camera): string {
    const f = camera.follow, o = camera.orbit;
    // Rust serializes f32s as short decimal strings. Compare at stored precision,
    // while keeping the full 32 bits of the integer layer mask.
    return JSON.stringify([
        camera.position, camera.yaw, camera.pitch,
        camera.vertical_fov, camera.near, camera.far,
        f ? [f.target, f.offset, f.look_offset, f.smoothing_seconds] : null,
        o ? [o.target, o.pivot_offset, o.initial_yaw, o.initial_pitch, o.initial_distance,
            o.min_distance, o.max_distance, o.min_pitch, o.max_pitch, o.follow_seconds,
            o.recovery_seconds, o.teleport_distance, o.probe_radius, String(o.obstruction_mask)] : null,
    ], (_, value: unknown) => typeof value === 'number' ? Math.fround(value) : value);
}
