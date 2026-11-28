import {
    afterNextRender, Component, computed, DestroyRef, effect, ElementRef, inject, Injector, input, linkedSignal, output,
    signal, untracked, viewChild,
} from '@angular/core';
import type {AnimationPreviewDocument} from '../../../../core/editor/animation-workspace.models';
import type {ClipAssetDocument, ClipKey, ClipKeyValue, ClipTimelineDraft, Quaternion, TimelineEdit} from '../../../../core/editor/authored-clip.models';
import type {Vec3} from '../../../../core/editor/editor.models';
import {EditorService} from '../../../../core/editor/editor.service';
import {EditorDialog} from '../../../../shared/components/dialog/dialog';
import {EditorSearchPicker} from '../../../../shared/components/search-picker/search-picker';
import {PropWorkspaceService} from '../prop-workspace/prop-workspace.service';
import {AnimationWorkspaceService} from './animation-workspace.service';
import {TimelineEditService} from './timeline-edit.service';

type JumpDirection = 'previous' | 'next';

function adjacentMoments<T extends {readonly at: number}>(
    moments: readonly T[], tick: number,
): Readonly<Record<JumpDirection, T | null>> {
    let previous: T | null = null;
    let next: T | null = null;
    for (const moment of moments) {
        if (moment.at < tick && (previous === null || moment.at > previous.at)) previous = moment;
        if (moment.at > tick && (next === null || moment.at < next.at)) next = moment;
    }
    return {previous, next};
}

@Component({
    selector: 'editor-clip-timeline',
    imports: [EditorDialog, EditorSearchPicker],
    providers: [TimelineEditService],
    templateUrl: './timeline.html',
    styleUrl: './timeline.scss',
})
export class ClipTimeline {
    readonly asset = input.required<ClipAssetDocument>();
    readonly scope = input<AnimationPreviewDocument | null>(null);
    readonly initialMarker = input('');
    readonly closed = output<void>();
    readonly saved = output<ClipTimelineDraft>();
    protected readonly timeline = inject(TimelineEditService);
    private readonly injector = inject(Injector);
    private readonly editor = inject(EditorService);
    private readonly props = inject(PropWorkspaceService, {optional: true});
    private readonly scrollLane = viewChild<ElementRef<HTMLElement>>('scrollLane');
    private readonly rulerCursor = viewChild<ElementRef<HTMLElement>>('rulerCursor');
    protected readonly track = signal(0);
    private readonly key = signal('');
    private readonly marker = signal('');
    private revealedMarker = '';
    protected readonly axes = ['X', 'Y', 'Z', 'W'] as const;
    protected readonly tracks = computed(() => this.timeline.draft()?.clip.tracks ?? []);
    protected readonly trackOptions = computed(() => {
        const owner = this.timeline.draft()?.clip.owner;
        const snapshot = this.editor.snapshot();
        const scope = this.scope();
        const sceneNames = owner?.kind === 'scene' && owner.scene_id === snapshot?.active_scene_id
            ? new Map(snapshot.document?.entities?.map(entity => [entity.key, entity.name]))
            : null;
        const prop = this.props?.document();
        const linked = scope?.kind === 'instance' && scope.scene_id === snapshot?.active_scene_id
            ? snapshot.document?.entities?.find(entity => entity.key === scope.root_key)?.linked_prefab?.source
            : null;
        const source = owner?.kind === 'prop' && scope?.kind === 'source' && prop?.kind === 'source' &&
        owner.asset_id === scope.asset_id && owner.asset_id === prop.draft.asset_id && prop.draft.draft_id === scope.draft_id
            ? prop.draft.prefab
            : owner?.kind === 'prop' && linked?.source_id === owner.asset_id ? linked.prefab : null;
        const propNames = new Map(source?.entities.map(entity => [entity.key, entity.name]));
        const placements = new Map(source?.composition?.placements.map(item => [item.id, item.name]));
        const options = this.tracks().map(({target, track}, index) => {
            let key: string;
            let name: string | undefined;
            if (target.kind === 'scene') {
                key = target.entity_key;
                name = sceneNames?.get(key);
            } else {
                const address = target.address;
                if (address.kind === 'object') {
                    key = [...address.placement_path, address.source_entity].join('/');
                    if (!address.placement_path.length) name = propNames.get(address.source_entity);
                } else {
                    key = address.placement_path.join('/');
                    if (address.placement_path.length === 1) name = placements.get(key);
                }
            }
            const property = track.kind === 'local_position' ? 'Position' : 'Rotation';
            return {id: String(index), label: `${property} · ${name ?? key}`, keywords: key};
        });
        const counts = new Map<string, number>();
        for (const option of options) counts.set(option.label, (counts.get(option.label) ?? 0) + 1);
        return options.map(option => counts.get(option.label) === 1
            ? option : {...option, label: `${option.label} (${option.keywords})`});
    });
    protected readonly keys = computed<readonly ClipKey<Vec3 | Quaternion>[]>(() => this.tracks()[this.track()]?.track.keys ?? []);
    protected readonly selected = computed(() => this.keys().find(key => key.id === this.key()) ?? this.keys().at(0) ?? null);
    protected readonly values = linkedSignal(() => this.selected()?.value.map(String) ?? []);
    protected readonly markers = computed(() => this.timeline.draft()?.clip.markers ?? []);
    protected readonly selectedMarker = computed(() => this.markers().find(marker => marker.id === this.marker()) ?? null);
    protected readonly label = linkedSignal(() => this.selectedMarker()?.label ?? '');
    protected readonly closing = signal(false);
    protected readonly resolvingClose = signal(false);
    private navigationAnswer: ((allowed: boolean) => void) | null = null;
    protected readonly closePending = computed(() => this.resolvingClose() || this.timeline.busy());
    private readonly navigationBlocked = computed(() =>
        this.timeline.playing() || this.timeline.drag() !== null || this.closePending() || this.closing());
    private readonly identifiedKeys = computed(() => this.keys().filter(key => !!key.id));
    protected readonly adjacentKeys = computed(() => this.navigationBlocked()
        ? {previous: null, next: null} : adjacentMoments(this.identifiedKeys(), this.timeline.playhead()));
    protected readonly adjacentBookmarks = computed(() => this.navigationBlocked()
        ? {previous: null, next: null} : adjacentMoments(this.markers(), this.timeline.playhead()));
    protected readonly rulerKeys = computed(() => this.keys().map(key => ({key, percent: this.percentAt(key.at)})));
    protected readonly rulerBookmarks = computed(() =>
        this.markers().map(marker => ({marker, percent: this.percentAt(marker.at)})));
    protected readonly rulerPlayhead = computed(() => this.percentAt(this.timeline.playhead()));
    protected readonly rulerMidpoint = computed(() => Math.floor(this.timeline.duration() / 2));

    constructor() {
        const release = inject(AnimationWorkspaceService).registerDraftReview(() => this.reviewNavigation());
        inject(DestroyRef).onDestroy(() => {
            release();
            this.navigationAnswer?.(false);
        });
        effect(() => {
            const asset = this.asset(), scope = this.scope();
            untracked(() => this.timeline.connect(asset, scope));
        });
        effect(() => {
            const id = this.initialMarker();
            const draft = this.timeline.draft();
            if (!id || !draft || (draft.asset.asset_id !== this.asset().asset.asset_id)) return;
            const marker = draft.clip.markers?.find(entry => entry.id === id);
            const reveal = `${draft.draft_id}:${id}`;
            if (!marker || reveal === this.revealedMarker) return;
            this.revealedMarker = reveal;
            untracked(() => this.selectMarker(marker.id, marker.at));
        });
    }

    protected selectKey(key: ClipKey<Vec3 | Quaternion>): void {
        if (!key.id) return;
        this.key.set(key.id);
        this.seekSelection(key.at);
    }

    protected selectTrack(value: number): void {
        if (this.tracks()[value]) this.track.set(value);
    }

    protected updateValue(index: number, value: string): void {
        this.values.update(values => values.map((item, axis) => axis === index ? value : item));
    }

    protected insert(record: boolean): void {
        const value = record ? this.keyValue() : null;
        if (record && !value) return;
        void this.edit({kind: 'insert_key', track: this.track(), at: this.timeline.playhead(), value});
    }

    protected setValue(): void {
        const keyId = this.selected()?.id, value = this.keyValue();
        if (keyId && value) void this.edit({kind: 'set_key_value', key_id: keyId, value});
    }

    protected deleteKey(): void {
        const keyId = this.selected()?.id;
        if (keyId) void this.edit({kind: 'remove_key', key_id: keyId});
    }

    protected beginDrag(): void {
        const key = this.selected();
        if (key?.id) this.timeline.beginDrag(key.id, key.at);
    }

    protected dragTo(value: number): void {
        if (!this.timeline.drag()) this.beginDrag();
        this.timeline.moveDrag(value);
    }

    protected selectMarker(id: string, at: number): void {
        this.marker.set(id);
        this.seekSelection(at);
    }

    protected jump(direction: JumpDirection, target: 'key' | 'bookmark'): boolean {
        if (target === 'key') {
            const key = this.adjacentKeys()[direction];
            if (!key) return false;
            this.selectKey(key);
        } else {
            const bookmark = this.adjacentBookmarks()[direction];
            if (!bookmark) return false;
            this.selectMarker(bookmark.id, bookmark.at);
        }
        return true;
    }

    protected addMarker(): void {
        const label = this.label().trim();
        if (label) void this.edit({kind: 'add_marker', at: this.timeline.playhead(), label});
    }

    protected renameMarker(): void {
        const marker = this.selectedMarker(), label = this.label().trim();
        if (marker && label) void this.edit({kind: 'rename_marker', marker_id: marker.id, label});
    }

    protected moveMarker(): void {
        const marker = this.selectedMarker();
        if (marker) void this.edit({kind: 'move_marker', marker_id: marker.id, at: this.timeline.playhead()});
    }

    protected removeMarker(): void {
        const marker = this.selectedMarker();
        if (marker) void this.edit({kind: 'remove_marker', marker_id: marker.id});
    }

    protected async save(): Promise<void> {
        const saved = await this.timeline.save();
        if (saved) this.saved.emit(saved);
    }

    reviewNavigation(): Promise<boolean> {
        if (this.closePending() || this.closing()) return Promise.resolve(false);
        this.timeline.stop();
        if (this.timeline.draft()?.dirty) {
            this.closing.set(true);
            return new Promise(resolve => this.navigationAnswer = resolve);
        }
        return this.resolveClose('close');
    }

    protected keepEditing(): void {
        if (this.closePending()) return;
        this.closing.set(false);
        this.answerNavigation(false);
    }

    protected async resolveClose(choice: 'save' | 'discard' | 'close'): Promise<boolean> {
        if (this.closePending()) return false;
        this.resolvingClose.set(true);
        try {
            if (choice === 'save') {
                const saved = await this.timeline.save();
                if (!saved) return false;
                this.saved.emit(saved);
            }
            if (!await this.timeline.close(choice === 'discard')) return false;
            this.closing.set(false);
            this.answerNavigation(true);
            this.closed.emit();
            return true;
        } finally {
            this.resolvingClose.set(false);
        }
    }

    private answerNavigation(allowed: boolean): void {
        const answer = this.navigationAnswer;
        this.navigationAnswer = null;
        answer?.(allowed);
    }

    protected onKeys(event: KeyboardEvent): void {
        if (event.key === 'Escape' && this.timeline.drag()) {
            event.preventDefault();
            event.stopPropagation();
            this.timeline.cancelDrag();
            return;
        }
        if (event.target instanceof HTMLInputElement || event.target instanceof HTMLSelectElement ||
            event.altKey || this.timeline.busy()) return;
        if (event.ctrlKey || event.metaKey) {
            if (event.key.toLowerCase() === 'z') {
                event.preventDefault();
                event.stopPropagation();
                void this.timeline.edit({type: event.shiftKey ? 'redo' : 'undo'});
            }
            return;
        }
        if ((event.key === 'PageUp' || event.key === 'PageDown') &&
            event.target instanceof Element && event.target.closest('.scroll-lane')) {
            const direction = event.key === 'PageUp' ? 'previous' : 'next';
            if (this.jump(direction, event.shiftKey ? 'bookmark' : 'key')) {
                event.preventDefault();
                event.stopPropagation();
            }
            return;
        }
        const step = this.timeline.snap() ? 10 : 1;
        switch (event.key) {
            case 'ArrowLeft':
            case 'ArrowRight': {
                const delta = event.key === 'ArrowLeft' ? -step : step;
                const key = this.selected();
                if (event.shiftKey && key?.id) {
                    const at = Math.min(this.timeline.duration(), Math.max(0, key.at + delta));
                    void this.edit({kind: 'move_key', key_id: key.id, at});
                } else this.timeline.seek(this.timeline.playhead() + delta);
                break;
            }
            case 'Insert':
                this.insert(false);
                break;
            case 'Delete':
                this.deleteKey();
                break;
            default:
                return;
        }
        event.preventDefault();
        event.stopPropagation();
    }

    private keyValue(): ClipKeyValue | null {
        const track = this.tracks()[this.track()]?.track;
        const values = this.values().map(value => value.trim() ? Number(value) : NaN);
        if (!track || !values.every(Number.isFinite)) {
            this.timeline.error.set('Enter finite component values for this key.');
            return null;
        }
        return track.kind === 'local_position'
            ? {kind: 'position', value: values as unknown as Vec3}
            : {kind: 'rotation', value: values as unknown as Quaternion};
    }

    private seekSelection(tick: number): void {
        this.timeline.stop();
        this.timeline.seek(tick, false);
        const draftId = this.timeline.draft()?.draft_id;
        afterNextRender({
            earlyRead: () => {
                const lane = this.scrollLane()?.nativeElement;
                const cursor = this.rulerCursor()?.nativeElement;
                if (!lane || !cursor || !lane.clientWidth || this.timeline.playhead() !== tick ||
                    this.timeline.draft()?.draft_id !== draftId) return null;
                const left = lane.getBoundingClientRect().left + lane.clientLeft;
                const right = left + lane.clientWidth;
                const marker = cursor.getBoundingClientRect();
                const delta = marker.left < left ? marker.left - left : Math.max(0, marker.right - right);
                return delta ? {lane, offset: lane.scrollLeft + delta} : null;
            },
            write: scroll => {
                if (scroll) scroll.lane.scrollLeft = scroll.offset;
            },
        }, {injector: this.injector});
    }

    private percentAt(tick: number): number {
        return Math.min(100, Math.max(0, tick / Math.max(1, this.timeline.duration()) * 100));
    }

    private edit(edit: TimelineEdit): Promise<void> {
        return this.timeline.edit({type: 'edit', edit});
    }
}
