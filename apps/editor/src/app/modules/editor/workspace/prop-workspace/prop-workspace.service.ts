import {
    afterNextRender, computed, DestroyRef, DOCUMENT, effect, inject, Injectable, Injector, signal, untracked,
} from '@angular/core';
import {listen} from '@tauri-apps/api/event';
import type {ComponentRecord} from '../../../../core/editor/editor-components';
import type {EntityKey, ModelAsset, SceneEntity, SceneId, Transform} from '../../../../core/editor/editor.models';
import {EditorService} from '../../../../core/editor/editor.service';
import type {
    NestedPropPlacement, NestedPropPlacementRequest, NestedPropSource, PrefabSourceDraft, PropDraftEdit,
} from '../../../../core/editor/linked-prefabs.models';
import {readPrefabSource} from '../prefabs/prefab-source-values';
import {SceneEditingService} from '../scene-editing.service';
import {WorkspaceViewService} from '../scene-viewport/workspace-view.service';
import type {PropObjectEdit} from './prop-object-editor';

export interface PropWorkspaceObject {
    readonly key: EntityKey;
    readonly name: string;
    readonly parent: EntityKey | null;
    readonly transform: Transform;
    readonly components: readonly ComponentRecord[];
    readonly level: number;
}

interface PropScope {
    readonly generation: number;
    readonly sceneId: SceneId;
}

type PropTab =
    | {readonly kind: 'source'; readonly scope: PropScope; readonly name: string; readonly draft: PrefabSourceDraft}
    | {readonly kind: 'instance'; readonly scope: PropScope; readonly root: EntityKey};

export type PropWorkspaceDocument = PropTab & {
    readonly name: string;
    readonly path: string;
    readonly objects: readonly PropWorkspaceObject[];
    readonly placements: readonly NestedPropPlacement[];
    readonly references: readonly {readonly slot: string; readonly target: string}[];
};

interface PropObjectRecord {
    readonly key: EntityKey;
    readonly name: string;
    readonly parent: EntityKey | null;
    readonly transform: Transform;
    readonly components: readonly ComponentRecord[];
}

/** Owns one isolated Prop document, its native draft, and local object selection. */
@Injectable()
export class PropWorkspaceService {
    private readonly editor = inject(EditorService);
    private readonly editing = inject(SceneEditingService);
    private readonly views = inject(WorkspaceViewService);
    private readonly injector = inject(Injector);
    private readonly dom = inject(DOCUMENT);
    private readonly tab = signal<PropTab | null>(null);
    private readonly selectedKey = signal<EntityKey | null>(null);
    private readonly placementId = signal<string | null>(null);
    private readonly opening = signal<PropScope | null>(null);
    private readonly failure = signal<string | null>(null);
    private readonly editingDraft = signal(false);
    private readonly reviewingClose = signal(false);
    private readonly nestedSource = signal<NestedPropSource | null>(null);
    private readonly reviewingNestedSource = signal(false);
    private nestedReviewRequest = 0;
    private closeReview: {readonly promise: Promise<boolean>; readonly resolve: (approved: boolean) => void} | null = null;
    private sourceDraft: PrefabSourceDraft | null = null;
    private focusedBeforeOpen: HTMLElement | null = null;
    private request = 0;
    private destroyed = false;

    readonly error = this.failure.asReadonly();
    readonly pending = computed(() => this.opening() !== null);
    readonly draftPending = this.editingDraft.asReadonly();
    readonly nestedReview = this.nestedSource.asReadonly();
    readonly nestedReviewPending = this.reviewingNestedSource.asReadonly();
    readonly assets = computed(() => this.editor.modelAssets() ?? []);
    readonly confirmingClose = this.reviewingClose.asReadonly();
    readonly canOpen = computed(() => this.editing.ready() && !this.pending() && !this.draftPending() && !!this.editor.snapshot());
    readonly draft = computed(() => {
        const tab = this.tab();
        return tab?.kind === 'source' ? tab.draft : null;
    });
    readonly canEditDraft = computed(() => {
        const snapshot = this.editor.snapshot();
        return this.draft()?.draft_revision !== undefined && !this.draftPending() && !this.editor.busy() &&
            snapshot?.mode === 'edit' && snapshot.transaction === null;
    });
    readonly document = computed<PropWorkspaceDocument | null>(() => {
        const tab = this.tab();
        if (!tab) return null;
        if (tab.kind === 'source') {
            const prefab = tab.draft.prefab;
            const objects = flattenObjects(prefab.entities.map(entity => ({...entity, parent: entity.parent ?? null})));
            return {...tab, path: tab.draft.path, objects, placements: prefab.composition?.placements ?? [],
                references: Object.keys(prefab.reference_slots).map(slot => ({slot, target: 'Assigned on placement'}))};
        }
        const entities = this.editor.document()?.entities;
        if (!entities) return null;
        const root = entities.find(entity => entity.key === tab.root);
        if (!root?.linked_prefab) return null;
        const objects = flattenObjects(entities.map(instanceObject), root.key);
        const names = new Map(entities.map(entity => [entity.key, entity.name]));
        const references = Object.entries(root.linked_prefab.bindings)
            .map(([slot, key]) => ({slot, target: names.get(key) ?? 'Missing object'}));
        return {...tab, name: root.name, path: root.linked_prefab.source.path, objects, references, placements: []};
    });
    readonly selectedPlacement = computed<NestedPropPlacement | null>(() => {
        const id = this.placementId();
        return this.document()?.placements.find(placement => placement.id === id) ?? null;
    });
    readonly selectedObject = computed<PropWorkspaceObject | null>(() => {
        if (this.selectedPlacement()) return null;
        const objects = this.document()?.objects ?? [];
        return objects.find(item => item.key === this.selectedKey()) ?? objects[0] ?? null;
    });

    constructor() {
        effect(() => {
            const tab = this.tab();
            const scope = this.scope();
            const document = this.document();
            const opening = this.opening();
            const playing = this.editor.playing();
            if (tab && (!scope || tab.scope.generation !== scope.generation ||
                (tab.kind === 'instance' && tab.scope.sceneId !== scope.sceneId) || !document)) untracked(() => this.close(false));
            if (opening && (playing || !this.sameScope(opening, scope))) untracked(() => this.cancelOpen());
        });
        let unsubscribe: (() => void) | undefined;
        if (this.editor.nativeAvailable) {
            void listen<PrefabSourceDraft>('editor://prop-draft-changed', event => this.acceptDraft(event.payload, true))
                .then(release => {
                    if (this.destroyed) release();
                    else unsubscribe = release;
                }).catch(() => undefined);
        }
        inject(DestroyRef).onDestroy(() => {
            this.destroyed = true;
            this.request++;
            unsubscribe?.();
            this.finishClose(false);
            this.releaseSource();
        });
    }

    async openSource(asset: Pick<ModelAsset, 'asset_id' | 'path' | 'name'>): Promise<boolean> {
        return this.open(async (scope, revision, current) => {
            const draft = await this.editor.openPrefabSource(asset.path, scope.generation, revision);
            if (!current()) {
                this.closeDraft(draft.draft_id);
                return null;
            }
            try {
                if (asset.asset_id && draft.asset_id !== asset.asset_id) {
                    throw new Error('The Prop asset has changed. Refresh Assets and open it again.');
                }
                readPrefabSource(draft.prefab);
            } catch (error) {
                this.closeDraft(draft.draft_id);
                throw error;
            }
            return {kind: 'source', scope, name: asset.name, draft};
        });
    }

    async openInstance(root: EntityKey): Promise<boolean> {
        return this.open(async scope => {
            const entity = this.editor.document()?.entities.find(candidate => candidate.key === root);
            if (!entity?.linked_prefab) throw new Error('This object no longer has a linked Prop source.');
            return {kind: 'instance', scope, root};
        });
    }

    selectObject(key: EntityKey): void {
        if (this.document()?.objects.some(item => item.key === key)) {
            this.placementId.set(null);
            this.selectedKey.set(key);
        }
    }

    selectPlacement(id: string): void {
        if (this.document()?.placements.some(placement => placement.id === id)) this.placementId.set(id);
    }

    async reviewNested(assetId: string, fingerprint: string): Promise<void> {
        const draft = this.draft();
        if (!draft || !this.canEditDraft() || !this.assets().some(asset =>
            asset.kind === 'prefab' && asset.asset_id === assetId && asset.fingerprint === fingerprint) ||
            assetId === draft.asset_id) return;
        const request = ++this.nestedReviewRequest;
        const generation = this.editor.workspaceGeneration();
        const current = () => request === this.nestedReviewRequest && !this.destroyed &&
            this.editor.workspaceGeneration() === generation && this.draft()?.draft_id === draft.draft_id;
        this.failure.set(null);
        this.nestedSource.set(null);
        this.reviewingNestedSource.set(true);
        try {
            const review = await this.editor.reviewNestedProp(assetId, fingerprint, draft.workspace_generation, generation);
            if (!current()) return;
            if (review.asset_id !== assetId || review.fingerprint !== fingerprint || !Array.isArray(review.slots) ||
                review.slots.length > 128 || new Set(review.slots).size !== review.slots.length ||
                review.slots.some(slot => typeof slot !== 'string' || !/^[A-Za-z0-9_-]{1,64}$/.test(slot))) {
                throw new Error('The editor returned a different or invalid nested Prop source.');
            }
            this.nestedSource.set(review);
        } catch (error) {
            if (current()) this.failure.set(error instanceof Error ? error.message : String(error));
        } finally {
            if (current()) this.reviewingNestedSource.set(false);
        }
    }

    clearNestedReview(): void {
        this.nestedReviewRequest++;
        this.nestedSource.set(null);
        this.reviewingNestedSource.set(false);
    }

    async addNested(placement: NestedPropPlacementRequest): Promise<boolean> {
        const review = this.nestedReview();
        const document = this.document();
        if (!review || review.asset_id !== placement.asset_id || review.fingerprint !== placement.fingerprint ||
            !document?.objects.some(object => object.key === placement.parent)) return false;
        const previousIds = new Set(document.placements.map(existing => existing.id));
        const accepted = await this.runDraft((draft, revision, generation) =>
            this.editor.addNestedProp(draft.draft_id, revision, placement, generation));
        if (accepted) {
            const added = this.document()?.placements.find(item => !previousIds.has(item.id));
            if (added) this.selectPlacement(added.id);
            this.clearNestedReview();
        }
        return accepted;
    }

    editPlacement(change: PropObjectEdit): Promise<boolean> {
        const selected = this.selectedPlacement();
        const composition = this.draft()?.prefab.composition;
        if (!selected || change.key !== `placement:${selected.id}` || !composition) return Promise.resolve(false);
        const patch = 'name' in change ? {name: change.name} :
            'parent' in change ? {parent: change.parent ?? selected.parent} : {local: change.transform};
        return this.replacePlacements(composition.placements.map(placement =>
            placement.id === selected.id ? {...placement, ...patch} : placement));
    }

    async reorderPlacement(offset: -1 | 1): Promise<boolean> {
        const selected = this.selectedPlacement();
        const placements = this.draft()?.prefab.composition?.placements;
        if (!selected || !placements) return false;
        const siblings = placements.filter(placement => placement.parent === selected.parent);
        const neighbor = siblings[siblings.findIndex(placement => placement.id === selected.id) + offset];
        if (!neighbor) return false;
        return this.replacePlacements(placements.map(placement => placement.id === selected.id ? neighbor :
            placement.id === neighbor.id ? selected : placement));
    }

    editObject(change: PropObjectEdit): Promise<boolean> {
        const draft = this.draft();
        if (!draft || !draft.prefab.entities.some(entity => entity.key === change.key)) return Promise.resolve(false);
        return this.editDraft({type: 'apply', prefab: {
            ...draft.prefab,
            entities: draft.prefab.entities.map(entity => entity.key === change.key ? {...entity, ...change} : entity),
        }});
    }

    editDraft(edit: PropDraftEdit): Promise<boolean> {
        return this.runDraft((draft, revision, generation) => this.editor.editPropDraft(draft.draft_id, revision, edit, generation));
    }

    saveDraft(): Promise<boolean> {
        return this.runDraft((draft, revision, generation) => this.editor.savePropDraft(draft.draft_id, revision, generation));
    }

    reviewClose(): Promise<boolean> {
        if (!this.draft()?.dirty) return Promise.resolve(!this.draftPending());
        if (this.closeReview) return this.closeReview.promise;
        let resolve!: (approved: boolean) => void;
        const promise = new Promise<boolean>(done => (resolve = done));
        this.closeReview = {promise, resolve};
        this.failure.set(null);
        this.reviewingClose.set(true);
        return promise;
    }

    async chooseClose(action: 'save' | 'discard'): Promise<void> {
        if (!this.closeReview) return;
        const accepted = action === 'save' ? await this.saveDraft() : await this.editDraft({type: 'discard'});
        if (accepted && !this.draft()?.dirty) this.finishClose(true);
    }

    cancelClose(): void {
        if (!this.draftPending()) this.finishClose(false);
    }

    cancelOpening(): void {
        if (this.pending()) this.cancelOpen();
    }

    close(restoreFocus = true, fallbackFocus?: () => void): void {
        this.finishClose(false);
        this.cancelOpen();
        this.tab.set(null);
        this.selectedKey.set(null);
        this.placementId.set(null);
        this.clearNestedReview();
        this.releaseSource();
        const origin = this.focusedBeforeOpen;
        this.focusedBeforeOpen = null;
        this.views.closePropView();
        if (restoreFocus) afterNextRender(() => {
            if (origin?.isConnected && !origin.matches(':disabled')) origin.focus({preventScroll: true});
            else fallbackFocus?.();
        }, {injector: this.injector});
    }

    private async open(load: (
        scope: PropScope, revision: number, current: () => boolean,
    ) => Promise<PropTab | null>): Promise<boolean> {
        const scope = this.scope();
        if (!scope || !this.canOpen() || !await this.reviewClose() ||
            !this.sameScope(scope, this.scope()) || !this.canOpen()) return false;
        const focused = this.dom.activeElement;
        const request = ++this.request;
        const current = () => !this.destroyed && this.request === request &&
            this.sameScope(scope, this.scope()) && this.editor.snapshot()?.mode === 'edit';
        this.failure.set(null);
        this.opening.set(scope);
        let opened = false;
        try {
            const accepted = await this.editing.withCommittedEdit(async () => {
                const snapshot = this.editor.snapshot();
                if (!snapshot || !current()) return;
                const tab = await load(scope, snapshot.revision, current);
                if (!tab) return;
                if (!current()) {
                    if (tab.kind === 'source') this.closeDraft(tab.draft.draft_id);
                    return;
                }
                this.releaseSource();
                this.sourceDraft = tab.kind === 'source' ? tab.draft : null;
                this.focusedBeforeOpen = focused instanceof HTMLElement ? focused : null;
                this.selectedKey.set(null);
                this.placementId.set(null);
                this.clearNestedReview();
                this.tab.set(tab);
                this.views.selectView('prop-workspace');
                opened = true;
            });
            if (!accepted && current()) this.failure.set(this.editing.error() ?? 'Finish the current edit before opening the Prop.');
            return opened;
        } finally {
            if (request === this.request) this.opening.set(null);
        }
    }

    private scope(): PropScope | null {
        const sceneId = this.editor.snapshot()?.active_scene_id;
        return sceneId ? {generation: this.editor.workspaceGeneration(), sceneId} : null;
    }

    private replacePlacements(placements: readonly NestedPropPlacement[]): Promise<boolean> {
        const prefab = this.draft()?.prefab;
        if (!prefab?.composition) return Promise.resolve(false);
        return this.editDraft({type: 'apply', prefab: {...prefab, composition: {...prefab.composition, placements}}});
    }

    private async runDraft(run: (
        draft: PrefabSourceDraft, revision: number, generation: number,
    ) => Promise<PrefabSourceDraft>): Promise<boolean> {
        const draft = this.draft();
        if (!this.canEditDraft() || !draft || draft.draft_revision === undefined) return false;
        const generation = this.editor.workspaceGeneration();
        this.failure.set(null);
        this.editingDraft.set(true);
        try {
            const accepted = await run(draft, draft.draft_revision, generation);
            return this.acceptDraft(accepted, false);
        } catch (error) {
            if (!this.destroyed && this.draft()?.draft_id === draft.draft_id &&
                this.editor.workspaceGeneration() === generation) {
                this.failure.set(error instanceof Error ? error.message : String(error));
            }
            return false;
        } finally {
            this.editingDraft.set(false);
        }
    }

    private acceptDraft(draft: PrefabSourceDraft, newerOnly: boolean): boolean {
        const tab = this.tab();
        if (this.destroyed || tab?.kind !== 'source' || tab.scope.generation !== this.editor.workspaceGeneration() ||
            draft.draft_id !== tab.draft.draft_id || draft.workspace_generation !== tab.draft.workspace_generation ||
            draft.asset_id !== tab.draft.asset_id || draft.path !== tab.draft.path || draft.draft_revision === undefined ||
            tab.draft.draft_revision === undefined || draft.draft_revision < tab.draft.draft_revision ||
            (newerOnly && draft.draft_revision === tab.draft.draft_revision)) return false;
        try {
            readPrefabSource(draft.prefab);
        } catch (error) {
            this.failure.set(error instanceof Error ? error.message : String(error));
            return false;
        }
        this.sourceDraft = draft;
        this.tab.set({...tab, draft});
        return true;
    }

    private finishClose(approved: boolean): void {
        const review = this.closeReview;
        this.closeReview = null;
        this.reviewingClose.set(false);
        review?.resolve(approved);
    }

    private sameScope(a: PropScope, b: PropScope | null): boolean {
        return b !== null && a.generation === b.generation && a.sceneId === b.sceneId;
    }

    private cancelOpen(): void {
        this.request++;
        this.opening.set(null);
    }

    private releaseSource(): void {
        if (this.sourceDraft) this.closeDraft(this.sourceDraft.draft_id);
        this.sourceDraft = null;
    }

    private closeDraft(draftId: string): void {
        void this.editor.closePrefabSource(draftId).catch(() => undefined);
    }
}

function instanceObject(entity: SceneEntity): PropObjectRecord {
    return {...entity, parent: entity.parent ?? null, components: entity.component_views ?? []};
}

function flattenObjects(objects: readonly PropObjectRecord[], root?: EntityKey): readonly PropWorkspaceObject[] {
    const keys = new Set(objects.map(object => object.key));
    const children = new Map<EntityKey | null, PropObjectRecord[]>();
    for (const object of objects) {
        const parent = object.parent && keys.has(object.parent) ? object.parent : null;
        const siblings = children.get(parent);
        if (siblings) siblings.push(object);
        else children.set(parent, [object]);
    }
    const roots = root === undefined ? children.get(null) ?? [] : objects.filter(object => object.key === root);
    const stack = roots.map(object => ({object, level: 1})).reverse();
    const seen = new Set<EntityKey>();
    const rows: PropWorkspaceObject[] = [];
    while (stack.length) {
        const row = stack.pop()!;
        if (seen.has(row.object.key)) continue;
        seen.add(row.object.key);
        rows.push({...row.object, parent: row.level === 1 ? null : row.object.parent, level: row.level});
        const descendants = children.get(row.object.key) ?? [];
        for (let index = descendants.length - 1; index >= 0; index--) {
            stack.push({object: descendants[index], level: row.level + 1});
        }
    }
    return rows;
}
