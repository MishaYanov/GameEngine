import {computed, DestroyRef, effect, inject, Injectable, signal, untracked} from '@angular/core';
import {invoke} from '@tauri-apps/api/core';
import {listen, type UnlistenFn} from '@tauri-apps/api/event';
import type {EntityKey, SceneId} from '../../../../core/editor/editor.models';
import {EditorService} from '../../../../core/editor/editor.service';
import {ViewportEditorService} from '../../../../core/editor/viewport-editor.service';
import {WorkspaceViewService} from '../scene-viewport/workspace-view.service';
import {PropWorkspaceService} from './prop-workspace.service';

type PropPreviewTarget =
    | {readonly kind: 'source'; readonly draft_id: string}
    | {readonly kind: 'instance'; readonly scene_id: SceneId; readonly root_key: EntityKey};

export type PropPreviewEnvironment = 'neutral' | 'scene';

type PreviewState = {
    readonly token: string;
    readonly workspace_generation: number;
    readonly revision: number;
    readonly request_id: number;
    readonly selection: EntityKey | null;
};

interface PreviewOwner {
    readonly target: PropPreviewTarget;
    readonly generation: number;
    readonly key: string;
}

let nextPreviewRequest = 0;

/** Owns one native preview lease while the Prop tab is active. */
@Injectable()
export class PropPreviewService {
    private readonly editor = inject(EditorService);
    private readonly viewport = inject(ViewportEditorService);
    private readonly views = inject(WorkspaceViewService);
    private readonly props = inject(PropWorkspaceService);
    private readonly state = signal<PreviewState | null>(null);
    private readonly busy = signal(false);
    private readonly failure = signal<string | null>(null);
    private readonly previewEnvironment = signal<PropPreviewEnvironment>('neutral');
    private readonly environmentBusy = signal(false);
    private readonly environmentFailure = signal<string | null>(null);
    private readonly retryVersion = signal(0);
    private readonly subscriptions: UnlistenFn[] = [];
    private request = 0;
    private destroyed = false;

    readonly ready = computed(() => this.state() !== null && !this.failure());
    readonly currentState = this.state.asReadonly();
    readonly loading = this.busy.asReadonly();
    readonly error = this.failure.asReadonly();
    readonly environment = this.previewEnvironment.asReadonly();
    readonly environmentPending = this.environmentBusy.asReadonly();
    readonly environmentError = this.environmentFailure.asReadonly();
    readonly supportsEnvironment = computed(() =>
        this.editor.capabilities()?.commands?.some(command => command.name === 'set_scene_environment') ?? false,
    );
    private readonly owner = computed<PreviewOwner | null>(() => {
        const view = this.views.activeViewKind();
        const sourceAudio = view === 'audio-workspace' && this.views.audioOrigin() === 'prop';
        if (!this.editor.nativeAvailable || (view !== 'prop-workspace' && !sourceAudio) || this.editor.playing()) return null;
        const document = this.props.document();
        const snapshot = this.editor.snapshot();
        if (!document || !snapshot) return null;
        const generation = this.viewport.state()?.workspace_generation ??
            (document.kind === 'source' ? document.draft.workspace_generation : null);
        if (generation === null) return null;
        const target: PropPreviewTarget = document.kind === 'source'
            ? {kind: 'source', draft_id: document.draft.draft_id}
            : {kind: 'instance', scene_id: snapshot.active_scene_id, root_key: document.root};
        // Draft mutations refresh the existing native source preview and preserve its camera.
        const version = document.kind === 'source' ? null : snapshot.scene_revision;
        return {
            target, generation,
            key: JSON.stringify([target, generation, version, this.retryVersion()]),
        };
    }, {equal: (first, second) => first?.key === second?.key});

    constructor() {
        effect(() => {
            const owner = this.owner();
            untracked(() => this.synchronize(owner));
        });
        if (this.editor.nativeAvailable) {
            this.subscribe('editor://prop-preview-selection', value => {
                const state = readPreviewState(value);
                if (state && this.matches(state)) {
                    this.state.set(state);
                    if (state.selection) this.props.selectObject(state.selection);
                }
            });
            this.subscribe('editor://prop-preview-error', value => {
                const state = readPreviewState(value);
                if (state && this.matches(state) && typeof (value as {message?: unknown}).message === 'string') {
                    this.failure.set((value as {message: string}).message);
                }
            });
        }
        inject(DestroyRef).onDestroy(() => {
            this.destroyed = true;
            this.request++;
            for (const unsubscribe of this.subscriptions.splice(0)) unsubscribe();
            this.closeToken(this.state()?.token);
        });
    }

    retry(): void {
        if (this.owner()) this.retryVersion.update(value => value + 1);
    }

    async frame(): Promise<void> {
        const state = this.state();
        if (!state || !this.ready()) return;
        try {
            await invoke<void>('editor_prop_preview_frame', {token: state.token});
        } catch (error) {
            this.reportFailure(state, error);
        }
    }

    async setEnvironment(environment: PropPreviewEnvironment): Promise<void> {
        const current = this.state();
        if (!current || !this.ready() || !this.supportsEnvironment() || this.environmentPending() ||
            this.environment() === environment) return;
        this.environmentBusy.set(true);
        this.environmentFailure.set(null);
        try {
            await invoke<void>('editor_prop_preview_environment', {token: current.token, environment});
            if (this.matches(current)) this.previewEnvironment.set(environment);
        } catch (error) {
            if (this.matches(current)) this.environmentFailure.set(error instanceof Error ? error.message : String(error));
        } finally {
            if (this.matches(current)) this.environmentBusy.set(false);
        }
    }

    async select(key: EntityKey): Promise<void> {
        this.props.selectObject(key);
        const current = this.state();
        if (!current || !this.ready()) return;
        try {
            const selected = readPreviewState(await invoke('editor_prop_preview_select', {token: current.token, key}));
            if (selected && this.matches(selected) && this.props.selectedObject()?.key === selected.selection) {
                this.state.set(selected);
            }
        } catch (error) {
            this.reportFailure(current, error);
        }
    }

    private synchronize(owner: PreviewOwner | null): void {
        const request = ++nextPreviewRequest;
        this.request = request;
        this.closeToken(this.state()?.token);
        this.state.set(null);
        this.failure.set(null);
        this.previewEnvironment.set('neutral');
        this.environmentBusy.set(false);
        this.environmentFailure.set(null);
        const revision = this.editor.snapshot()?.revision;
        this.busy.set(owner !== null && revision !== undefined);
        if (!owner || revision === undefined) return;
        void this.open(owner, revision, request);
    }

    private async open(owner: PreviewOwner, revision: number, request: number): Promise<void> {
        try {
            const reply = await invoke('editor_prop_preview_open', {
                target: owner.target, expectedRevision: revision, expectedGeneration: owner.generation,
                requestId: request,
            });
            const state = readPreviewState(reply);
            if (!this.current(owner, request)) {
                this.closeToken(state?.token);
                return;
            }
            if (!state || state.workspace_generation !== owner.generation || state.revision !== revision ||
                state.request_id !== request) {
                this.closeToken(state?.token);
                throw new Error('The Prop preview returned a different view owner.');
            }
            this.state.set(state);
            if (state.selection) this.props.selectObject(state.selection);
            else {
                const selected = this.props.selectedObject()?.key;
                if (selected) void this.select(selected);
            }
        } catch (error) {
            if (this.current(owner, request)) this.failure.set(String(error));
        } finally {
            if (!this.destroyed && request === this.request) this.busy.set(false);
        }
    }

    private current(owner: PreviewOwner, request: number): boolean {
        return !this.destroyed && request === this.request && owner.key === this.owner()?.key;
    }

    private matches(other: PreviewState): boolean {
        const current = this.state();
        return !this.destroyed && current?.token === other.token &&
            current.workspace_generation === other.workspace_generation && current.revision === other.revision &&
            current.request_id === other.request_id;
    }

    private reportFailure(state: PreviewState, error: unknown): void {
        if (this.matches(state)) this.failure.set(String(error));
    }

    private subscribe(event: string, receiver: (value: unknown) => void): void {
        void listen<unknown>(event, ({payload}) => receiver(payload)).then(unlisten => {
            if (this.destroyed) unlisten();
            else this.subscriptions.push(unlisten);
        }).catch(() => undefined);
    }

    private closeToken(token?: string): void {
        if (token) void invoke<void>('editor_prop_preview_close', {token}).catch(() => undefined);
    }
}

function readPreviewState(value: unknown): PreviewState | null {
    if (typeof value !== 'object' || value === null) return null;
    const state = value as Partial<PreviewState>;
    if (typeof state.token !== 'string' || !state.token ||
        !Number.isSafeInteger(state.workspace_generation) || !Number.isSafeInteger(state.revision) ||
        !Number.isSafeInteger(state.request_id) ||
        (state.selection !== null && typeof state.selection !== 'string')) return null;
    return state as PreviewState;
}
