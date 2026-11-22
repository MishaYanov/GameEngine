import {Injectable, signal} from '@angular/core';
import type {SceneId} from '../../../../core/editor/editor.models';

export interface PropAssetReveal {
    readonly sourceId: string;
    readonly workspaceGeneration: number;
    readonly sceneId: SceneId;
}

/** Routes an explicit source reveal from the Inspector to this workspace's Assets panel. */
@Injectable()
export class PropAssetSelection {
    private readonly state = signal<PropAssetReveal | null>(null);
    readonly request = this.state.asReadonly();

    reveal(source: PropAssetReveal): void {
        this.state.set(source);
    }

    complete(source: PropAssetReveal): void {
        if (this.state() === source) this.state.set(null);
    }
}
