import {Component, computed, DestroyRef, effect, inject, input, output, signal, untracked} from '@angular/core';
import {
    type AnimatorAsset, type AnimatorDocument, type AnimatorGraph, MAX_ANIMATOR_BYTES, readAnimatorGraph,
    starterAnimator, validAnimatorPath,
} from '../../../../core/editor/animator.models';
import type {AnimationReview} from '../../../../core/editor/animation.models';
import type {ClipAssetIndex} from '../../../../core/editor/authored-clip.models';
import type {SceneEntity} from '../../../../core/editor/editor.models';
import type {WorkspaceAssetScope} from '../../../../core/editor/editor.service';
import {EditorDialog} from '../../../../shared/components/dialog/dialog';
import {SceneEditingService} from '../scene-editing.service';
import {AnimatorAssets} from './animator-assets.service';

@Component({
    selector: 'editor-animator-asset-editor',
    imports: [EditorDialog],
    templateUrl: './animator-asset-editor.html',
    styleUrls: ['../game-settings/settings.scss', './animator-inspector.scss'],
})
export class AnimatorAssetEditor {
    readonly actor = input.required<SceneEntity>();
    readonly editable = input(false);
    readonly saved = output<AnimatorDocument>();
    protected readonly library = inject(AnimatorAssets);
    private readonly editor = this.library.editor;
    private readonly editing = inject(SceneEditingService);
    protected readonly open = signal(false);
    protected readonly pending = signal(false);
    protected readonly error = signal<string | null>(null);
    protected readonly status = signal<string | null>(null);
    protected readonly destination = signal('actor.animator.json');
    protected readonly registeredId = signal('');
    protected readonly source = signal<AnimatorAsset | null>(null);
    protected readonly sourceKind = signal<'authored' | 'imported'>('authored');
    protected readonly selectedClip = signal('');
    protected readonly clips = signal<readonly ClipAssetIndex[]>([]);
    protected readonly rig = signal<AnimationReview | null>(null);
    protected readonly importedModel = signal('');
    protected readonly importedClip = signal(0);
    protected readonly importedModels = computed(() => (this.editor.modelAssets() ?? [])
        .filter(asset => asset.kind !== 'prefab' && !!asset.asset_id));
    protected readonly text = signal('');
    private readonly review = signal<{readonly text: string; readonly graph: AnimatorGraph} | null>(null);
    protected readonly graph = computed(() => {
        const result = this.review();
        return result?.text === this.text() ? result.graph : null;
    });
    protected readonly available = computed(() => this.editable() && !this.pending() && !!this.library.scope() &&
        this.editor.snapshot()?.mode === 'edit');
    protected readonly maxBytes = MAX_ANIMATOR_BYTES;
    private request = 0;

    constructor() {
        effect(() => {
            const mode = this.editor.snapshot()?.mode;
            const scope = this.library.scope();
            if (this.open() && (!scope || mode !== 'edit')) untracked(() => this.close());
        });
        inject(DestroyRef).onDestroy(() => this.request++);
    }

    protected begin(): void {
        if (!this.available()) return;
        this.error.set(null);
        this.status.set(null);
        const model = this.actor().asset_id;
        if (model && !this.importedModel()) this.chooseModel(model);
        this.open.set(true);
        void this.run(async (scope, current) => {
            const clips = await this.editor.listAuthoredClips(scope.workspaceGeneration);
            if (current()) this.clips.set(clips);
        });
        void this.editor.assets().catch(() => undefined);
    }

    protected close(): void {
        this.request++;
        this.pending.set(false);
        this.open.set(false);
    }

    protected newDefinition(): void {
        if (!this.available()) return;
        this.source.set(null);
        this.registeredId.set('');
        this.destination.set('actor.animator.json');
        this.text.set('');
        this.review.set(null);
        this.error.set(null);
        this.status.set(null);
    }

    protected async load(): Promise<void> {
        const assetId = this.registeredId();
        if (!assetId || !this.library.assets().some(asset => asset.id === assetId)) return;
        await this.run(async (scope, current) => {
            const document = await this.editor.openAnimator(assetId, scope);
            if (!current()) return;
            if (document.asset.id !== assetId) throw new Error('The editor opened a different Animator asset.');
            this.adopt(document);
        });
    }

    protected editText(text: string): void {
        this.text.set(text);
        this.error.set(null);
        this.status.set(null);
    }

    protected reviewText(): void {
        if (!this.available()) return;
        try {
            if (new TextEncoder().encode(this.text()).byteLength > MAX_ANIMATOR_BYTES) {
                throw new Error('Animator definitions must fit within 256 KiB.');
            }
            const graph = readAnimatorGraph(JSON.parse(this.text()));
            this.review.set({text: this.text(), graph});
            this.error.set(null);
            this.status.set('Animator definition is valid.');
        } catch (error) {
            this.review.set(null);
            this.status.set(null);
            this.error.set(error instanceof Error ? error.message : String(error));
        }
    }

    protected async importFile(file: File | undefined): Promise<void> {
        if (!file) return;
        await this.run(async (_, current) => {
            if (file.size > MAX_ANIMATOR_BYTES) throw new Error('Animator definitions must fit within 256 KiB.');
            const text = await file.text();
            if (!current()) return;
            this.source.set(null);
            this.registeredId.set('');
            this.destination.set(file.name.toLowerCase().endsWith('.animator.json') ? file.name : 'actor.animator.json');
            this.editText(text);
        });
    }

    protected chooseModel(id: string): void {
        this.rig.set(null);
        this.importedModel.set(id);
        this.importedClip.set(0);
    }

    protected async reviewRig(): Promise<void> {
        const assetId = this.importedModel();
        if (!assetId) return;
        await this.run(async (scope, current) => {
            const review = await this.editor.inspectAnimation(assetId, scope.workspaceGeneration, scope.revision);
            if (!current() || this.importedModel() !== assetId) return;
            if (review.workspace_generation !== scope.nativeGeneration || review.metadata.source.asset_id !== assetId) {
                throw new Error('The editor reviewed a different animation rig.');
            }
            this.rig.set(review);
            this.importedClip.set(review.metadata.clips[0].index);
        });
    }

    protected makeStarter(): void {
        if (!this.available()) return;
        const clip = this.clips().find(asset => asset.asset_id === this.selectedClip());
        const rig = this.rig()?.metadata;
        if (this.sourceKind() === 'authored' && clip) {
            this.setGraph(starterAnimator({kind: 'authored', asset_id: clip.asset_id, expected_sha256: clip.fingerprint}));
        } else if (this.sourceKind() === 'imported' && rig?.clips.some(item => item.index === this.importedClip())) {
            this.setGraph(starterAnimator({kind: 'imported', source: rig.source, clip_index: this.importedClip()}));
        }
    }

    protected async save(): Promise<void> {
        const graph = this.graph();
        const path = this.source()?.path ?? this.destination();
        const source = this.source();
        if (!graph || !validAnimatorPath(path) || !this.available()) return;
        await this.run(async (_, current) => {
            const accepted = await this.editing.withCommittedEdit(async () => {
                const scope = this.library.scope();
                if (!scope || !current()) return;
                const capture = source ? {id: source.id, fingerprint: source.fingerprint} : null;
                const saved = await this.editor.saveAnimator(path, graph, capture, scope);
                if (!current()) return;
                if (saved.asset.path !== path || source && saved.asset.id !== source.id) {
                    throw new Error('The editor returned a different Animator asset.');
                }
                this.library.accept(saved.asset, scope);
                this.adopt(saved);
                this.status.set('Animator asset saved.');
                this.saved.emit(saved);
            });
            if (current() && !accepted) this.error.set(this.editing.error() ?? 'Finish the current edit before saving the Animator.');
        });
    }

    protected validPath(path: string): boolean { return validAnimatorPath(path); }

    private adopt(document: AnimatorDocument): void {
        this.source.set(document.asset);
        this.destination.set(document.asset.path);
        this.registeredId.set(document.asset.id);
        this.setGraph(document.graph);
    }

    private setGraph(graph: AnimatorGraph): void {
        const text = JSON.stringify(graph, null, 2);
        this.editText(text);
        this.review.set({text, graph});
    }

    private async run(operation: (scope: WorkspaceAssetScope, current: () => boolean) => Promise<void>): Promise<void> {
        const scope = this.library.scope();
        if (!scope || !this.open() || !this.available()) return;
        const request = ++this.request;
        const current = () => request === this.request && this.open() && this.library.current(scope);
        this.pending.set(true);
        this.error.set(null);
        this.status.set(null);
        try {
            await operation(scope, current);
        } catch (error) {
            if (current()) this.error.set(error instanceof Error ? error.message : String(error));
        } finally {
            if (current()) this.pending.set(false);
        }
    }
}
