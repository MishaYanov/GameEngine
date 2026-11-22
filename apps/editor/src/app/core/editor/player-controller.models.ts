export type InputActionsRef = {readonly kind: 'project_default'};

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
    return Object.keys(value.input).length === 1 && 'kind' in value.input && value.input.kind === 'project_default';
}
