import {computed, effect, inject, Injectable, signal, untracked} from '@angular/core';
import {
    type ControlActionSample, type ControlGraph, type ControlGraphAsset, type ControlGraphCapture,
    type ControlGraphProbe, controlIssues, controlPath, emptyControlGraph, readControlGraph,
} from '../../../../core/editor/controls-graph.models';
import {EditorService, type WorkspaceAssetScope} from '../../../../core/editor/editor.service';
import {ViewportEditorService} from '../../../../core/editor/viewport-editor.service';
import type {GraphCanvasSelection} from '../../../../shared/components/graph/graph';
import {WorkspaceViewService} from '../scene-viewport/workspace-view.service';
import {controlCanvas, type ControlEdit, editControlGraph} from './control-graph-draft';

interface Draft {
    readonly asset: ControlGraphAsset | null;
    readonly baseline: ControlGraph;
    readonly graph: ControlGraph;
    readonly back: readonly ControlGraph[];
    readonly forward: readonly ControlGraph[];
}
interface SampleResult {
    readonly context: string;
    readonly result: ControlGraphProbe;
}
const historyLimit = 32;

@Injectable()
export class ControlGraphWorkspaceService {
    private readonly editor = inject(EditorService);
    private readonly viewport = inject(ViewportEditorService);
    private readonly views = inject(WorkspaceViewService);
    private readonly catalog = signal<readonly ControlGraphAsset[]>([]);
    private readonly draft = signal<Draft | null>(null);
    private readonly sample = signal<SampleResult | null>(null);
    private serial = 0;
    private catalogSerial = 0;
    private sampleSerial = 0;
    private settle?: (accepted: boolean) => void;

    readonly supported = computed(() => this.editor.capabilities()?.controls_graph_assets === true);
    readonly assets = this.catalog.asReadonly();
    readonly asset = computed(() => this.draft()?.asset ?? null);
    readonly graph = computed(() => this.draft()?.graph ?? null);
    readonly pending = signal(false);
    readonly loadingCatalog = signal(false);
    readonly probing = signal(false);
    readonly confirming = signal(false);
    readonly error = signal<string | null>(null);
    readonly route = computed(() => {
        const sample = this.sample();
        return sample && sample.context === this.sampleContext() ? sample.result : null;
    });
    readonly selection = signal<GraphCanvasSelection | null>(null);
    readonly revision = signal(0);
    readonly destination = signal('');
    readonly issues = computed(() => this.graph() ? controlIssues(this.graph()!) : []);
    readonly dirty = computed(() => {
        const draft = this.draft();
        return !!draft && JSON.stringify(draft.graph) !== JSON.stringify(draft.baseline);
    });
    readonly canSave = computed(() => this.dirty() && !this.issues().length && !this.pending() &&
        this.editor.snapshot()?.mode === 'edit' && !!this.scope() && controlPath(this.destination()));
    readonly canUndo = computed(() => !!this.draft()?.back.length && !this.pending());
    readonly canRedo = computed(() => !!this.draft()?.forward.length && !this.pending());
    readonly actorKey = computed(() => {
        const snapshot = this.editor.snapshot();
        const selected = snapshot?.selection[0];
        return snapshot?.document.entities.find(entity => entity.key === selected && entity.player_controller)?.key ?? null;
    });

    constructor() {
        effect(() => {
            this.editor.workspaceGeneration();
            untracked(() => this.clear());
        });
        effect(() => {
            const context = this.scopeContext();
            untracked(() => {
                this.catalogSerial++;
                this.catalog.set([]);
                this.loadingCatalog.set(false);
                if (context) void this.refresh();
            });
        });
        effect(() => {
            this.sampleContext();
            untracked(() => this.invalidateSample());
        });
    }

    async refresh(): Promise<void> {
        const scope = this.scope();
        if (!scope || this.loadingCatalog()) return;
        const request = ++this.catalogSerial;
        const current = () => this.catalogSerial === request && this.current(scope);
        this.loadingCatalog.set(true);
        try {
            const assets = await this.editor.listControlGraphs(scope);
            if (current()) this.catalog.set(assets);
        } catch (error) {
            if (current()) this.showError(error);
        } finally {
            if (current()) this.loadingCatalog.set(false);
        }
    }

    async open(id: string): Promise<boolean> {
        const scope = this.scope();
        if (!scope || this.pending() || this.editor.playing()) return false;
        this.views.selectView('controls-graph');
        if (this.asset()?.id === id) return true;
        if (!await this.reviewNavigation() || this.pending() || !this.current(scope)) return false;
        const request = ++this.serial;
        this.pending.set(true);
        this.error.set(null);
        try {
            const document = await this.editor.openControlGraph(id, scope);
            if (request !== this.serial || !this.current(scope)) return false;
            if (document.asset.id !== id) throw new Error('The editor opened a different Controls Graph asset.');
            this.destination.set(document.asset.path);
            this.accept({asset: document.asset, graph: document.graph, baseline: document.graph, back: [], forward: []});
            return true;
        } catch (error) {
            if (request === this.serial && this.current(scope)) this.showError(error);
            return false;
        } finally {
            if (request === this.serial) this.pending.set(false);
        }
    }

    async create(): Promise<void> {
        if (!await this.reviewNavigation() || this.editor.playing()) return;
        const graph = emptyControlGraph();
        this.destination.set('controls/player.controls.json');
        this.accept({asset: null, baseline: graph, graph, back: [], forward: []});
        this.error.set(null);
        this.views.selectView('controls-graph');
    }

    async save(): Promise<boolean> {
        const draft = this.draft();
        const scope = this.scope();
        if (!draft || !scope || !this.canSave()) return false;
        const request = ++this.serial;
        this.pending.set(true);
        this.error.set(null);
        try {
            const saved = await this.editor.saveControlGraph(draft.asset?.path ?? this.destination(),
                readControlGraph(draft.graph), pin(draft.asset), scope);
            if (request !== this.serial || !this.current(scope)) return false;
            this.destination.set(saved.asset.path);
            this.accept({...draft, asset: saved.asset, baseline: saved.graph, graph: saved.graph});
            this.catalogSerial++;
            this.loadingCatalog.set(false);
            this.catalog.update(items => [...items.filter(item => item.id !== saved.asset.id), saved.asset]
                .sort((a, b) => a.path.localeCompare(b.path)));
            return true;
        } catch (error) {
            if (request === this.serial && this.current(scope)) this.showError(error);
            return false;
        } finally {
            if (request === this.serial) this.pending.set(false);
        }
    }

    async probe(actions: readonly ControlActionSample[]): Promise<void> {
        const draft = this.draft();
        const scope = this.scope();
        const context = this.sampleContext();
        if (!draft || !scope || !context || this.issues().length || this.probing()) return;
        const request = ++this.sampleSerial;
        const current = () => request === this.sampleSerial && context === this.sampleContext();
        this.sample.set(null);
        this.error.set(null);
        this.probing.set(true);
        try {
            const result = await this.editor.probeControlsGraph(readControlGraph(draft.graph), actions, pin(draft.asset), this.actorKey(), scope);
            if (current()) this.sample.set({context, result});
        } catch (error) {
            if (current()) this.showError(error);
        } finally {
            if (request === this.sampleSerial) this.probing.set(false);
        }
    }

    invalidateSample(): void {
        this.sampleSerial++;
        this.sample.set(null);
        this.probing.set(false);
    }

    edit(edit: ControlEdit): boolean {
        const draft = this.draft();
        if (!draft || this.pending() || this.confirming() || this.editor.playing()) return false;
        this.error.set(null);
        try {
            const graph = editControlGraph(draft.graph, edit, this.revision());
            if (graph === draft.graph || JSON.stringify(graph) === JSON.stringify(draft.graph)) return false;
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
            this.accept({...draft, graph, back: [...draft.back, draft.graph], forward: draft.forward.slice(0, -1)});
        }
    }

    async close(): Promise<void> {
        if (!await this.reviewNavigation()) return;
        this.clear();
        this.views.selectView('scene-viewport');
    }

    reviewNavigation(): Promise<boolean> {
        if (this.pending() || this.confirming()) return Promise.resolve(false);
        if (!this.dirty()) return Promise.resolve(true);
        this.confirming.set(true);
        return new Promise(resolve => this.settle = resolve);
    }

    async settleNavigation(choice: 'save' | 'discard' | 'cancel'): Promise<void> {
        if (!this.confirming() || this.pending() || (choice === 'save' && !await this.save())) return;
        const draft = this.draft();
        if (choice === 'discard' && draft) this.accept({...draft, graph: draft.baseline, back: [], forward: []});
        this.finish(choice !== 'cancel');
    }

    showError(error: unknown): void {
        this.error.set(error instanceof Error ? error.message : String(error));
    }

    scope(): WorkspaceAssetScope | null {
        const snapshot = this.editor.snapshot();
        const nativeGeneration = this.viewport.state()?.workspace_generation;
        return this.supported() && snapshot && nativeGeneration != null
            ? {workspaceGeneration: this.editor.workspaceGeneration(), nativeGeneration, revision: snapshot.revision} : null;
    }

    private readonly scopeContext = computed(() => {
        const scope = this.scope();
        return scope ? `${scope.workspaceGeneration}:${scope.nativeGeneration}` : null;
    });

    private readonly sampleContext = computed(() => {
        const context = this.scopeContext();
        if (!context || this.editor.playing() || !this.views.controlsGraphView()) return null;
        return JSON.stringify([context, this.editor.snapshot()?.revision, this.actorKey(), this.asset()?.id, this.revision()]);
    });

    private current(scope: WorkspaceAssetScope): boolean {
        return this.scopeContext() === `${scope.workspaceGeneration}:${scope.nativeGeneration}`;
    }

    private accept(draft: Draft): void {
        this.draft.set(draft);
        this.revision.update(value => value + 1);
        const selected = this.selection();
        if (!selected || !(selected.kind === 'node'
            ? draft.graph.nodes.some(node => node.id === selected.id)
            : controlCanvas(draft.graph).connections.some(edge => edge.id === selected.id))) {
            this.selection.set(draft.graph.nodes[0] ? {kind: 'node', id: draft.graph.nodes[0].id} : null);
        }
    }

    private clear(): void {
        this.serial++;
        this.pending.set(false);
        this.draft.set(null);
        this.selection.set(null);
        this.error.set(null);
        this.invalidateSample();
        this.finish(false);
    }

    private finish(accepted: boolean): void {
        this.confirming.set(false);
        this.settle?.(accepted);
        this.settle = undefined;
    }
}

function pin(asset: ControlGraphAsset | null): ControlGraphCapture | null {
    return asset ? {id: asset.id, fingerprint: asset.fingerprint} : null;
}