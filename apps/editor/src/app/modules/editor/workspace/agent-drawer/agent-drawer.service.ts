import {DestroyRef, effect, inject, Injectable, signal, untracked} from '@angular/core';
import {invoke} from '@tauri-apps/api/core';
import {listen} from '@tauri-apps/api/event';
import {EditorService} from '../../../../core/editor/editor.service';
import {ViewportEditorService} from '../../../../core/editor/viewport-editor.service';
import type {AgentSelectionContext} from './agent-selection-context';

const STREAM_EVENT = 'editor-agent-session';
const MAX_QUESTION = 512;
const MAX_ANSWER = 32_768;

export interface AgentReference {
    readonly citation: {
        readonly corpus: 'engine-help' | 'active-project';
        readonly source_id: string;
        readonly relative_path: string;
        readonly heading: string;
        readonly ordinal: number;
        readonly source_revision: string;
        readonly content_hash: string;
    };
    readonly excerpt: string;
}

type FinishStatus = 'completed' | 'retrieval_only' | 'cancelled' | 'error';

type StreamEvent = {readonly id: string; readonly workspace_generation: number} & (
    | {readonly type: 'started'}
    | {readonly type: 'sources'; readonly citations: readonly AgentReference[]}
    | {readonly type: 'delta'; readonly text: string}
    | {readonly type: 'finished'; readonly status: FinishStatus; readonly message?: string}
);

export interface AgentAnswer {
    readonly id: string;
    readonly question: string;
    readonly selection: AgentSelectionContext | null;
    readonly text: string;
    readonly references: readonly AgentReference[];
    readonly status: 'starting' | 'streaming' | FinishStatus;
    readonly message?: string;
}

interface ActiveQuestion {
    readonly id: string;
    readonly scope: number;
    readonly native: number;
    readonly revision: number;
}

@Injectable()
export class AgentDrawerService {
    private readonly editor = inject(EditorService);
    private readonly viewport = inject(ViewportEditorService);
    private readonly destroy = inject(DestroyRef);
    private readonly history = signal<readonly AgentAnswer[]>([]);
    readonly answers = this.history.asReadonly();
    private readonly active = signal<ActiveQuestion | null>(null);
    readonly running = this.active.asReadonly();
    readonly failure = signal<string | null>(null);
    private ready: Promise<boolean> | null = null;
    private release: (() => void) | null = null;
    private scope = this.editor.workspaceGeneration();

    constructor() {
        effect(() => {
            const scope = this.editor.workspaceGeneration();
            const native = this.viewport.state()?.workspace_generation;
            const request = this.active();
            untracked(() => {
                const changed = this.scope !== scope;
                if (request && (native !== request.native || !this.current(request))) this.stop();
                if (changed) {
                    this.scope = scope;
                    this.failure.set(null);
                    this.history.set([]);
                }
            });
        });
        this.destroy.onDestroy(() => {
            this.stop();
            this.release?.();
        });
    }

    async start(question: string, selection: AgentSelectionContext | null): Promise<boolean> {
        const prompt = question.trim();
        const snapshot = this.editor.snapshot();
        const native = this.viewport.state()?.workspace_generation;
        const scope = this.editor.workspaceGeneration();
        if (!this.editor.nativeAvailable || !snapshot || native === undefined || native === null || this.active() ||
            this.destroy.destroyed || !prompt || [...prompt].length > MAX_QUESTION) return false;
        if (selection && (selection.subject.scope.workspaceGeneration !== scope ||
            selection.subject.scope.nativeGeneration !== native ||
            selection.subject.scope.revision !== snapshot.revision)) {
            this.failure.set('The selection changed. Refresh the selection and ask again.');
            return false;
        }

        const request: ActiveQuestion = {id: crypto.randomUUID(), scope, native, revision: snapshot.revision};
        const captured = selection ? structuredClone(selection) : null;
        this.failure.set(null);
        this.active.set(request);
        this.history.update(history => [...history.slice(-11), {
            id: request.id, status: 'starting', selection: captured, question: prompt, text: '', references: [],
        }]);
        try {
            if (!await this.connect()) throw new Error('The native Agent event stream could not be opened.');
            if (this.active() !== request || !this.current(request)) return false;
            await invoke<void>('editor_agent_session_start', {request: {
                id: request.id, question: prompt, workspace_generation: native, revision: request.revision,
                ...(captured ? {selection: {address: captured.subject.address,
                    content_revision: captured.subject.contentRevision,
                    ...(captured.componentId ? {component_id: captured.componentId} : {})}} : {}),
            }});
            return this.current(request);
        } catch (error) {
            if (this.active() === request && this.current(request)) {
                this.finish(request, 'error', agentError(error));
            }
            return false;
        }
    }

    stop(): void {
        const request = this.active();
        if (!request) return;
        this.finish(request, 'cancelled');
        void invoke('editor_agent_session_stop', {request: {
            id: request.id, workspace_generation: request.native,
        }}).catch(() => undefined);
    }

    private connect(): Promise<boolean> {
        return this.ready ??= listen<StreamEvent>(STREAM_EVENT, ({payload}) => this.receive(payload)).then(release => {
            if (this.destroy.destroyed) {
                release();
                return false;
            }
            this.release = release;
            return true;
        }).catch(() => {
            this.ready = null;
            return false;
        });
    }

    private receive(event: StreamEvent): void {
        const request = this.active();
        if (!request || !event || event.id !== request.id || event.workspace_generation !== request.native ||
            !this.current(request)) return;
        switch (event.type) {
            case 'started':
                this.update(request.id, answer => ({...answer, status: 'streaming'}));
                break;
            case 'delta':
                if (typeof event.text === 'string') {
                    this.update(request.id, answer => ({...answer,
                        text: answer.text + event.text.slice(0, MAX_ANSWER - answer.text.length)}));
                }
                break;
            case 'sources':
                if (Array.isArray(event.citations)) {
                    this.update(request.id, answer => ({...answer, references: event.citations.slice(0, 8).filter(isReference)}));
                }
                break;
            case 'finished':
                if (['completed', 'retrieval_only', 'cancelled', 'error'].includes(event.status)) {
                    this.finish(request, event.status, typeof event.message === 'string' ? event.message : undefined);
                }
                break;
        }
    }

    private current(request: ActiveQuestion): boolean {
        return !this.destroy.destroyed && request.scope === this.editor.workspaceGeneration() &&
            request.native === this.viewport.state()?.workspace_generation;
    }

    private finish(request: ActiveQuestion, status: FinishStatus, message?: string): void {
        this.active.set(null);
        this.update(request.id, answer => ({...answer, status, message}));
    }

    private update(id: string, change: (answer: AgentAnswer) => AgentAnswer): void {
        this.history.update(history => history.map(answer => answer.id === id ? change(answer) : answer));
    }
}

function agentError(error: unknown): string {
    if (error instanceof Error) return error.message;
    if (typeof error === 'object' && error && 'message' in error && typeof error.message === 'string') {
        return error.message;
    }
    return 'The local Agent could not start.';
}

function isReference(hit: AgentReference): boolean {
    const citation = hit?.citation;
    return !!citation && (citation.corpus === 'engine-help' || citation.corpus === 'active-project') &&
        typeof citation.relative_path === 'string' && citation.relative_path.length <= 512 &&
        typeof citation.heading === 'string' && citation.heading.length <= 512 &&
        typeof hit.excerpt === 'string' && hit.excerpt.length <= 2_048;
}
