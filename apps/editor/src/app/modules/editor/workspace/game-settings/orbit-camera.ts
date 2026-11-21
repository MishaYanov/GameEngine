import {parseF32} from '../../../../core/editor/authoring-number';
import type {Camera, OrbitCamera} from '../../../../core/editor/editor.models';

export interface OrbitDraft {
    target: string;
    pivot: { x: string; y: string; z: string };
    yawDegrees: string;
    pitchDegrees: string;
    minimumPitchDegrees: string;
    maximumPitchDegrees: string;
    distance: string;
    minimumDistance: string;
    maximumDistance: string;
    followSeconds: string;
    recoverySeconds: string;
    teleportDistance: string;
    probeRadius: string;
    obstructionMask: string;
}

export function orbitDraft(value: OrbitCamera | null | undefined,
                           target = ''): OrbitDraft {
    const degrees = (radians: number) => String(radians * 180 / Math.PI);
    const pivot = value?.pivot_offset ?? [0, 0.7, 0];
    return {
        target: value?.target ?? target,
        pivot: {x: String(pivot[0]), y: String(pivot[1]), z: String(pivot[2])},
        yawDegrees: degrees(value?.initial_yaw ?? 0),
        pitchDegrees: degrees(value?.initial_pitch ?? -20 * Math.PI / 180),
        minimumPitchDegrees: degrees(value?.min_pitch ?? -80 * Math.PI / 180),
        maximumPitchDegrees: degrees(value?.max_pitch ?? 70 * Math.PI / 180),
        distance: String(value?.initial_distance ?? 5),
        minimumDistance: String(value?.min_distance ?? 0.5),
        maximumDistance: String(value?.max_distance ?? 10),
        followSeconds: String(value?.follow_seconds ?? 0.1),
        recoverySeconds: String(value?.recovery_seconds ?? 0.2),
        teleportDistance: String(value?.teleport_distance ?? 10),
        probeRadius: String(value?.probe_radius ?? 0.25),
        obstructionMask: String(value?.obstruction_mask ?? 0xffff_ffff),
    };
}

function bounded(raw: string, label: string, low: number, high: number): number {
    const parsed = parseF32(raw);
    const value = parsed === null ? NaN : Math.fround(parsed);
    if (!Number.isFinite(value) || value < Math.fround(low) || value > Math.fround(high)) {
        throw new Error(label + ' must be between ' + low + ' and ' + high + '.');
    }
    return value;
}

function angle(raw: string, label: string, low: number, high: number): number {
    const degrees = parseF32(raw);
    if (degrees === null || Math.fround(degrees) < low || Math.fround(degrees) > high) {
        throw new Error(label + ' must be between ' + low + ' and ' + high + ' degrees.');
    }
    return Math.fround(degrees * Math.PI / 180);
}

function mask(raw: string): number {
    if (!/^\d+$/.test(raw.trim())) {
        throw new Error('Obstacle layers must be a positive decimal integer.');
    }
    const value = Number(raw);
    if (!Number.isSafeInteger(value) || value < 1 || value > 0xffff_ffff) {
        throw new Error('Obstacle layers must fit a nonzero u32.');
    }
    return value;
}

// Mirrors MAX_GAME_ASPECT from 10.1. Keep a parity test for this contract.
const MAX_GAME_ASPECT = 3;

function roundPositiveF32Up(value: number): number {
    const rounded = Math.fround(value);
    if (!Number.isFinite(value) || value <= 0 || !Number.isFinite(rounded)) {
        throw new Error('Invalid projection envelope.');
    }
    if (rounded >= value) return rounded;
    const bytes = new ArrayBuffer(4);
    const view = new DataView(bytes);
    view.setFloat32(0, rounded, true);
    view.setUint32(0, view.getUint32(0, true) + 1, true);
    return view.getFloat32(0, true);
}

export function minimumProbeRadius(camera: Camera): number {
    if (!Number.isFinite(camera.near) || camera.near <= 0 ||
        !Number.isFinite(camera.vertical_fov) || camera.vertical_fov <= 0 ||
        camera.vertical_fov >= Math.PI) {
        throw new Error('Near plane and vertical field of view must be valid.');
    }
    const near = Math.fround(camera.near);
    const fov = Math.fround(camera.vertical_fov);
    if (near <= 0 || !Number.isFinite(near) || fov <= 0 || fov >= Math.PI) {
        throw new Error('Projection is not representable as a valid f32 camera.');
    }
    const halfHeight = near * Math.tan(fov / 2);
    const radius = Math.hypot(near, halfHeight,
        halfHeight * MAX_GAME_ASPECT) + 0.001;
    if (!Number.isFinite(radius) || radius > 2) {
        throw new Error('Projection is too wide for the supported camera probe.');
    }
    return roundPositiveF32Up(radius);
}

export function orbitFromDraft(draft: OrbitDraft, camera: Camera,
                               targets: readonly string[]): OrbitCamera {
    if (new TextEncoder().encode(draft.target).length > 256 ||
        /[\s\u0000-\u001f\u007f-\u009f]/u.test(draft.target) ||
        !targets.includes(draft.target)) {
        throw new Error('Choose a camera target in this scene.');
    }
    const minDistance = bounded(draft.minimumDistance, 'Minimum distance', 0.25, 100);
    const maxDistance = bounded(draft.maximumDistance, 'Maximum distance', 0.25, 100);
    const distance = bounded(draft.distance, 'Initial distance', 0.25, 100);
    if (minDistance > distance || distance > maxDistance) {
        throw new Error('Distance must satisfy minimum <= initial <= maximum.');
    }
    const minPitch = angle(draft.minimumPitchDegrees, 'Minimum pitch', -85, 85);
    const maxPitch = angle(draft.maximumPitchDegrees, 'Maximum pitch', -85, 85);
    const pitch = angle(draft.pitchDegrees, 'Initial pitch', -85, 85);
    if (minPitch >= maxPitch || pitch < minPitch || pitch > maxPitch) {
        throw new Error('Initial pitch must lie inside distinct pitch limits.');
    }
    const radius = bounded(draft.probeRadius, 'Probe radius', 0.01, 2);
    if (radius < minimumProbeRadius(camera)) {
        throw new Error('Probe radius is too small for the near plane and field of view.');
    }
    return {
        target: draft.target,
        pivot_offset: [
            bounded(draft.pivot.x, 'Pivot X', -100, 100),
            bounded(draft.pivot.y, 'Pivot Y', -100, 100),
            bounded(draft.pivot.z, 'Pivot Z', -100, 100),
        ],
        initial_yaw: angle(draft.yawDegrees, 'Initial yaw', -180, 180),
        initial_pitch: pitch,
        initial_distance: distance,
        min_distance: minDistance,
        max_distance: maxDistance,
        min_pitch: minPitch,
        max_pitch: maxPitch,
        follow_seconds: bounded(draft.followSeconds, 'Follow response', 0, 5),
        recovery_seconds: bounded(draft.recoverySeconds, 'Arm recovery', 0, 5),
        teleport_distance: bounded(draft.teleportDistance, 'Teleport threshold', 1, 1000),
        probe_radius: radius,
        obstruction_mask: mask(draft.obstructionMask),
    };
}
