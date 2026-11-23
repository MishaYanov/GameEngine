import {computed, effect, inject, Injectable, signal, untracked} from '@angular/core';
import {type AnimatorDocument, type AnimatorGraph, readAnimatorGraph} from '../../../../core/editor/animator.models';
import {EditorService} from '../../../../core/editor/editor.service';
import type {GraphCanvasSelection} from '../../../../shared/components/graph/graph';
import {WorkspaceViewService} from '../scene-viewport/workspace-view.service';
import {AnimatorAssets} from './animator-assets.service';
import {type AnimatorEdit, animatorIssues, applyAnimatorEdit, runtimeDefinition} from './animator-draft';

interface Draft {
    readonly document: AnimatorDocument;
    readonly graph: AnimatorGraph;
    readonly back: readonly AnimatorGraph[];
    readonly forward: readonly AnimatorGraph[];
}

const historyLimit = 32;

@Injectable()
export class AnimatorWorkspaceService {
    private readonly editor = inject(EditorService);
    private readonly assets = inject(AnimatorAssets);
    private readonly views = inject(WorkspaceViewService);
    private readonly draft = signal<Draft | null>(null);
    private request = 0;
    private resolveNavigation?: (allowed: boolean) => void;
    readonly document = computed(() => this.draft()?.document ?? null);
    readonly graph = computed(() => this.draft()?.graph ?? null);
    readonly issues = computed(() => this.graph() ? animatorIssues(this.graph()!) : []);
    readonly dirty = computed(() => {
        const draft = this.draft();
        return !!draft && JSON.stringify(draft.graph) !== JSON.stringify(draft.document.graph);
    });
    private readonly revisionState = signal(0);
    readonly revision = this.revisionState.asReadonly();
    private readonly pendingState = signal(false);
    readonly pending = this.pendingState.asReadonly();
    private readonly confirmingState = signal(false);
    readonly confirming = this.confirmingState.asReadonly();
    private readonly selectionState = signal<GraphCanvasSelection | null>(null);
    readonly selection = this.selectionState.asReadonly();
    private readonly errorState = signal<string | null>(null);
    readonly error = this.errorState.asReadonly();
    private readonly runtimeDigestState = signal<string | null>(null);
    readonly runtimeDigest = this.runtimeDigestState.asReadonly();
    readonly canUndo = computed(() => !!this.draft()?.back.length && !this.pending());
    readonly canRedo = computed(() => !!this.draft()?.forward.length && !this.pending());
    readonly canSave = computed(() => this.dirty() && !this.issues().length && !this.pending());
    private readonly runtime = computed(() => this.graph() ? runtimeDefinition(this.graph()!) : null);

    constructor() {
        effect(() => {
            this.editor.workspaceGeneration();
            untracked(() => this.reset());
        });
        effect(() => {
            const runtime = this.runtime();
            this.runtimeDigestState.set(null);
            if (!runtime || !globalThis.crypto?.subtle) return;
            void globalThis.crypto.subtle.digest('SHA-256', new TextEncoder().encode(runtime)).then(bytes => {
                if (this.runtime() === runtime) {
                    this.runtimeDigestState.set(Array.from(new Uint8Array(bytes), byte => byte.toString(16).padStart(2, '0')).join(''));
                }
            }).catch(() => undefined);
        });
    }

    async open(assetId: string): Promise<boolean> {
        const origin = this.assets.scope();
        if (!origin || this.pending() || this.editor.playing()) return false;
        const scene = this.editor.snapshot()?.active_scene_id;
        const view = this.views.activeViewKind();
        const current = () => this.assets.current(origin) && !this.editor.playing() &&
            this.editor.snapshot()?.active_scene_id === scene && this.views.activeViewKind() === view;
        if (this.document()?.asset.id === assetId) {
            this.views.selectView('animation-controller');
            return true;
        }
        if (!await this.reviewNavigation() || this.pending() || !current()) return false;
        const scope = this.assets.scope();
        if (!scope) return false;
        const request = ++this.request;
        this.pendingState.set(true);
        this.errorState.set(null);
        try {
            const document = await this.editor.openAnimator(assetId, scope);
            if (!current() || request !== this.request || this.assets.scope()?.revision !== scope.revision) return false;
            if (document.asset.id !== assetId) throw new Error('The editor opened a different Animator asset.');
            this.draft.set({document, graph: document.graph, back: [], forward: []});
            this.selectionState.set({kind: 'node', id: document.graph.entry});
            this.revisionState.update(value => value + 1);
            this.views.selectView('animation-controller');
            return true;
        } catch (error) {
            if (request === this.request && current()) this.showError(error);
            return false;
        } finally {
            if (request === this.request) this.pendingState.set(false);
        }
    }

    async save(): Promise<boolean> {
        const draft = this.draft();
        const scope = this.assets.scope();
        if (!draft || !scope || !this.canSave()) return false;
        const request = ++this.request;
        const {id, path, fingerprint} = draft.document.asset;
        this.pendingState.set(true);
        this.errorState.set(null);
        try {
            const saved = await this.editor.saveAnimator(path, readAnimatorGraph(draft.graph), {id, fingerprint}, scope);
            if (!this.assets.current(scope) || request !== this.request) return false;
            this.draft.set({...draft, document: saved, graph: saved.graph});
            this.assets.accept(saved.asset, scope);
            return true;
        } catch (error) {
            if (request === this.request && this.assets.current(scope)) this.showError(error);
            return false;
        } finally {
            if (request === this.request) this.pendingState.set(false);
        }
    }

    edit(edit: AnimatorEdit): boolean {
        const draft = this.draft();
        if (!draft || this.pending() || this.confirming()) return false;
        this.errorState.set(null);
        try {
            const graph = applyAnimatorEdit(draft.graph, edit, this.revision());
            if (this.sameGraph(graph, draft.graph)) return false;
            this.accept({...draft, graph, back: [...draft.back, draft.graph].slice(-historyLimit), forward: []});
            return true;
        } catch (error) {
            this.showError(error);
            return false;
        }
    }

    undo(): void {
        const draft = this.draft();
        const graph = draft?.back.at(-1);
        if (draft && graph && this.canUndo()) {
            this.accept({...draft, graph, back: draft.back.slice(0, -1), forward: [...draft.forward, draft.graph]});
        }
    }

    redo(): void {
        const draft = this.draft();
        const graph = draft?.forward.at(-1);
        if (draft && graph && this.canRedo()) {
            this.accept({...draft, graph, forward: draft.forward.slice(0, -1), back: [...draft.back, draft.graph]});
        }
    }

    async close(): Promise<void> {
        if (!await this.reviewNavigation()) return;
        this.reset();
        this.views.selectView('scene-viewport');
    }

    reviewNavigation(): Promise<boolean> {
        if (this.pending() || this.confirming()) return Promise.resolve(false);
        if (!this.dirty()) return Promise.resolve(true);
        this.confirmingState.set(true);
        return new Promise(resolve => this.resolveNavigation = resolve);
    }

    async settleNavigation(choice: 'save' | 'discard' | 'cancel'): Promise<void> {
        if (!this.confirming() || this.pending() || (choice === 'save' && !await this.save())) return;
        if (choice === 'discard') {
            const document = this.document();
            if (document) this.accept({document, graph: document.graph, back: [], forward: []});
        }
        this.finishReview(choice !== 'cancel');
    }

    select(selection: GraphCanvasSelection | null): void {
        this.selectionState.set(selection);
    }

    clearError(): void {
        this.errorState.set(null);
    }

    showError(error: unknown): void {
        this.errorState.set(error instanceof Error ? error.message : String(error));
    }

    private accept(draft: Draft): void {
        this.draft.set(draft);
        this.revisionState.update(value => value + 1);
        const selected = this.selection();
        if (selected) {
            const candidates = selected.kind === 'node' ? draft.graph.states : draft.graph.transitions;
            if (!candidates.some(item => item.id === selected.id)) this.selectionState.set({kind: 'node', id: draft.graph.entry});
        }
    }

    private reset(): void {
        this.request++;
        this.pendingState.set(false);
        this.draft.set(null);
        this.selectionState.set(null);
        this.errorState.set(null);
        this.finishReview(false);
    }

    private finishReview(allowed: boolean): void {
        this.confirmingState.set(false);
        this.resolveNavigation?.(allowed);
        this.resolveNavigation = undefined;
    }

    private sameGraph(a: AnimatorGraph, b: AnimatorGraph): boolean {
        return a === b || JSON.stringify(a) === JSON.stringify(b);
    }
}