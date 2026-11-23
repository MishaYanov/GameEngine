import {inject, Injectable} from '@angular/core';
import type {WorkspaceViewKind} from '../../../core/editor/workspace/workspace-view';
import {inspectorAddressKey, type InspectorSubject} from './inspector-panel/inspector-subject';
import {WorkspaceDraftReview} from './workspace-draft-review';

export type ScopedPaneKind = Exclude<WorkspaceViewKind, 'game-preview'> |
    'audio-workspace' | 'particle-designer' | 'behavior-workspace';

export interface ScopedPaneAsset {
    readonly id: string;
    readonly revision?: number;
    readonly fingerprint?: string;
}

export interface ScopedPaneTarget {
    readonly subject: InspectorSubject;
    readonly componentId?: string;
    readonly asset?: ScopedPaneAsset;
}

export interface OpenPaneRequest extends ScopedPaneTarget {
    readonly pane: ScopedPaneKind;
}

export interface PreparedScopedPane<T> {
    readonly value: T;
    readonly asset?: ScopedPaneAsset;
}

export interface ScopedPaneTransition<T> {
    /** Read the current subject, component, and binding facts for the captured address. */
    readonly resolve: (request: OpenPaneRequest) => ScopedPaneTarget | null;
    /** Prepare data without changing the active pane, focus, or preview. */
    readonly load: (target: ScopedPaneTarget) => Promise<PreparedScopedPane<T> | null>;
    /** Apply prepared data synchronously. Check stillCurrent again before deferred focus. */
    readonly activate: (prepared: T, target: ScopedPaneTarget, stillCurrent: () => boolean) => boolean;
}

function matchesAsset(expected: ScopedPaneAsset | undefined, actual: ScopedPaneAsset | undefined): boolean {
    if (!expected) return actual === undefined;
    return actual?.id === expected.id &&
        (expected.revision === undefined || actual.revision === expected.revision) &&
        (expected.fingerprint === undefined || actual.fingerprint === expected.fingerprint);
}

function subjectKey(subject: InspectorSubject): string {
    return inspectorAddressKey(subject.address, subject.scope.workspaceGeneration, subject.scope.nativeGeneration);
}

function matchesTarget(expected: ScopedPaneTarget, actual: ScopedPaneTarget): boolean {
    return subjectKey(expected.subject) === subjectKey(actual.subject) &&
        expected.subject.contentRevision === actual.subject.contentRevision &&
        expected.subject.access === actual.subject.access &&
        expected.componentId === actual.componentId && matchesAsset(expected.asset, actual.asset);
}

/** Workspace-scoped entry point for component panes opened across asynchronous draft reviews. */
@Injectable()
export class ScopedPaneRequests {
    private readonly reviews = inject(WorkspaceDraftReview);
    private sequence = 0;

    async open<T>(request: OpenPaneRequest, transition: ScopedPaneTransition<T>): Promise<boolean> {
        if (this.reviews.pending()) return false;
        const origin = this.reviews.capture();
        const {address, scope} = request.subject;
        if (scope.workspaceGeneration !== origin.generation ||
            (address.kind !== 'prop' && address.scene !== origin.sceneId)) return false;

        const capture = transition.resolve(request);
        if (!capture || !matchesTarget(request, capture)) return false;
        const sequence = ++this.sequence;
        const resolveCurrent = () => {
            if (sequence !== this.sequence || !this.reviews.current(origin)) return null;
            const current = transition.resolve(request);
            return current && matchesTarget(capture, current) ? current : null;
        };

        let opened = false;
        await this.reviews.run(origin, async () => {
            // Saving a different draft can advance the workspace revision during review.
            // The resolved target supplies that current revision to the loader.
            const target = resolveCurrent();
            if (!target) return;
            const prepared = await transition.load(target);
            const stillCurrent = () => resolveCurrent()?.subject.scope.revision === target.subject.scope.revision;
            if (!prepared || !matchesAsset(target.asset, prepared.asset) || !stillCurrent()) return;
            opened = transition.activate(prepared.value, target, stillCurrent);
        });
        return opened;
    }

    /** Invalidate a pending load or focus request when the workspace leaves the originating flow. */
    invalidate(): void {
        this.sequence++;
    }
}
