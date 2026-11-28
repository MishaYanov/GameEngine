import {computed, inject, Injectable, linkedSignal, signal} from '@angular/core';
import {EditorService} from '../../../core/editor/editor.service';
import type {SceneId} from '../../../core/editor/editor.models';
import type {WorkspaceViewKind} from '../../../core/editor/workspace/workspace-view';

export type NavigationDirection = 'back' | 'forward';

export type WorkspaceLocation =
    | { readonly kind: 'folder'; readonly path: string }
    | { readonly kind: 'scene'; readonly id: SceneId }
    | { readonly kind: 'view'; readonly view: WorkspaceViewKind; readonly origin: 'scene' | 'prop' };

interface Visit {
    readonly from: WorkspaceLocation;
    readonly to: WorkspaceLocation;
}

interface NavigationHistory {
    readonly visits: readonly Visit[];
    readonly cursor: number;
}

export type NavigationResult = 'applied' | 'unavailable' | 'cancelled';

const HISTORY_LIMIT = 64;

@Injectable()
export class WorkspaceNavigation {
    private readonly editor = inject(EditorService);
    private readonly history = linkedSignal({
        source: this.editor.workspaceGeneration,
        computation: (): NavigationHistory => ({visits: [], cursor: 0}),
    });
    private readonly moving = signal(false);
    readonly canGoBack = computed(() => !this.moving() && this.history().cursor > 0);
    readonly canGoForward = computed(() => !this.moving() && this.history().cursor < this.history().visits.length);

    record(from: WorkspaceLocation, to: WorkspaceLocation, direction?: NavigationDirection): void {
        if (sameLocation(from, to)) return;
        const {visits, cursor} = this.history();
        const previous = visits[cursor - 1];
        const next = visits[cursor];
        if (direction === 'back' && previous && sameLocation(previous.to, from) && sameLocation(previous.from, to)) {
            this.history.set({visits, cursor: cursor - 1});
        } else if (direction === 'forward' && next && sameLocation(next.from, from) && sameLocation(next.to, to)) {
            this.history.set({visits, cursor: cursor + 1});
        } else {
            const recorded = [...visits.slice(0, cursor), {from, to}].slice(-HISTORY_LIMIT);
            this.history.set({visits: recorded, cursor: recorded.length});
        }
    }

    async step(direction: NavigationDirection, apply: (target: WorkspaceLocation) => Promise<NavigationResult>): Promise<void> {
        if (this.moving()) return;
        const generation = this.editor.workspaceGeneration();
        const offset = direction === 'back' ? -1 : 1;
        this.moving.set(true);
        try {
            while (this.editor.workspaceGeneration() === generation) {
                const current = this.history();
                const visit = current.visits[current.cursor + Math.min(0, offset)];
                if (!visit) return;
                const result = await apply(direction === 'back' ? visit.from : visit.to);
                if (result === 'cancelled' || this.editor.workspaceGeneration() !== generation || this.history() !== current) return;
                this.history.set({...current, cursor: current.cursor + offset});
                if (result === 'applied') return;
            }
        } finally {
            this.moving.set(false);
        }
    }
}

function sameLocation(left: WorkspaceLocation, right: WorkspaceLocation): boolean {
    if (left.kind === 'folder' && right.kind === 'folder') return left.path === right.path;
    if (left.kind === 'scene' && right.kind === 'scene') return left.id === right.id;
    return left.kind === 'view' && right.kind === 'view' && left.view === right.view && left.origin === right.origin;
}

