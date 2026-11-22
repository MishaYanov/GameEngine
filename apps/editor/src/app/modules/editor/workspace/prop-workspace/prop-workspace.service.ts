import {
    afterNextRender, computed, DestroyRef, DOCUMENT, effect, inject, Injectable, Injector, signal, untracked,
} from '@angular/core';
import type {ComponentRecord} from '../../../../core/editor/editor-components';
import type {EntityKey, ModelAsset, SceneEntity, SceneId, Transform} from '../../../../core/editor/editor.models';
import {EditorService} from '../../../../core/editor/editor.service';
import type {PrefabSourceDraft} from '../../../../core/editor/linked-prefabs.models';
import {readPrefabSource} from '../prefabs/prefab-source-values';
import {SceneEditingService} from '../scene-editing.service';
import {WorkspaceViewService} from '../scene-viewport/workspace-view.service';

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
    readonly references: readonly {readonly slot: string; readonly target: string}[];
};

interface PropObjectRecord {
    readonly key: EntityKey;
    readonly name: string;
    readonly parent: EntityKey | null;
    readonly transform: Transform;
    readonly components: readonly ComponentRecord[];
}

/** Owns a single isolated, read-only prop document and its local selection. */
@Injectable()
export class PropWorkspaceService {
    private readonly editor = inject(EditorService);
    private readonly editing = inject(SceneEditingService);
    private readonly views = inject(WorkspaceViewService);
    private readonly injector = inject(Injector);
    private readonly dom = inject(DOCUMENT);
    private readonly tab = signal<PropTab | null>(null);
    private readonly selectedKey = signal<EntityKey | null>(null);
    private readonly opening = signal<PropScope | null>(null);
    private readonly failure = signal<string | null>(null);
    private sourceDraft: PrefabSourceDraft | null = null;
    private focusedBeforeOpen: HTMLElement | null = null;
    private request = 0;
    private destroyed = false;

    readonly error = this.failure.asReadonly();
    readonly pending = computed(() => this.opening() !== null);
    readonly canOpen = computed(() => this.editing.ready() && !this.pending() && !!this.editor.snapshot());
    readonly document = computed<PropWorkspaceDocument | null>(() => {
        const tab = this.tab();
        if (!tab) return null;
        if (tab.kind === 'source') {
            const prefab = tab.draft.prefab;
            const objects = flattenObjects(prefab.entities.map(entity => ({...entity, parent: entity.parent ?? null})));
            return {...tab, path: tab.draft.path, objects,
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
        return {...tab, name: root.name, path: root.linked_prefab.source.path, objects, references};
    });
    readonly selectedObject = computed(() => {
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
            if (tab && (!this.sameScope(tab.scope, scope) || !document)) untracked(() => this.close(false));
            if (opening && (playing || !this.sameScope(opening, scope))) untracked(() => this.cancelOpen());
        });
        inject(DestroyRef).onDestroy(() => {
            this.destroyed = true;
            this.request++;
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
        if (this.document()?.objects.some(item => item.key === key)) this.selectedKey.set(key);
    }

    cancelOpening(): void {
        if (this.pending()) this.cancelOpen();
    }

    close(restoreFocus = true, fallbackFocus?: () => void): void {
        this.cancelOpen();
        this.tab.set(null);
        this.selectedKey.set(null);
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
        if (!scope || !this.canOpen()) return false;
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
