import {
    afterNextRender,
    Component,
    computed,
    effect,
    ElementRef,
    inject,
    Injector,
    input,
    signal,
    viewChild
} from '@angular/core';
import {invoke} from '@tauri-apps/api/core';
import {CommandBuilder, type MaterialOverride, type SceneEntity} from '../../../../core/editor/editor.models';
import {EditorService} from '../../../../core/editor/editor.service';
import {EditorDialog} from '../../../../shared/components/dialog/dialog';
import {EditorGraph} from '../../../../shared/components/graph/graph';
import {createGraphNode, emptyGraph, type GraphDocument, type GraphNodeKind} from '../../../../shared/components/graph/graph.models';
import type {InspectorSubject} from '../inspector-panel/inspector-subject';
import {SceneEditingService} from '../scene-editing.service';
import {WorkspaceDraftReview} from '../workspace-draft-review';

type MaterialGraphTarget = {
    readonly generation: number;
    readonly scene: string;
    readonly sceneRevision: number;
    readonly key: string;
    readonly name: string;
    readonly material: MaterialOverride | null;
    readonly initial: GraphDocument;
};

const NODE_KINDS: readonly GraphNodeKind[] = ['shader.color', 'value', 'shader.multiply', 'shader.output'];

@Component({
    selector: 'editor-material-graph',
    imports: [EditorDialog, EditorGraph],
    templateUrl: './material-graph-dialog.html',
    styles: [
        'editor-graph {display: block; height: min(29rem, 58dvh)}',
        '.discard-review {padding: var(--space-3); border: var(--border-width) solid var(--color-pane-edge); border-radius: var(--radius-sm)}',
        '.discard-review p {margin: 0 0 var(--space-3)}',
        '.discard-actions {display: flex; flex-wrap: wrap; gap: var(--space-2)}',
    ],
})
export class MaterialGraphDialog {
    readonly entity = input.required<SceneEntity | null>();
    readonly subject = input.required<InspectorSubject | null>();
    readonly editable = input.required<boolean>();
    private readonly editor = inject(EditorService);
    private readonly editing = inject(SceneEditingService);
    private readonly drafts = inject(WorkspaceDraftReview, {optional: true});
    private readonly injector = inject(Injector);
    private readonly keepEditingButton = viewChild<ElementRef<HTMLButtonElement>>('keepEditingButton');
    private readonly graphWorkspace = viewChild<ElementRef<HTMLElement>>('graphWorkspace');
    protected readonly target = signal<MaterialGraphTarget | null>(null);
    protected readonly graph = signal<GraphDocument>(emptyGraph());
    protected readonly opening = signal(false);
    protected readonly pending = signal(false);
    protected readonly error = signal<string | null>(null);
    protected readonly confirmDiscard = signal(false);
    protected readonly nodeKinds = NODE_KINDS;
    protected readonly dialogConfig = {width: '60rem'};
    protected readonly canShow = computed(() => {
        const entity = this.entity();
        return this.editor.nativeAvailable && !!entity && entity.model_path !== '__no_mesh__' &&
            this.subject()?.access === 'scene-edit';
    });
    protected readonly dirty = computed(() => {
        const initial = this.target()?.initial;
        return !!initial && JSON.stringify(this.graph()) !== JSON.stringify(initial);
    });

    constructor() {
        effect(() => {
            const target = this.target();
            if (target && !this.currentScope(target)) this.close();
        });
    }

    protected async show(): Promise<void> {
        if (!this.canShow() || !this.editable() || this.opening()) return;
        const key = this.entity()?.key;
        const reviewedScope = this.drafts?.capture();
        const capture = () => {
            const entity = this.entity();
            const snapshot = this.editor.snapshot();
            if (!this.canShow() || !snapshot || !entity || entity.key !== key ||
                snapshot.mode !== 'edit' || snapshot.selection.length !== 1 ||
                snapshot.selection[0] !== key) return;
            const material = entity.material ?? null;
            const initial = material?.source_graph ?? starterGraph(material);
            this.graph.set(initial);
            this.target.set({
                generation: this.editor.workspaceGeneration(),
                scene: snapshot.active_scene_id, sceneRevision: snapshot.scene_revision,
                key: entity.key, name: entity.name, material, initial,
            });
        };
        this.opening.set(true);
        this.error.set(null);
        try {
            if (this.drafts && reviewedScope) {
                const approved = await this.drafts.run(reviewedScope, async () => capture());
                if (!approved && this.drafts.current(reviewedScope) && this.entity()?.key === key) {
                    this.error.set('Finish or discard the current edit before opening Material Graph.');
                }
            } else capture();
        } catch (error) {
            this.error.set(diagnostic(error));
        } finally {
            this.opening.set(false);
        }
    }

    protected update(graph: GraphDocument): void {
        this.graph.set(graph);
        this.error.set(null);
    }

    protected reviewClose(): void {
        if (this.pending() || !this.dirty()) return;
        this.confirmDiscard.set(true);
        afterNextRender(() => this.keepEditingButton()?.nativeElement.focus(), {injector: this.injector});
    }

    protected keepEditing(): void {
        this.confirmDiscard.set(false);
        afterNextRender(() => this.graphWorkspace()?.nativeElement.focus(), {injector: this.injector});
    }

    protected close(): void {
        this.target.set(null);
        this.confirmDiscard.set(false);
        this.error.set(null);
    }

    protected async apply(detach = false): Promise<void> {
        const target = this.target();
        if (!target || this.pending() || this.confirmDiscard()) return;
        this.pending.set(true);
        this.error.set(null);
        try {
            const material = detach && target.material
                ? {base_color_factor: target.material.base_color_factor}
                : await invoke<MaterialOverride>('editor_compile_material_graph', {graph: this.graph()});
            if (this.target() !== target) return;
            const accepted = await this.editing.withCommittedEdit(async () => {
                const snapshot = this.editor.snapshot();
                if (this.target() !== target || !this.currentScope(target) || snapshot?.scene_revision !== target.sceneRevision) {
                    throw new Error('The scene changed. Reopen Material Graph to edit its current material.');
                }
                const [r, g, b] = material.base_color_factor;
                await this.editor.execute(CommandBuilder.material(target.key, {
                    ...material,
                    base_color_factor: [r, g, b, target.material?.base_color_factor[3] ?? 1],
                }), target.generation, snapshot.revision);
            });
            if (this.target() !== target) return;
            if (accepted) this.close();
            else this.error.set(this.editing.error() ?? 'Finish the current edit before applying this material.');
        } catch (error) {
            if (this.target() === target) this.error.set(diagnostic(error));
        } finally {
            this.pending.set(false);
        }
    }

    private currentScope(target: MaterialGraphTarget): boolean {
        const snapshot = this.editor.snapshot();
        return this.editor.workspaceGeneration() === target.generation &&
            snapshot?.mode === 'edit' && snapshot.active_scene_id === target.scene &&
            snapshot.selection.length === 1 && snapshot.selection[0] === target.key;
    }
}

function starterGraph(material: MaterialOverride | null): GraphDocument {
    const [red, green, blue] = material?.base_color_factor ?? [1, 1, 1, 1];
    return {
        ...emptyGraph('shader'),
        nodes: [
            {...createGraphNode('shader.color', 'color', {x: 24, y: 30}), values: {red, green, blue}},
            createGraphNode('shader.output', 'material', {x: 320, y: 30}),
        ],
        connections: [
            {id: 'base-color', from: {node: 'color', port: 'color'}, to: {node: 'material', port: 'color'}},
        ],
    };
}

function diagnostic(error: unknown): string {
    if (error instanceof Error) return error.message;
    if (typeof error === 'string') return error;
    if (error && typeof error === 'object' && 'message' in error && typeof error.message === 'string') {
        const node = 'node' in error && typeof error.node === 'string' ? error.node : null;
        const port = 'port' in error && typeof error.port === 'string' ? error.port : null;
        return node ? [node, port, error.message].filter(Boolean).join(' · ') : error.message;
    }
    return 'Material Graph could not be compiled.';
}
