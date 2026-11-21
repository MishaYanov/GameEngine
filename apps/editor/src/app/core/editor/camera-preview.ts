import {signal} from '@angular/core';
import type {Camera, PlayCameraInputCommand} from './editor.models';

export type PreviewIntent = Pick<PlayCameraInputCommand, 'yaw_delta' | 'pitch_delta' | 'log_distance_delta'>;

export interface PreviewDefinition {
    camera: Camera;
    expectedGeneration: number;
    expectedRevision: number;
    sceneId: string;
}

export interface CameraPreviewReply {
    token: string;
    workspace_generation: number;
    scene_id: string;
    revision: number;
    sample: number;
    effective_distance: number;
    required_probe_radius: number;
    target_lost: boolean;
    blocker: string | null;
}

function counter(value: unknown): boolean {
    return typeof value === 'number' && Number.isSafeInteger(value) && value >= 0;
}

function within(value: unknown, min: number, max: number): boolean {
    return typeof value === 'number' && Number.isFinite(value) && value >= min && value <= max;
}

export function readCameraPreview(value: unknown): CameraPreviewReply {
    if (!value || typeof value !== 'object') throw new Error('Invalid camera preview reply.');
    const r = value as Record<string, unknown>;
    if (typeof r['token'] !== 'string' || !r['token'] || r['token'].length > 128 ||
        typeof r['scene_id'] !== 'string' || !r['scene_id'] || !counter(r['workspace_generation']) ||
        !counter(r['revision']) || !counter(r['sample']) || !within(r['effective_distance'], 0, 100) ||
        !within(r['required_probe_radius'], 0.001, 2) || typeof r['target_lost'] !== 'boolean' ||
        !(r['blocker'] === null || typeof r['blocker'] === 'string' && r['blocker'].length > 0)) {
        throw new Error('Invalid camera preview reply.');
    }
    return value as CameraPreviewReply;
}

interface PreviewApi {
    open(definition: PreviewDefinition): Promise<CameraPreviewReply>;

    sample(token: string, nextSample: number, intent: PreviewIntent): Promise<CameraPreviewReply>;

    close(token: string): Promise<void>;
}

// One owner per EditorService, including when a dialog is destroyed and another opens.
// A pending open and a pending sample each occupy at most one slot.
export class CameraPreviewController {
    readonly state = signal<CameraPreviewReply | null>(null);
    readonly error = signal<string | null>(null);
    private wanted: PreviewDefinition | null = null;
    private active: { owner: PreviewDefinition; reply: CameraPreviewReply } | null = null;
    private intent: PreviewIntent | null = null;
    private running = false;
    private unknownOpen: PreviewDefinition | null = null;

    constructor(private readonly api: PreviewApi) {
    }

    open(definition: PreviewDefinition): void {
        const unknown = this.unknownOpen;
        if (unknown && unknown.expectedGeneration === definition.expectedGeneration &&
            unknown.expectedRevision === definition.expectedRevision && unknown.sceneId === definition.sceneId) {
            this.error.set('The native preview could not be confirmed. Reopen the scene or workspace before previewing again.');
            return;
        }
        this.unknownOpen = null;
        this.wanted = definition;
        this.intent = null;
        this.error.set(null);
        this.state.set(null);
        void this.drain();
    }

    close(owner?: PreviewDefinition): void {
        if (owner && this.wanted && this.wanted !== owner) return;
        this.wanted = null;
        this.intent = null;
        this.state.set(null);
        this.error.set(null);
        void this.drain();
    }

    sample(intent: PreviewIntent): void {
        if (!this.active || this.active.owner !== this.wanted || !this.state()) return;
        if (![intent.yaw_delta, intent.pitch_delta, intent.log_distance_delta].every(Number.isFinite)) return;
        const add = (key: keyof PreviewIntent, limit: number) =>
            Math.max(-limit, Math.min(limit, (this.intent?.[key] ?? 0) + intent[key]));
        this.intent = {
            yaw_delta: add('yaw_delta', Math.PI),
            pitch_delta: add('pitch_delta', Math.PI),
            log_distance_delta: add('log_distance_delta', Math.log(4)),
        };
        void this.drain();
    }

    private async drain(): Promise<void> {
        if (this.running) return;
        this.running = true;
        try {
            for (; ;) {
                const wanted = this.wanted;
                if (this.active && this.active.owner !== wanted) {
                    // Keep the token until close succeeds. A later request can retry cleanup.
                    await this.api.close(this.active.reply.token);
                    this.active = null;
                    continue;
                }
                if (!wanted) return;
                try {
                    if (!this.active) {
                        const reply = await this.api.open(wanted);
                        this.active = {owner: wanted, reply};
                        if (reply.workspace_generation !== wanted.expectedGeneration ||
                            reply.scene_id !== wanted.sceneId || reply.revision !== wanted.expectedRevision ||
                            reply.sample !== 0) {
                            if (this.wanted === wanted) {
                                this.wanted = null;
                                this.error.set('Camera preview opened in a different scene context.');
                            }
                            continue;
                        }
                    } else if (this.intent) {
                        const {reply: old} = this.active;
                        const intent = this.intent;
                        this.intent = null;
                        const reply = await this.api.sample(old.token, old.sample + 1, intent);
                        if (reply.token !== old.token || reply.sample !== old.sample + 1 ||
                            reply.revision !== old.revision || reply.scene_id !== old.scene_id ||
                            reply.workspace_generation !== old.workspace_generation) {
                            if (this.wanted === wanted) {
                                this.wanted = null;
                                this.state.set(null);
                                this.error.set('Camera preview context changed. Open the preview again.');
                            }
                            continue;
                        }
                        this.active.reply = reply;
                    } else return;
                    if (this.wanted === wanted) {
                        this.state.set(this.active.reply);
                        this.error.set(null);
                    }
                } catch (cause) {
                    this.intent = null;
                    // Only an explicit pre-commit rejection guarantees the sample is safe to retry.
                    // A worker error, broken reply or dropped transport can follow a committed change.
                    if (!(cause && typeof cause === 'object' && 'kind' in cause && cause.kind === 'validation' &&
                        'message' in cause && typeof cause.message === 'string')) {
                        if (!this.active) {
                            this.unknownOpen = wanted;
                            const current = this.wanted;
                            if (current && current.expectedGeneration === wanted.expectedGeneration &&
                                current.expectedRevision === wanted.expectedRevision && current.sceneId === wanted.sceneId) {
                                this.wanted = null;
                                this.error.set('The native preview could not be confirmed. Reopen the scene or workspace before previewing again.');
                            }
                        } else if (this.wanted === wanted) {
                            this.wanted = null;
                            this.state.set(null);
                            this.error.set('The native sample could not be confirmed. Open the preview again.');
                        }
                        continue;
                    }
                    if (this.wanted !== wanted) continue;
                    this.error.set(cause.message);
                    if (!this.state()) {
                        this.wanted = null;
                        continue;
                    }
                    // A rejected sample leaves the last accepted pose and sequence available for retry.
                    return;
                }
            }
        } catch (cause) {
            this.error.set('Could not close the camera preview. ' + (cause instanceof Error ? cause.message : String(cause)));
        } finally {
            this.running = false;
        }
    }
}
