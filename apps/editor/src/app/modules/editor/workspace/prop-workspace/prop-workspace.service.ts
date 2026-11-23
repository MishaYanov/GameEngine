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
import type {ComponentEditIntent} from '../components/component-editor';

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
    | SourcePropTab
    | {readonly kind: 'instance'; readonly scope: PropScope; readonly root: EntityKey};

interface SourcePropTab {
    readonly kind: 'source';
    readonly scope: PropScope;
    readonly name: string;
    readonly draft: PrefabSourceDraft;
}

interface PropAncestor {
    readonly tab: SourcePropTab;
    readonly selectedKey: EntityKey | null;
    readonly placementId: string | null;
}

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

/** Owns the isolated Prop document and retained draft trail for nested source navigation. */
@Injectable()
export class PropWorkspaceService {
    private readonly editor = inject(EditorService);
    private readonly editing = inject(SceneEditingService);
    private readonly views = inject(WorkspaceViewService);
    private readonly injector = inject(Injector);
    private readonly dom = inject(DOCUMENT);
    private readonly tab = signal<PropTab | null>(null);
    private readonly ancestors = signal<readonly PropAncestor[]>([]);
    private readonly selectedKey = signal<EntityKey | null>(null);
    private readonly placementId = signal<string | null>(null);
    private readonly opening = signal<PropScope | null>(null);
    private readonly closingChild = signal(false);
    private readonly failure = signal<string | null>(null);
    private readonly editingDraft = signal(false);
    private readonly reviewingClose = signal(false);
    private readonly nestedSource = signal<NestedPropSource | null>(null);
    private readonly reviewingNestedSource = signal(false);
    private nestedReviewRequest = 0;
    private closeReview: {
        readonly draftId: string;
        readonly resolve: (approved: boolean) => void;
    } | null = null;
    private focusedBeforeOpen: HTMLElement | null = null;
    private request = 0;
    private destroyed = false;

    readonly error = this.failure.asReadonly();
    readonly breadcrumbs = computed(() => this.ancestors().map(route => route.tab.name));
    readonly pending = computed(() => this.opening() !== null);
    readonly draftPending = this.editingDraft.asReadonly();
    readonly nestedReview = this.nestedSource.asReadonly();
    readonly nestedReviewPending = this.reviewingNestedSource.asReadonly();
    readonly assets = computed(() => this.editor.modelAssets() ?? []);
    readonly confirmingClose = this.reviewingClose.asReadonly();
    private readonly closeName = signal<string | null>(null);
    readonly closeTargetName = this.closeName.asReadonly();
    readonly canOpen = computed(() => this.editing.ready() && !this.pending() && !this.closingChild() && !this.draftPending() &&
        !this.confirmingClose() && !!this.editor.snapshot());
    readonly atNestedDepthLimit = computed(() => this.ancestors().length >= 3);
    readonly canOpenNested = computed(() => this.canOpen() && !this.atNestedDepthLimit());
    readonly draft = computed(() => {
        const tab = this.tab();
        return tab?.kind === 'source' ? tab.draft : null;
    });
    private readonly draftCommandsReady = computed(() => {
        const snapshot = this.editor.snapshot();
        return !this.draftPending() && !this.pending() && !this.closingChild() &&
            !this.editor.busy() && snapshot?.mode === 'edit' && snapshot.transaction === null;
    });
    readonly canEditDraft = computed(() => this.draft()?.draft_revision !== undefined && this.draftCommandsReady());
    readonly canResolveClose = this.draftCommandsReady;
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
            this.releaseSources();
        });
    }

    async openSource(asset: Pick<ModelAsset, 'asset_id' | 'path' | 'name'>): Promise<boolean> {
        const active = this.draft();
        if (asset.asset_id && active?.asset_id === asset.asset_id) {
            this.views.selectView('prop-workspace');
            return true;
        }
        const ancestor = asset.asset_id ? this.ancestors().findIndex(item => item.tab.draft.asset_id === asset.asset_id) : -1;
        return ancestor >= 0 ? this.backTo(ancestor) : this.loadSource(asset, false);
    }

    async openNestedSource(asset: ModelAsset): Promise<boolean> {
        const placement = this.selectedPlacement();
        if (!placement || placement.target.asset_id !== asset.asset_id || asset.kind !== 'prefab') return false;
        if (!this.canOpenNested()) return false;
        if (this.sourceTabs().some(tab => tab.draft.asset_id === asset.asset_id)) {
            this.failure.set('This source is already open in the Prop breadcrumb trail.');
            return false;
        }
        return this.loadSource(asset, true);
    }

    private async loadSource(asset: Pick<ModelAsset, 'asset_id' | 'path' | 'name'>, nested: boolean): Promise<boolean> {
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
        }, nested);
    }

    async openInstance(root: EntityKey): Promise<boolean> {
        return this.open(async scope => {
            const entity = this.editor.document()?.entities.find(candidate => candidate.key === root);
            if (!entity?.linked_prefab) throw new Error('This object no longer has a linked Prop source.');
            return {kind: 'instance', scope, root};
        });
    }

    async backTo(index: number): Promise<boolean> {
        if (!this.canOpen() || index < 0 || index >= this.ancestors().length) return false;
        while (this.ancestors().length > index) {
            const active = this.tab();
            const parentId = this.ancestors().at(-1)?.tab.draft.draft_id;
            if (active?.kind !== 'source' || !parentId || !await this.reviewClose(active)) return false;
            const ancestor = this.ancestors().at(-1);
            if (this.draft()?.draft_id !== active.draft.draft_id || ancestor?.tab.draft.draft_id !== parentId) return false;
            this.closingChild.set(true);
            try {
                await this.editor.closePrefabSource(active.draft.draft_id);
            } catch (error) {
                this.failure.set(error instanceof Error ? error.message : String(error));
                return false;
            } finally {
                this.closingChild.set(false);
            }
            if (this.draft()?.draft_id !== active.draft.draft_id) return false;
            this.ancestors.update(route => route.slice(0, -1));
            this.tab.set(ancestor.tab);
            this.selectedKey.set(ancestor.selectedKey);
            this.placementId.set(ancestor.placementId);
            this.clearNestedReview();
            this.failure.set(null);
        }
        this.views.selectView('prop-workspace');
        return true;
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

    async refreshNested(placementId: string, previous: string, fingerprint: string): Promise<boolean> {
        const placement = this.selectedPlacement();
        const review = this.nestedReview();
        if (!placement || placement.id !== placementId || placement.target.fingerprint !== previous ||
            previous === fingerprint || review?.asset_id !== placement.target.asset_id || review.fingerprint !== fingerprint ||
            !this.assets().some(asset => asset.kind === 'prefab' &&
                asset.asset_id === placement.target.asset_id && asset.fingerprint === fingerprint)) return false;
        const accepted = await this.runDraft((draft, revision, generation) =>
            this.editor.refreshNestedProp(draft.draft_id, revision, placementId, fingerprint, generation));
        if (accepted) this.clearNestedReview();
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

    editAudioSource(key: EntityKey, intent: ComponentEditIntent): Promise<boolean> {
        const draft = this.draft();
        const entity = draft?.prefab.entities.find(entity => entity.key === key);
        if (!draft || !entity || this.selectedObject()?.key !== key || !this.canEditDraft()) return Promise.resolve(false);
        const id = 'component' in intent ? intent.component.id : intent.component_id;
        const current = entity.components.find(component => component.id === id);
        if (('component' in intent && intent.component.data.kind !== 'audio_source') ||
            (current && current.data.kind !== 'audio_source') || (intent.kind !== 'add' && !current) ||
            ('expected' in intent && intent.expected !== (current ? JSON.stringify(current) : null))) {
            this.failure.set('This Audio Source changed. Discard the edit and review its current values.');
            return Promise.resolve(false);
        }
        const components = entity.components.filter(component => component.id !== id);
        const next = 'component' in intent ? intent.component : intent.kind === 'enabled' && current
            ? {...current, enabled: intent.enabled} : null;
        if (next) components.push(next);
        return this.editDraft({type: 'apply', prefab: {...draft.prefab,
            entities: draft.prefab.entities.map(candidate => candidate.key === key ? {...entity, components} : candidate),
        }});
    }

    async saveDraft(target = this.tab()): Promise<boolean> {
        const saved = await this.runDraft((draft, revision, generation) =>
            this.editor.savePropDraft(draft.draft_id, revision, generation), target);
        if (saved) void this.editor.assets(true).catch(() => undefined);
        return saved;
    }

    async reviewNavigation(): Promise<boolean> {
        if (this.pending() || this.closingChild() || this.draftPending() || this.confirmingClose()) return false;
        const draftIds = this.sourceTabs().map(tab => tab.draft.draft_id);
        for (const draftId of draftIds) {
            const tab = this.sourceTab(draftId);
            if (!tab || !await this.reviewClose(tab)) return false;
        }
        return !this.sourceTabs().some(tab => tab.draft.dirty);
    }

    private reviewClose(tab: SourcePropTab): Promise<boolean> {
        if (!tab.draft.dirty) return Promise.resolve(!this.draftPending());
        if (this.closeReview) return Promise.resolve(false);
        let resolve!: (approved: boolean) => void;
        const promise = new Promise<boolean>(done => (resolve = done));
        this.closeReview = {draftId: tab.draft.draft_id, resolve};
        this.failure.set(null);
        this.closeName.set(tab.name);
        this.reviewingClose.set(true);
        return promise;
    }

    async chooseClose(action: 'save' | 'discard'): Promise<void> {
        const review = this.closeReview;
        const tab = review && this.sourceTab(review.draftId);
        if (!review || !tab) return;
        const accepted = action === 'save' ? await this.saveDraft(tab) : await this.runDraft(
            (draft, revision, generation) => this.editor.editPropDraft(draft.draft_id, revision, {type: 'discard'}, generation), tab);
        if (accepted && this.closeReview === review && this.sourceTab(review.draftId)?.draft.dirty === false) {
            this.finishClose(true);
        }
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
        this.releaseSources();
        this.tab.set(null);
        this.ancestors.set([]);
        this.selectedKey.set(null);
        this.placementId.set(null);
        this.clearNestedReview();
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
    ) => Promise<PropTab | null>, nested = false): Promise<boolean> {
        const scope = this.scope();
        if (!scope || !this.canOpen()) return false;
        const parentId = nested ? this.draft()?.draft_id : null;
        if (nested ? !parentId : !await this.reviewNavigation()) return false;
        if (!this.sameScope(scope, this.scope()) || !this.canOpen()) return false;
        const focused = this.dom.activeElement;
        const selectedKey = this.selectedKey();
        const placementId = this.placementId();
        const request = ++this.request;
        const current = () => !this.destroyed && this.request === request &&
            this.sameScope(scope, this.scope()) && this.editor.snapshot()?.mode === 'edit' &&
            (!nested || this.draft()?.draft_id === parentId && this.placementId() === placementId);
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
                const parent = this.tab();
                if (nested && parent?.kind === 'source') {
                    this.ancestors.update(route => [...route, {tab: parent, selectedKey, placementId}]);
                } else {
                    this.releaseSources();
                    this.ancestors.set([]);
                    this.focusedBeforeOpen = focused instanceof HTMLElement ? focused : null;
                }
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
    ) => Promise<PrefabSourceDraft>, target = this.tab()): Promise<boolean> {
        if (target?.kind !== 'source' || !this.draftCommandsReady()) return false;
        const draft = target.draft;
        const generation = this.editor.workspaceGeneration();
        if (target.scope.generation !== generation || draft.draft_revision === undefined) return false;
        this.failure.set(null);
        this.editingDraft.set(true);
        try {
            const accepted = await run(draft, draft.draft_revision, generation);
            return this.acceptDraft(accepted, false);
        } catch (error) {
            if (!this.destroyed && this.sourceTab(draft.draft_id) &&
                this.editor.workspaceGeneration() === generation) {
                this.failure.set(error instanceof Error ? error.message : String(error));
            }
            return false;
        } finally {
            this.editingDraft.set(false);
        }
    }

    private acceptDraft(draft: PrefabSourceDraft, newerOnly: boolean): boolean {
        const tab = this.sourceTab(draft.draft_id);
        if (this.destroyed || !tab || tab.scope.generation !== this.editor.workspaceGeneration() ||
            draft.workspace_generation !== tab.draft.workspace_generation ||
            draft.asset_id !== tab.draft.asset_id || draft.path !== tab.draft.path || draft.draft_revision === undefined ||
            tab.draft.draft_revision === undefined || draft.draft_revision < tab.draft.draft_revision ||
            (newerOnly && draft.draft_revision === tab.draft.draft_revision)) return false;
        try {
            readPrefabSource(draft.prefab);
        } catch (error) {
            this.failure.set(error instanceof Error ? error.message : String(error));
            return false;
        }
        const updated = {...tab, draft};
        if (this.draft()?.draft_id === draft.draft_id) this.tab.set(updated);
        else this.ancestors.update(route => route.map(item => item.tab.draft.draft_id === draft.draft_id
            ? {...item, tab: updated} : item));
        return true;
    }

    private finishClose(approved: boolean): void {
        const review = this.closeReview;
        this.closeReview = null;
        this.closeName.set(null);
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

    private sourceTabs(): readonly SourcePropTab[] {
        const active = this.tab();
        const ancestors = this.ancestors().map(item => item.tab).reverse();
        return active?.kind === 'source' ? [active, ...ancestors] : ancestors;
    }

    private sourceTab(draftId: string): SourcePropTab | undefined {
        return this.sourceTabs().find(tab => tab.draft.draft_id === draftId);
    }

    private releaseSources(): void {
        for (const tab of this.sourceTabs()) this.closeDraft(tab.draft.draft_id);
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
