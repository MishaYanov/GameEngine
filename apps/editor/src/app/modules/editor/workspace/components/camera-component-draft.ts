import {parseF32} from '../../../../core/editor/authoring-number';
import type {Camera, CameraComponentSettings} from '../../../../core/editor/editor.models';
import {cameraFromDraft, gameCameraDraft, type GameCameraDraft} from '../game-settings/camera-draft';

type CameraLens = CameraComponentSettings['lens'];

export interface CameraLensDraft {
    readonly fovDegrees: string;
    readonly near: string;
    readonly far: string;
}

export interface CameraComponentDraft {
    readonly lens: CameraLensDraft;
    readonly rig: GameCameraDraft;
}

const CAMERA_POSITION = {position: [0, 0, 0], yaw: 0, pitch: 0} as const;
const DEFAULT_LENS = {vertical_fov: Math.PI / 3, near: 0.1, far: 1000} satisfies CameraLens;

export function cameraComponentDraft(settings?: CameraComponentSettings): CameraComponentDraft {
    const lens = settings?.lens ?? DEFAULT_LENS;
    return {
        lens: {fovDegrees: fovText(lens.vertical_fov), near: String(lens.near), far: String(lens.far)},
        rig: gameCameraDraft(cameraProjection(lens, settings?.rig)),
    };
}

export function validateCameraLens(
    draft: CameraLensDraft, original?: CameraLens,
): {readonly lens: CameraLens | null; readonly errors: Readonly<Partial<Record<keyof CameraLensDraft, string>>>} {
    const degrees = parseF32(draft.fovDegrees);
    const verticalFov = original && draft.fovDegrees === fovText(original.vertical_fov)
        ? original.vertical_fov : degrees === null ? null : Math.fround(degrees * Math.PI / 180);
    const near = parseF32(draft.near);
    const far = parseF32(draft.far);
    const errors: Partial<Record<keyof CameraLensDraft, string>> = {};
    if (verticalFov === null || verticalFov <= 0 || verticalFov >= Math.PI) {
        errors.fovDegrees = 'Enter a vertical field of view between 0° and 180°.';
    }
    if (near === null || near <= 0) errors.near = 'Near clip must be greater than zero.';
    if (far === null || far <= 0 || (near !== null && far <= near)) {
        errors.far = 'Far clip must be greater than near clip.';
    }
    const lens = verticalFov !== null && near !== null && far !== null && !Object.keys(errors).length
        ? {vertical_fov: verticalFov, near, far} : null;
    return {lens, errors};
}

export function cameraComponentFromDraft(
    draft: CameraComponentDraft, targets: readonly string[], original?: CameraComponentSettings,
): CameraComponentSettings {
    const {lens, errors} = validateCameraLens(draft.lens, original?.lens);
    if (!lens) throw new Error(errors.fovDegrees ?? errors.near ?? errors.far ?? 'Enter a valid camera lens.');
    const camera = cameraFromDraft(cameraProjection(lens), draft.rig, targets);
    if (camera.follow) return {lens, rig: {mode: 'follow', settings: camera.follow}};
    if (camera.orbit) return {lens, rig: {mode: 'orbit', settings: camera.orbit}};
    return {lens, rig: {mode: 'fixed'}};
}

function cameraProjection(lens: CameraLens, rig?: CameraComponentSettings['rig']): Camera {
    return {
        ...CAMERA_POSITION, ...lens,
        follow: rig?.mode === 'follow' ? rig.settings : null,
        orbit: rig?.mode === 'orbit' ? rig.settings : null,
    };
}

function fovText(radians: number): string {
    return Number((radians * 180 / Math.PI).toPrecision(8)).toString();
}
