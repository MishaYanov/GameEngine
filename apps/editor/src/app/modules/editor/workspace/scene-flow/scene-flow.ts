import {Component, computed, effect, inject, input, signal} from '@angular/core';
import {form, FormField} from '@angular/forms/signals';
import type {EditorCommand, SceneDescriptor, SceneEntity, SceneId} from '../../../../core/editor/editor.models';
import {EditorService} from '../../../../core/editor/editor.service';
import type {PlayStatus} from '../../../../core/editor/viewport-editor.models';
import {ViewportEditorService} from '../../../../core/editor/viewport-editor.service';
import {EditorDialog} from '../../../../shared/components/dialog/dialog';
import {EditorSearchPicker} from '../../../../shared/components/search-picker/search-picker';
import {SceneEditingService} from '../scene-editing.service';
import {RouteOverview} from './route-overview';
import {
    cueRoles, type FlowDraft, flowFields, newCue, newInstance, newRegion, newRoute, readFlow, sceneFlowError,
} from './scene-flow-draft';

type View = 'routes' | 'timelines' | 'regions';
type ObjectKind = 'entities' | 'sensors' | 'cameras' | 'actors' | 'audio';

@Component({
    selector: 'editor-scene-flow',
    imports: [EditorDialog, EditorSearchPicker, FormField, RouteOverview],
    templateUrl: './scene-flow.html',
    styleUrl: './scene-flow.scss',
})
export class SceneFlow {
    readonly opening = input(false);
    private readonly editor = inject(EditorService);
    private readonly editing = inject(SceneEditingService);
    private readonly viewport = inject(ViewportEditorService, {optional: true});
    private generation = -1;
    private baseline = '';
    protected readonly dialogConfig = {width: '62rem'};
    protected readonly open = signal(false);
    protected readonly view = signal<View>('routes');
    protected readonly draft = signal<FlowDraft>(readFlow({}));
    protected readonly fields = form(this.draft);
    protected readonly routeIndex = signal(0);
    protected readonly timelineIndex = signal(0);
    protected readonly instanceIndex = signal(0);
    protected readonly regionIndex = signal(0);
    protected readonly pending = signal(false);
    protected readonly error = signal<string | null>(null);
    protected readonly scenes = computed(() => this.editor.snapshot()?.project.scenes ?? []);
    protected readonly worldScenes = computed(() => {
        const streamed = new Set(this.draft().regions.map(region => region.scene));
        return this.scenes().filter(scene => !streamed.has(scene.id));
    });
    protected readonly sceneOptions = computed(() => this.scenes()
        .map(scene => ({id: scene.id, label: this.label(scene)})));
    protected readonly worldSceneOptions = computed(() => this.worldScenes()
        .map(scene => ({id: scene.id, label: this.label(scene)})));
    protected readonly timelineOptions = computed(() =>
        this.idOptions(this.draft().timelines.map(timeline => timeline.id.trim())));
    protected readonly sceneId = computed(() => this.editor.snapshot()?.active_scene_id ?? '');
    protected readonly entities = computed(() => this.editor.document()?.entities.filter(entity => entity.active !== false) ?? []);
    protected readonly route = computed(() => this.draft().routes[this.routeIndex()]);
    protected readonly sourceTimelineOptions = computed(() => this.sequenceInstances(this.route()?.from));
    protected readonly destinationTimelineOptions = computed(() => this.sequenceInstances(this.route()?.to));
    protected readonly timeline = computed(() => this.draft().timelines[this.timelineIndex()]);
    protected readonly instance = computed(() => this.draft().instances[this.instanceIndex()]);
    protected readonly region = computed(() => this.draft().regions[this.regionIndex()]);
    protected readonly instanceRoles = computed(() => {
        const instance = this.instance();
        const asset = this.draft().timelines.find(timeline => timeline.id.trim() === instance?.asset);
        return asset ? cueRoles(asset) : [];
    });
    protected readonly changed = computed(() => this.open() && JSON.stringify(this.draft()) !== this.baseline);
    protected readonly validation = computed(() => this.changed() ? sceneFlowError(this.draft()) : null);
    protected readonly playing = this.editor.playing;
    protected readonly gameStatus = computed(() => {
        const status = this.viewport?.state()?.game_status;
        return status && this.playing() && status.play_session_id === this.editor.snapshot()?.play_session_id
            ? status : null;
    });

    protected readonly worldGpuAssetBytes = computed(() => {
        const bytes = this.viewport?.state()?.world_gpu_asset_bytes;
        return this.gameStatus() && bytes !== undefined && Number.isSafeInteger(bytes) && bytes >= 0
            ? bytes : null;
    });

    constructor() {
        effect(() => {
            if (this.open() && !this.current()) this.open.set(false);
        });
    }

    show(): boolean {
        const snapshot = this.editor.snapshot();
        if (!snapshot || this.opening() || (snapshot.mode === 'edit' && !this.editing.ready())) return false;
        const draft = readFlow(snapshot.project);
        this.generation = this.editor.workspaceGeneration();
        this.baseline = JSON.stringify(draft);
        this.draft.set(draft);
        this.routeIndex.set(0);
        this.timelineIndex.set(0);
        this.instanceIndex.set(0);
        this.regionIndex.set(0);
        this.view.set('routes');
        this.error.set(null);
        this.open.set(true);
        return true;
    }

    canLeaveProject(): boolean {
        if (!this.open()) return true;
        if (this.pending() || this.changed()) {
            this.error.set('Apply or discard the Scene Flow draft before leaving this project.');
            return false;
        }
        this.open.set(false);
        return true;
    }

    protected label(scene: SceneDescriptor): string {
        return scene.asset_path.endsWith('.json') ? scene.asset_path.slice(0, -5) : scene.asset_path;
    }

    protected sceneLabel(id: SceneId): string {
        const scene = this.scenes().find(candidate => candidate.id === id);
        return scene ? this.label(scene) : id || 'Select a scene';
    }

    private sequenceInstances(scene: SceneId | undefined) {
        return this.idOptions(this.draft().instances.filter(instance => instance.scene === scene)
            .map(instance => instance.id));
    }

    private idOptions(ids: readonly string[]) {
        return [...new Set(ids.filter(id => id.trim()))].map(id => ({id, label: id}));
    }

    protected openTimeline(scene: SceneId, id: string): void {
        const instance = this.draft().instances.findIndex(entry => entry.scene === scene && entry.id === id);
        if (instance < 0) return;
        const asset = this.draft().instances[instance].asset;
        this.instanceIndex.set(instance);
        this.timelineIndex.set(Math.max(0, this.draft().timelines.findIndex(timeline => timeline.id.trim() === asset)));
        this.view.set('timelines');
    }

    protected objectList(scene: SceneId, kind: ObjectKind): string | null {
        return scene === this.sceneId() ? 'scene-flow-' + kind : null;
    }

    protected roleList(scene: SceneId, role: string): string | null {
        const instance = this.instance();
        const timeline = this.draft().timelines.find(asset => asset.id.trim() === instance?.asset);
        const cue = timeline?.cues.find(entry => entry.kind !== 'subtitle' && entry.role.trim() === role);
        if (!cue) return this.objectList(scene, 'entities');
        switch (cue.kind) {
            case 'camera_cut':
                return this.objectList(scene, 'cameras');
            case 'play_clip':
                return this.objectList(scene, 'actors');
            case 'audio':
                return this.objectList(scene, 'audio');
            default:
                return this.objectList(scene, 'entities');
        }
    }

    protected readonly canTestRoute = computed(() => {
        const route = this.route();
        const status = this.gameStatus();
        return !!route && !!status && route.activation === 'manual' && route.from === status.scene_id
            && !status.pending_scene_id && !this.changed() && !this.opening() && !this.pending();
    });
    protected readonly regionFootprint = computed(() => {
        const world = this.gameStatus()?.scene_id ?? this.region()?.world ?? this.sceneId();
        const resident = new Set(this.gameStatus()?.resident_regions ?? []);
        const cells = this.draft().regions.map((region, index) => ({region, index})).filter(({region}) =>
            region.world === world &&
            [region.minX, region.maxX, region.minZ, region.maxZ].every(Number.isFinite) &&
            region.minX < region.maxX && region.minZ < region.maxZ);
        if (!cells.length) return [];
        const left = Math.min(...cells.map(({region}) => region.minX));
        const top = Math.min(...cells.map(({region}) => region.minZ));
        const width = Math.max(...cells.map(({region}) => region.maxX)) - left;
        const height = Math.max(...cells.map(({region}) => region.maxZ)) - top;
        return cells.map(({region, index}) => ({
            index, scene: region.scene, resident: resident.has(region.scene),
            left: (region.minX - left) * 100 / width,
            top: (region.minZ - top) * 100 / height,
            width: (region.maxX - region.minX) * 100 / width,
            height: (region.maxZ - region.minZ) * 100 / height,
        }));
    });

    protected readonly sensors = computed(() => this.entities().filter(entity =>
        entity.collider?.interaction?.sensor && entity.collision_enabled !== false));
    protected readonly cameras = computed(() => this.entities().filter(entity => entity.camera?.enabled));
    protected readonly actors = computed(() => this.entities().filter(entity => entity.animation?.enabled || entity.animator?.enabled));
    protected readonly audioSources = computed(() => this.entities().filter(entity => entity.component_views?.some(component => component.enabled && component.data.kind === 'audio_source')));
    protected readonly entityLists = computed((): readonly { kind: ObjectKind; entries: readonly SceneEntity[] }[] =>
        this.suggestionLists().map(list => ({...list, entries: list.entries.slice(0, 256)})));

    private suggestionLists(): readonly { kind: ObjectKind; entries: readonly SceneEntity[] }[] {
        return [
            {kind: 'entities', entries: this.entities()},
            {kind: 'sensors', entries: this.sensors()},
            {kind: 'cameras', entries: this.cameras()},
            {kind: 'actors', entries: this.actors()},
            {kind: 'audio', entries: this.audioSources()},
        ];
    }

    protected addRoute(): void {
        const from = this.sceneId();
        const to = this.worldScenes().find(scene => scene.id !== from)?.id ?? '';
        this.routeIndex.set(this.draft().routes.length);
        this.draft.update(draft => ({...draft, routes: [...draft.routes, newRoute(from, to)]}));
    }

    protected addTimeline(): void {
        this.timelineIndex.set(this.draft().timelines.length);
        this.draft.update(draft => ({
            ...draft, timelines: [...draft.timelines, {
                id: '', duration: 3_000, cues: [newCue()],
            }]
        }));
    }

    protected addCue(): void {
        const timeline = this.timeline();
        if (!timeline || timeline.cues.length >= 256) return;
        const next = Math.min(timeline.duration - 1, (timeline.cues.at(-1)?.at ?? -1_000) + 1_000);
        this.updateCues(cues => [...cues, newCue(Math.max(0, next))]);
    }

    protected removeCue(index: number): void {
        this.updateCues(cues => cues.filter((_, position) => position !== index));
    }

    protected addInstance(): void {
        this.instanceIndex.set(this.draft().instances.length);
        this.draft.update(draft => ({
            ...draft, instances: [...draft.instances, newInstance(
                this.sceneId(), this.timeline()?.id.trim() ?? '',
            )]
        }));
    }

    protected bindRole(role: string, key: string): void {
        this.draft.update(draft => ({
            ...draft, instances: draft.instances.map((instance, index) =>
                index === this.instanceIndex() ? {
                    ...instance,
                    bindings: {...instance.bindings, [role]: key}
                } : instance),
        }));
    }

    protected bindingValue(role: string): string {
        const bindings = this.instance()?.bindings;
        return bindings && Object.hasOwn(bindings, role) ? bindings[role] : '';
    }

    protected addRegion(): void {
        const world = this.sceneId();
        const used = new Set(this.draft().regions.map(region => region.scene));
        const cell = this.scenes().find(scene => scene.id !== world && !used.has(scene.id))?.id ?? '';
        this.regionIndex.set(this.draft().regions.length);
        this.draft.update(draft => ({...draft, regions: [...draft.regions, newRegion(world, cell)]}));
    }

    protected remove(kind: 'routes' | 'timelines' | 'instances' | 'regions'): void {
        const selected = {
            routes: this.routeIndex,
            timelines: this.timelineIndex,
            instances: this.instanceIndex,
            regions: this.regionIndex
        }[kind];
        this.draft.update(draft => {
            const remaining = draft[kind].filter((_, index) => index !== selected());
            return {...draft, [kind]: remaining};
        });
        selected.update(index => Math.max(0, Math.min(index, this.draft()[kind].length - 1)));
    }

    protected testRoute(): void {
        const route = this.route();
        const status = this.gameStatus();
        if (route && status && this.canTestRoute()) {
            void this.executePlayAction({type: 'request_scene_route', route_id: route.id}, status);
        }
    }

    protected retryRoute(rendered: PlayStatus): void {
        const status = this.gameStatus();
        const attemptId = rendered.failed_scene_attempt_id;
        if (attemptId === undefined || !status || status.pending_scene_id ||
            status.play_session_id !== rendered.play_session_id || status.run_id !== rendered.run_id ||
            status.scene_generation !== rendered.scene_generation || status.failed_scene_attempt_id !== attemptId) return;
        void this.executePlayAction({
            type: 'retry_scene_route', run_id: rendered.run_id,
            scene_generation: rendered.scene_generation, attempt_id: attemptId,
        }, status);
    }

    private async executePlayAction(command: EditorCommand, status: PlayStatus): Promise<void> {
        const snapshot = this.editor.snapshot();
        if (!snapshot || snapshot.mode !== 'play' || !this.current() || this.pending() || this.opening()) return;
        this.pending.set(true);
        this.error.set(null);
        try {
            await this.editor.execute(command, this.generation, snapshot.revision);
            if (this.current() && this.gameStatus()?.run_id === status.run_id) this.open.set(false);
        } catch (error) {
            if (this.current() && this.gameStatus()?.run_id === status.run_id) {
                this.error.set(error instanceof Error ? error.message : String(error));
            }
        } finally {
            this.pending.set(false);
        }
    }

    protected async apply(): Promise<void> {
        if (!this.current() || this.opening() || this.pending() || this.playing() || !this.changed() || this.validation()) return;
        this.pending.set(true);
        this.error.set(null);
        try {
            const pending = this.draft();
            const generation = this.generation;
            const applied = await this.editing.withCommittedEdit(async () => {
                const snapshot = this.editor.snapshot();
                if (!snapshot || !this.current() || snapshot.mode !== 'edit') return;
                if (JSON.stringify(readFlow(snapshot.project)) !== this.baseline) {
                    throw new Error('Scene Flow changed while the dialog was open. Reopen it to review current settings.');
                }
                await this.editor.execute({
                    type: 'set_project',
                    project: {...snapshot.project, ...flowFields(pending)},
                }, generation, snapshot.revision);
                if (this.current()) this.open.set(false);
            });
            if (!applied && this.current()) this.error.set(this.editing.error() ?? 'Finish the current edit and try again.');
        } finally {
            this.pending.set(false);
        }
    }

    private updateCues(edit: (cues: FlowDraft['timelines'][number]['cues']) => FlowDraft['timelines'][number]['cues']): void {
        this.draft.update(draft => ({
            ...draft,
            timelines: draft.timelines.map((timeline, index) => index === this.timelineIndex()
                ? {...timeline, cues: edit(timeline.cues)} : timeline),
        }));
    }

    private current(): boolean {
        return this.generation === this.editor.workspaceGeneration() && !!this.editor.snapshot();
    }
}
