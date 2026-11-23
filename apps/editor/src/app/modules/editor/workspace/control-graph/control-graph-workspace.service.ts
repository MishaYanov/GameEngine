import {computed, effect, inject, Injectable, signal, untracked} from '@angular/core';
import {
    type ControlActionSample, type ControlGraph, type ControlGraphAsset, type ControlGraphCapture, type ControlGraphDocument,
    type ControlGraphProbe, controlIssues, controlPath, emptyControlGraph, readControlGraph,
} from '../../../../core/editor/controls-graph.models';
import type {EntityKey} from '../../../../core/editor/editor.models';
import {EditorService, type WorkspaceAssetScope} from '../../../../core/editor/editor.service';
import {ViewportEditorService} from '../../../../core/editor/viewport-editor.service';
import type {GraphCanvasSelection} from '../../../../shared/components/graph/graph';
import {WorkspaceViewService} from '../scene-viewport/workspace-view.service';
import {ScopedPaneRequests, type ScopedPaneAsset, type ScopedPaneTarget} from '../scoped-pane-request';
import {controlCanvas, type ControlEdit, editControlGraph} from './control-graph-draft';

export interface ControlGraphPaneTarget extends ScopedPaneTarget {
    readonly actorKey: EntityKey;
    readonly componentId: string;
    readonly asset: ScopedPaneAsset;
}

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
    private readonly panes = inject(ScopedPaneRequests, {optional: true});
    private readonly binding = signal<ControlGraphPaneTarget | null>(null);
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
    private readonly pendingState = signal(false);
    readonly pending = this.pendingState.asReadonly();
    private readonly loadingCatalogState = signal(false);
    readonly loadingCatalog = this.loadingCatalogState.asReadonly();
    private readonly probingState = signal(false);
    readonly probing = this.probingState.asReadonly();
    private readonly confirmingState = signal(false);
    readonly confirming = this.confirmingState.asReadonly();
    private readonly errorState = signal<string | null>(null);
    readonly error = this.errorState.asReadonly();
    readonly route = computed(() => {
        const sample = this.sample();
        return sample && sample.context === this.sampleContext() ? sample.result : null;
    });
    private readonly selectionState = signal<GraphCanvasSelection | null>(null);
    readonly selection = this.selectionState.asReadonly();
    private readonly revisionState = signal(0);
    readonly revision = this.revisionState.asReadonly();
    private readonly destinationState = signal('');
    readonly destination = this.destinationState.asReadonly();
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
        const target = this.binding();
        const selected = target?.actorKey ?? snapshot?.selection[0];
        const actor = snapshot?.document.entities.find(entity => entity.key === selected && entity.player_controller);
        if (!actor) return null;
        if (target && (target.subject.address.kind === 'prop' || target.subject.address.scene !== snapshot?.active_scene_id ||
            !this.current(target.subject.scope) || actor.player_controller?.id !== target.componentId ||
            actor.player_controller.settings.control_graph !== target.asset.id)) return null;
        return actor.key;
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
                this.loadingCatalogState.set(false);
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
        this.loadingCatalogState.set(true);
        try {
            const assets = await this.editor.listControlGraphs(scope);
            if (current()) this.catalog.set(assets);
        } catch (error) {
            if (current()) this.showError(error);
        } finally {
            if (current()) this.loadingCatalogState.set(false);
        }
    }

    async open(id: string): Promise<boolean> {
        const origin = this.scope();
        if (!origin || this.pending() || this.editor.playing()) return false;
        const scene = this.editor.snapshot()?.active_scene_id;
        const view = this.views.activeViewKind();
        const current = () => this.current(origin) && !this.editor.playing() &&
            this.editor.snapshot()?.active_scene_id === scene && this.views.activeViewKind() === view;
        if (this.asset()?.id === id) {
            this.binding.set(null);
            this.views.selectView('controls-graph');
            return true;
        }
        if (!await this.reviewNavigation() || this.pending() || !current()) return false;
        const scope = this.scope();
        if (!scope) return false;
        const request = ++this.serial;
        this.pendingState.set(true);
        this.errorState.set(null);
        try {
            const document = await this.editor.openControlGraph(id, scope);
            if (request !== this.serial || !current() || this.scope()?.revision !== scope.revision) return false;
            if (document.asset.id !== id) throw new Error('The editor opened a different Controls Graph asset.');
            this.adopt(document);
            this.binding.set(null);
            this.views.selectView('controls-graph');
            return true;
        } catch (error) {
            if (request === this.serial && current()) this.showError(error);
            return false;
        } finally {
            if (request === this.serial) this.pendingState.set(false);
        }
    }

    async openScoped(request: ControlGraphPaneTarget, resolve: () => ControlGraphPaneTarget | null): Promise<boolean> {
        if (!this.panes || !this.scope() || this.pending() || this.editor.playing()) return false;
        const origin = this.views.activeViewKind();
        const current = () => origin === this.views.activeViewKind() && !this.editor.playing();
        this.errorState.set(null);
        return this.panes.open<ControlGraphDocument>({...request, pane: 'controls-graph'}, {
            resolve: () => {
                const target = current() ? resolve() : null;
                return target?.actorKey === request.actorKey ? target : null;
            },
            load: async target => {
                const id = target.asset?.id;
                if (!id || this.pending()) return null;
                const request = ++this.serial;
                this.pendingState.set(true);
                try {
                    const document = await this.editor.openControlGraph(id, target.subject.scope);
                    if (request !== this.serial) return null;
                    return {value: document, asset: document.asset};
                } catch (error) {
                    if (request === this.serial && current()) this.showError(error);
                    return null;
                } finally {
                    if (request === this.serial) this.pendingState.set(false);
                }
            },
            activate: (document, target) => {
                if (!this.views.availableViews().includes('controls-graph')) return false;
                this.adopt(document);
                this.binding.set({...request, subject: target.subject});
                this.views.selectView('controls-graph');
                return true;
            },
        });
    }

    private adopt(document: ControlGraphDocument): void {
        const previous = this.asset();
        if (previous?.id === document.asset.id && previous.fingerprint === document.asset.fingerprint &&
            previous.revision === document.asset.revision && !this.dirty()) return;
        this.destinationState.set(document.asset.path);
        this.accept({asset: document.asset, graph: document.graph, baseline: document.graph, back: [], forward: []});
    }

    async create(): Promise<void> {
        if (!await this.reviewNavigation() || this.editor.playing()) return;
        const graph = emptyControlGraph();
        this.binding.set(null);
        this.destinationState.set('controls/player.controls.json');
        this.accept({asset: null, baseline: graph, graph, back: [], forward: []});
        this.errorState.set(null);
        this.views.selectView('controls-graph');
    }

    async save(): Promise<boolean> {
        const draft = this.draft();
        const scope = this.scope();
        if (!draft || !scope || !this.canSave()) return false;
        const request = ++this.serial;
        this.pendingState.set(true);
        this.errorState.set(null);
        try {
            const saved = await this.editor.saveControlGraph(draft.asset?.path ?? this.destination(),
                readControlGraph(draft.graph), pin(draft.asset), scope);
            if (request !== this.serial || !this.current(scope)) return false;
            this.destinationState.set(saved.asset.path);
            this.accept({...draft, asset: saved.asset, baseline: saved.graph, graph: saved.graph});
            this.catalogSerial++;
            this.loadingCatalogState.set(false);
            this.catalog.update(items => [...items.filter(item => item.id !== saved.asset.id), saved.asset]
                .sort((a, b) => a.path.localeCompare(b.path)));
            return true;
        } catch (error) {
            if (request === this.serial && this.current(scope)) this.showError(error);
            return false;
        } finally {
            if (request === this.serial) this.pendingState.set(false);
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
        this.errorState.set(null);
        this.probingState.set(true);
        try {
            const result = await this.editor.probeControlsGraph(readControlGraph(draft.graph), actions, pin(draft.asset), this.actorKey(), scope);
            if (current()) this.sample.set({context, result});
        } catch (error) {
            if (current()) this.showError(error);
        } finally {
            if (request === this.sampleSerial) this.probingState.set(false);
        }
    }

    invalidateSample(): void {
        this.sampleSerial++;
        this.sample.set(null);
        this.probingState.set(false);
    }

    edit(edit: ControlEdit): boolean {
        const draft = this.draft();
        if (!draft || this.pending() || this.confirming() || this.editor.playing()) return false;
        this.errorState.set(null);
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
        this.confirmingState.set(true);
        return new Promise(resolve => this.settle = resolve);
    }

    async settleNavigation(choice: 'save' | 'discard' | 'cancel'): Promise<void> {
        if (!this.confirming() || this.pending() || (choice === 'save' && !await this.save())) return;
        const draft = this.draft();
        if (choice === 'discard' && draft) this.accept({...draft, graph: draft.baseline, back: [], forward: []});
        this.finish(choice !== 'cancel');
    }

    select(selection: GraphCanvasSelection | null): void {
        this.selectionState.set(selection);
    }

    setDestination(path: string): void {
        if (!this.pending()) this.destinationState.set(path);
    }

    showError(error: unknown): void {
        this.errorState.set(error instanceof Error ? error.message : String(error));
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
        this.revisionState.update(value => value + 1);
        const selected = this.selection();
        if (!selected || !(selected.kind === 'node'
            ? draft.graph.nodes.some(node => node.id === selected.id)
            : controlCanvas(draft.graph).connections.some(edge => edge.id === selected.id))) {
            this.selectionState.set(draft.graph.nodes[0] ? {kind: 'node', id: draft.graph.nodes[0].id} : null);
        }
    }

    private clear(): void {
        this.serial++;
        this.pendingState.set(false);
        this.draft.set(null);
        this.binding.set(null);
        this.selectionState.set(null);
        this.errorState.set(null);
        this.invalidateSample();
        this.finish(false);
    }

    private finish(accepted: boolean): void {
        this.confirmingState.set(false);
        this.settle?.(accepted);
        this.settle = undefined;
    }
}

function pin(asset: ControlGraphAsset | null): ControlGraphCapture | null {
    return asset ? {id: asset.id, fingerprint: asset.fingerprint} : null;
}