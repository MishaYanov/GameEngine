import {validInputActionsId} from '../game-input/input-actions.models';

export type InputActionsRef = {readonly kind: 'project_default'} | {readonly kind: 'asset'; readonly asset_id: string};

export type PlayerMovementSpace = 'world' | 'camera_relative';

export interface PlayerControllerSettings {
    readonly input: InputActionsRef;
    readonly space: PlayerMovementSpace;
    readonly local: boolean;
}

export interface PlayerControllerAttachment {
    readonly id: string;
    readonly enabled: boolean;
    readonly settings: PlayerControllerSettings;
}

export function validPlayerControllerSettings(value: unknown): value is PlayerControllerSettings {
    if (!value || typeof value !== 'object' || Object.keys(value).length !== 3 ||
        !('space' in value) || (value.space !== 'world' && value.space !== 'camera_relative') ||
        !('local' in value) || typeof value.local !== 'boolean' ||
        !('input' in value) || !value.input || typeof value.input !== 'object') return false;
    const input = value.input;
    if (!('kind' in input)) return false;
    if (input.kind === 'project_default') return Object.keys(input).length === 1;
    return input.kind === 'asset' && Object.keys(input).length === 2 && 'asset_id' in input && validInputActionsId(input.asset_id);
}
