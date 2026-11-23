import {Injectable, signal} from '@angular/core';
import type {EntityKey, SceneId} from '../../../../core/editor/editor.models';

export interface PropAssetReveal {
    readonly sourceId: string;
    readonly workspaceGeneration: number;
    readonly sceneId: SceneId;
}

interface PropCreationRequest {
    readonly workspaceGeneration: number;
    readonly sceneId: SceneId;
    readonly selection: readonly EntityKey[];
}

/** Routes Prop asset navigation and creation within one Editor workspace. */
@Injectable()
export class PropAssetSelection {
    private readonly state = signal<PropAssetReveal | null>(null);
    readonly request = this.state.asReadonly();
    private readonly creation = signal<PropCreationRequest | null>(null);
    readonly creationRequest = this.creation.asReadonly();

    create(request: PropCreationRequest): void {
        this.creation.set(request);
    }

    completeCreation(request: PropCreationRequest): void {
        if (this.creation() === request) this.creation.set(null);
    }

    reveal(source: PropAssetReveal): void {
        this.state.set(source);
    }

    complete(source: PropAssetReveal): void {
        if (this.state() === source) this.state.set(null);
    }
}
