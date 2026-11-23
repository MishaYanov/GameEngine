import {afterNextRender, Component, computed, DestroyRef, effect, ElementRef, inject, Injector, signal, untracked, viewChild} from '@angular/core';
import {listen} from '@tauri-apps/api/event';
import type {AnimationPreviewDocument} from '../../../../core/editor/animation-workspace.models';
import {reserveAnimationPreviewRequest} from '../../../../core/editor/animation-workspace.models';
import type {
    AuthoredClip, ClipAssetDocument, ClipAssetIndex, ClipTimelineDraft, ClipOwner, Quaternion, VisualClipPreview, VisualClipTarget,
} from '../../../../core/editor/authored-clip.models';
import type {SceneEntity, Transform, Vec3} from '../../../../core/editor/editor.models';
import {EditorService} from '../../../../core/editor/editor.service';
import {PropWorkspaceService} from '../prop-workspace/prop-workspace.service';
import {SceneEditingService} from '../scene-editing.service';
import {ClipTimeline} from './timeline';
import {AnimationWorkspaceService} from './animation-workspace.service';

interface DraftTarget {
    readonly owner: ClipOwner;
    readonly target: VisualClipTarget;
    readonly name: string;
    readonly transform: Transform;
}

interface ClipDraft {
    readonly name: string;
    readonly path: string;
    readonly duration: string;
    readonly owner: ClipOwner;
    readonly target: VisualClipTarget;
    readonly targetName: string;
    readonly fromPosition: readonly string[];
    readonly toPosition: readonly string[];
    readonly fromRotation: readonly string[];
    readonly toRotation: readonly string[];
}

type KeyField = 'fromPosition' | 'toPosition' | 'fromRotation' | 'toRotation';

@Component({
    selector: 'editor-authored-clip-editor',
    imports: [ClipTimeline],
    templateUrl: './authored-clip-editor.html',
    styleUrl: './animation-workspace.scss',
})
export class AuthoredClipEditor {
    private readonly editor = inject(EditorService);
    private readonly scene = inject(SceneEditingService);
    private readonly props = inject(PropWorkspaceService);
    private readonly animation = inject(AnimationWorkspaceService);
    protected readonly assets = signal<readonly ClipAssetIndex[]>([]);
    protected readonly selected = signal<ClipAssetDocument | null>(null);
    protected readonly draft = signal<ClipDraft | null>(null);
    protected readonly preview = signal<VisualClipPreview | null>(null);
    protected readonly busy = signal(false);
    protected readonly timelineOpen = signal(false);
    private readonly timelineTrigger = viewChild<ElementRef<HTMLButtonElement>>('timelineTrigger');
    private readonly injector = inject(Injector);
    protected readonly error = signal<string | null>(null);
    protected readonly message = signal<string | null>(null);
    protected readonly axes = ['X', 'Y', 'Z', 'W'] as const;
    protected readonly keyFields: readonly {readonly field: KeyField; readonly label: string}[] = [
        {field: 'fromPosition', label: 'Start position'}, {field: 'fromRotation', label: 'Start rotation quaternion'},
        {field: 'toPosition', label: 'End position'}, {field: 'toRotation', label: 'End rotation quaternion'},
    ];
    private destroyed = false;
    private lastGeneration: number | null = null;

    private readonly target = computed<DraftTarget | null>(() => {
        const snapshot = this.editor.snapshot();
        if (!snapshot || snapshot.mode !== 'edit') return null;
        if (this.animation.scope() === 'prop') {
            const source = this.props.draft(), object = this.props.selectedObject();
            if (!source || !object?.parent || source.dirty ||
                !object.components.some(component => component.enabled && component.data.kind === 'mesh_renderer')) return null;
            return {
                owner: {kind: 'prop', asset_id: source.asset_id},
                target: {kind: 'prop', address: {kind: 'object', placement_path: [], source_entity: object.key}},
                name: object.name, transform: object.transform,
            };
        }
        const entity = this.scene.entity();
        if (!entity?.renderable || !entity.parent) return null;
        return {
            owner: {kind: 'scene', scene_id: snapshot.active_scene_id},
            target: {kind: 'scene', entity_key: entity.key},
            name: entity.name, transform: entity.transform,
        };
    });

    protected readonly available = computed(() => this.editor.nativeAvailable && !this.busy() &&
        this.editor.snapshot()?.mode === 'edit' && this.editor.snapshot()?.transaction === null);
    protected readonly canCreate = computed(() => this.available() && this.target() !== null);
    protected readonly canSave = computed(() => {
        const draft = this.draft();
        return this.available() && draft !== null && draft.name.trim() !== '' && draft.path.trim() !== '';
    });

    private readonly apertureContext = computed(() => {
        const snapshot = this.editor.snapshot();
        const clip = this.selected();
        return JSON.stringify([
            this.editor.workspaceGeneration(), snapshot?.revision, snapshot?.active_scene_id, snapshot?.mode,
            clip?.asset.fingerprint, clip ? this.previewScope(clip.clip.owner) : null,
        ]);
    });

    constructor() {
        effect(() => {
            const generation = this.editor.workspaceGeneration();
            const snapshot = this.editor.snapshot();
            if (!snapshot || !this.editor.nativeAvailable || this.lastGeneration === generation) return;
            this.lastGeneration = generation;
            untracked(() => {
                this.selected.set(null);
                this.draft.set(null);
                void this.loadLibrary(generation);
            });
        });
        effect(() => {
            this.apertureContext();
            untracked(() => this.closePreview());
        });
        let unsubscribe: (() => void) | undefined;
        if (this.editor.nativeAvailable) {
            void listen<unknown>('editor://prop-preview-error', ({payload}) => {
                if (!payload || typeof payload !== 'object') return;
                const event = payload as {token?: unknown; workspace_generation?: unknown; message?: unknown};
                const preview = this.preview();
                if (preview && event.token === preview.token && event.workspace_generation === preview.workspace_generation &&
                    typeof event.message === 'string') {
                    this.closePreview();
                    this.error.set(event.message);
                }
            }).then(release => {
                if (this.destroyed) release();
                else unsubscribe = release;
            }).catch(() => undefined);
        }
        inject(DestroyRef).onDestroy(() => {
            this.destroyed = true;
            this.closePreview();
            unsubscribe?.();
        });
    }

    protected openTimeline(): void {
        if (!this.available() || !this.selected()) return;
        this.closePreview();
        this.timelineOpen.set(true);
    }

    protected closeTimeline(): void {
        this.timelineOpen.set(false);
        afterNextRender(() => this.timelineTrigger()?.nativeElement.focus({preventScroll: true}), {injector: this.injector});
    }

    protected timelineSaved(draft: ClipTimelineDraft): void {
        this.selected.set(draft);
        this.draft.set(openForm(draft));
        void this.loadLibrary(this.editor.workspaceGeneration());
    }

    protected begin(): void {
        const target = this.target();
        if (!target || !this.canCreate()) return;
        this.closePreview();
        this.selected.set(null);
        this.error.set(null);
        this.message.set(null);
        const position = target.transform.position.map(String);
        const rotation = fromEuler(target.transform.rotation).map(String);
        this.draft.set({
            name: '', path: '', duration: '1000', owner: target.owner, target: target.target, targetName: target.name,
            fromPosition: position, toPosition: position, fromRotation: rotation, toRotation: rotation,
        });
    }

    protected async open(assetId: string): Promise<void> {
        if (!assetId || !this.available()) return;
        const generation = this.editor.workspaceGeneration();
        await this.run(async () => {
            const clip = await this.editor.openAuthoredClip(assetId, generation);
            if (this.current(generation)) this.accept(clip);
        });
    }

    protected update(field: 'name' | 'path' | 'duration', value: string): void {
        this.draft.update(draft => {
            if (!draft) return null;
            if (field === 'name' && !this.selected() && (!draft.path || draft.path === pathFromName(draft.name))) {
                return {...draft, name: value, path: pathFromName(value)};
            }
            return {...draft, [field]: value};
        });
    }

    protected updateKey(field: KeyField, axis: number, value: string): void {
        this.draft.update(draft => draft && {
            ...draft, [field]: draft[field].map((number, index) => index === axis ? value : number),
        });
    }

    protected async save(): Promise<void> {
        const form = this.draft(), snapshot = this.editor.snapshot(), selected = this.selected();
        if (!form || !snapshot || !this.canSave()) return;
        const generation = this.editor.workspaceGeneration();
        await this.run(async () => {
            const clip = buildClip(form);
            const replacement = selected && {asset_id: selected.asset.asset_id, fingerprint: selected.asset.fingerprint};
            const saved = await this.editor.saveAuthoredClip(form.path.trim(), clip, replacement, generation, snapshot.revision);
            if (!this.current(generation)) return;
            this.accept(saved);
            this.message.set(`Saved ${saved.clip.name}.`);
            await this.loadLibrary(generation);
        });
    }

    protected async openPreview(): Promise<void> {
        const selected = this.selected(), snapshot = this.editor.snapshot();
        if (!selected || !snapshot || !this.available()) return;
        const generation = this.editor.workspaceGeneration(), context = this.apertureContext();
        const request = reserveAnimationPreviewRequest();
        await this.run(async () => {
            this.closePreview();
            const preview = await this.editor.openAuthoredClipPreview({
                assetId: selected.asset.asset_id, fingerprint: selected.asset.fingerprint,
                scope: this.previewScope(selected.clip.owner), expectedRevision: snapshot.revision,
                expectedGeneration: selected.workspace_generation, requestId: request,
            }, generation);
            if (this.destroyed || context !== this.apertureContext() || preview.request_id !== request ||
                preview.workspace_generation !== selected.workspace_generation || preview.revision !== snapshot.revision) {
                void this.editor.closeAuthoredClipPreview(preview.token).catch(() => undefined);
                return;
            }
            this.preview.set(preview);
            this.animation.authoredReady.set(true);
        });
    }

    protected async sample(tick: number): Promise<void> {
        const preview = this.preview();
        if (!preview || !this.available() || !Number.isSafeInteger(tick) || tick < 0 || tick > preview.duration_ticks) return;
        await this.run(async () => {
            const next = await this.editor.sampleAuthoredClipPreview(preview.token, tick, this.editor.workspaceGeneration());
            if (this.preview()?.token === next.token && next.request_id === preview.request_id &&
                next.revision === preview.revision && next.workspace_generation === preview.workspace_generation) {
                this.preview.set(next);
            }
        });
    }

    protected closePreview(): void {
        const preview = this.preview();
        this.preview.set(null);
        if (preview) {
            this.animation.authoredReady.set(false);
            void this.editor.closeAuthoredClipPreview(preview.token).catch(() => undefined);
        }
    }

    private accept(clip: ClipAssetDocument): void {
        this.closePreview();
        this.selected.set(clip);
        this.draft.set(openForm(clip));
        this.error.set(null);
        this.message.set(null);
    }

    private async loadLibrary(generation: number): Promise<void> {
        try {
            const assets = await this.editor.listAuthoredClips(generation);
            if (this.current(generation)) this.assets.set(assets);
        } catch (error) {
            if (this.current(generation)) this.error.set(String(error));
        }
    }

    protected previewScope(owner: ClipOwner): AnimationPreviewDocument | null {
        const snapshot = this.editor.snapshot();
        if (owner.kind === 'scene' || !snapshot) return null;
        const prop = this.props.document();
        if (this.animation.scope() === 'prop' && prop?.kind === 'source' && owner.asset_id === prop.draft.asset_id) {
            return {kind: 'source', asset_id: owner.asset_id, draft_id: prop.draft.draft_id, draft_revision: prop.draft.draft_revision ?? 0};
        }
        const entities = new Map(snapshot.document.entities.map(entity => [entity.key, entity]));
        let candidate: SceneEntity | undefined = this.animation.scope() === 'prop' && prop?.kind === 'instance'
            ? entities.get(prop.root) : this.scene.entity() ?? undefined;
        for (let depth = 0; candidate && depth < entities.size; depth++) {
            if (candidate.linked_prefab?.source.source_id === owner.asset_id) {
                return {kind: 'instance', scene_id: snapshot.active_scene_id, root_key: candidate.key};
            }
            candidate = candidate.parent ? entities.get(candidate.parent) : undefined;
        }
        return null;
    }

    private current(generation: number): boolean {
        return !this.destroyed && this.editor.workspaceGeneration() === generation;
    }

    private async run(operation: () => Promise<void>): Promise<void> {
        if (this.busy()) return;
        const generation = this.editor.workspaceGeneration();
        this.busy.set(true);
        this.error.set(null);
        try {
            await operation();
        } catch (error) {
            if (this.current(generation)) this.error.set(String(error));
        } finally {
            this.busy.set(false);
        }
    }
}

function pathFromName(name: string): string {
    const slug = name.trim().toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '');
    return slug ? slug + '.clip.json' : '';
}

function fromEuler([x, y, z]: Vec3): Quaternion {
    const [s1, s2, s3] = [x, y, z].map(angle => Math.sin(angle / 2));
    const [c1, c2, c3] = [x, y, z].map(angle => Math.cos(angle / 2));
    return [
        s1 * c2 * c3 + c1 * s2 * s3, c1 * s2 * c3 - s1 * c2 * s3,
        c1 * c2 * s3 + s1 * s2 * c3, c1 * c2 * c3 - s1 * s2 * s3,
    ];
}

function numeric(values: readonly string[]): readonly number[] {
    const numbers = values.map(value => value.trim() === '' ? NaN : Number(value));
    if (!numbers.every(Number.isFinite)) throw new Error('Enter finite position and rotation values.');
    return numbers;
}

function buildClip(form: ClipDraft): AuthoredClip {
    const duration = Number(form.duration);
    if (!Number.isSafeInteger(duration) || duration < 1 || duration > 600_000) {
        throw new Error('Duration must be a whole number of milliseconds between 1 and 600000.');
    }
    return {
        schema_version: 1, name: form.name.trim(), owner: form.owner, duration_ticks: duration,
        tracks: [
            {target: form.target, track: {kind: 'local_position', interpolation: 'linear', keys: [
                {at: 0, value: numeric(form.fromPosition) as Vec3}, {at: duration, value: numeric(form.toPosition) as Vec3},
            ]}},
            {target: form.target, track: {kind: 'local_rotation', interpolation: 'linear', keys: [
                {at: 0, value: numeric(form.fromRotation) as Quaternion}, {at: duration, value: numeric(form.toRotation) as Quaternion},
            ]}},
        ],
    };
}

function openForm(document: ClipAssetDocument): ClipDraft | null {
    const clip = document.clip;
    if (clip.schema_version === 2) return null;
    const position = clip.tracks.find(entry => entry.track.kind === 'local_position'),
        rotation = clip.tracks.find(entry => entry.track.kind === 'local_rotation');
    if (!position || !rotation || clip.tracks.length !== 2 || JSON.stringify(position.target) !== JSON.stringify(rotation.target) ||
        ![position.track, rotation.track].every(track => track.interpolation === 'linear' && track.keys.length === 2 &&
            track.keys[0].at === 0 && track.keys[1].at === clip.duration_ticks)) return null;
    const target = position.target;
    return {
        name: clip.name, path: document.asset.path, duration: String(clip.duration_ticks), owner: clip.owner, target,
        targetName: target.kind === 'scene' ? target.entity_key : target.address.kind === 'object' ? target.address.source_entity : '',
        fromPosition: position.track.keys[0].value.map(String), toPosition: position.track.keys[1].value.map(String),
        fromRotation: rotation.track.keys[0].value.map(String), toRotation: rotation.track.keys[1].value.map(String),
    };
}
