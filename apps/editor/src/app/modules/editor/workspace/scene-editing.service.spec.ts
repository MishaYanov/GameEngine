import {Component, computed, inject, signal} from '@angular/core';
import {ComponentFixture, TestBed} from '@angular/core/testing';
import {vi} from 'vitest';
import type {EditorCommand, EditorSnapshot, SceneDocument, SceneEntity, Vec3} from '../../../core/editor/editor.models';
import {EditorService} from '../../../core/editor/editor.service';
import {ViewportEditorService} from '../../../core/editor/viewport-editor.service';
import {DEFAULT_VIEWPORT_SETTINGS, type ViewportEditorState} from '../../../core/editor/viewport-editor.models';
import {SceneEditingService} from './scene-editing.service';

function deferred() {
    let resolve!: () => void;
    const promise = new Promise<void>(complete => resolve = complete);
    return {promise, resolve};
}

function initialSnapshot(): EditorSnapshot {
    const entities: readonly SceneEntity[] = ['left', 'right'].map(key => ({
        key, name: `${key} robot`, model_path: 'props/robot.glb', renderable: true,
        transform: {position: [0, 0, 0], rotation: [0, 0, 0], scale: [1, 1, 1]},
    }));
    return {
        revision: 1, scene_revision: 1, active_scene_id: 'main', selection: ['left'],
        mode: 'edit', paused: false, dirty: false, project_dirty: false,
        can_undo: false, can_redo: false, transaction: null,
        snap: {enabled: false, translation: 1, rotation_radians: 0.1, scale: 0.1},
        scenes: [{id: 'main', asset_path: 'scenes/main.json', loaded: true, dirty: false}],
        project: {
            schema_version: 3, name: 'Editing fixture',
            scenes: [{id: 'main', asset_path: 'scenes/main.json'}], startup_scene_id: 'main',
            fixed_hz: 60, max_catchup_steps: 8, behaviors: [], behavior_definitions: [],
        },
        document: {
            schema_version: 3, bindings: {}, entities, lights: [],
            camera: {position: [0, 0, 3], yaw: 0, pitch: 0, vertical_fov: 1, near: 0.1, far: 100},
        },
    };
}

/** Minimal transaction-capable bridge; individual requests can stall or fail. */
class EditorBridge {
    readonly nativeAvailable = true;
    readonly workspaceGeneration = signal(1);
    readonly snapshot = signal(initialSnapshot());
    readonly pending = signal(0);
    readonly busy = computed(() => this.pending() > 0);
    readonly commands: EditorCommand[] = [];
    readonly history: SceneDocument[] = [];
    private original: SceneDocument | null = null;
    private gate: {type: EditorCommand['type']; entered: ReturnType<typeof deferred>; released: ReturnType<typeof deferred>} | null = null;
    private failure: {type: EditorCommand['type']; message: string} | null = null;

    holdNext(type: EditorCommand['type']) {
        const gate = {type, entered: deferred(), released: deferred()};
        this.gate = gate;
        return {started: gate.entered.promise, release: gate.released.resolve};
    }

    failNext(type: EditorCommand['type'], message: string): void {
        this.failure = {type, message};
    }

    readonly execute = vi.fn(async (command: EditorCommand, generation?: number, revision?: number): Promise<EditorSnapshot> => {
        this.commands.push(command);
        this.pending.update(count => count + 1);
        try {
            if (this.gate?.type === command.type) {
                const gate = this.gate;
                this.gate = null;
                gate.entered.resolve();
                await gate.released.promise;
            }
            if (generation !== undefined && generation !== this.workspaceGeneration()) throw new Error('The workspace changed.');
            if (revision !== undefined && revision !== this.snapshot().revision) throw new Error('The revision changed.');
            if (this.failure?.type === command.type) {
                const message = this.failure.message;
                this.failure = null;
                throw new Error(message);
            }
            const previous = this.snapshot();
            switch (command.type) {
                case 'begin_transaction':
                    this.original = previous.document;
                    this.snapshot.set({...previous, transaction: command.label});
                    break;
                case 'set_transforms':
                    this.snapshot.set({
                        ...previous, dirty: true,
                        document: {...previous.document, entities: previous.document.entities.map(entity => {
                            const edit = command.edits.find(candidate => candidate.key === entity.key);
                            return edit ? {...entity, transform: edit.transform} : entity;
                        })},
                    });
                    break;
                case 'rename':
                    this.snapshot.set({
                        ...previous, dirty: true,
                        document: {...previous.document, entities: previous.document.entities.map(entity =>
                            entity.key === command.key ? {...entity, name: command.name} : entity)},
                    });
                    break;
                case 'set_collider':
                    if (previous.transaction === null) this.history.push(previous.document);
                    this.snapshot.set({
                        ...previous, dirty: true,
                        document: {...previous.document, entities: previous.document.entities.map(entity =>
                            entity.key === command.key ? {...entity, collider: command.collider} : entity)},
                    });
                    break;
                case 'commit_transaction':
                    if (this.original && previous.document !== this.original) this.history.push(this.original);
                    this.original = null;
                    this.snapshot.set({...previous, transaction: null, can_undo: this.history.length > 0});
                    break;
                case 'cancel_transaction':
                    this.snapshot.set({...previous, document: this.original ?? previous.document, transaction: null, dirty: false});
                    this.original = null;
                    break;
                case 'select':
                    this.snapshot.set({...previous, selection: command.keys});
                    break;
                default:
                    throw new Error(`Unexpected controller command: ${command.type}`);
            }
            return this.snapshot();
        } finally {
            this.pending.update(count => count - 1);
        }
    });

    types(): EditorCommand['type'][] {
        return this.commands.map(command => command.type);
    }

    setVector(field: keyof SceneEntity['transform'], value: Vec3): void {
        this.snapshot.update(snapshot => ({
            ...snapshot,
            document: {
                ...snapshot.document, entities: snapshot.document.entities.map(entity =>
                    entity.key === snapshot.selection[0]
                        ? {...entity, transform: {...entity.transform, [field]: value}}
                        : entity)
            },
        }));
    }
}

@Component({template: '', providers: [SceneEditingService]})
class EditingHost {
    readonly editing = inject(SceneEditingService);
}

describe('SceneEditingService', () => {
    let bridge: EditorBridge;
    let fixture: ComponentFixture<EditingHost>;
    let editing: SceneEditingService;
    let viewportState: ReturnType<typeof signal<ViewportEditorState | null>>;

    beforeEach(async () => {
        bridge = new EditorBridge();
        viewportState = signal<ViewportEditorState | null>(null);
        TestBed.configureTestingModule({providers: [
            {provide: EditorService, useValue: bridge},
            {provide: ViewportEditorService, useValue: {state: viewportState}},
        ]});
        fixture = TestBed.createComponent(EditingHost);
        editing = fixture.componentInstance.editing;
        await fixture.whenStable();
    });

    it('overlays compact native previews without replacing the authored scene snapshot', () => {
        const original = bridge.snapshot();
        const preview = {...original.document.entities[0].transform, position: [9, 2, 3] as const};
        const state: ViewportEditorState = {
            settings: DEFAULT_VIEWPORT_SETTINGS, camera_position: [0, 5, 10], camera_target: [0, 0, 0],
            camera_distance: 10, revision: 4, workspace_generation: 41, gesture: 'transform',
            placement: null, focused: true, error: null,
            transform_preview: {key: 'left', transform: preview},
        };
        viewportState.set(state);
        expect(editing.entity()?.transform.position).toEqual([9, 2, 3]);
        expect(bridge.snapshot()).toBe(original);
        viewportState.set({...state, transform_preview: {key: 'right', transform: preview}});
        expect(editing.entity()?.transform.position).toEqual([0, 0, 0]);
        viewportState.set({...state, gesture: null});
        expect(editing.entity()?.transform.position).toEqual([0, 0, 0]);
        expect(bridge.execute).not.toHaveBeenCalled();
    });

    it('adds, changes and removes collider shapes as atomic edits preserving offsets', async () => {
        expect(await editing.replaceCollider('box')).toBe(true);
        expect(editing.entity()?.collider).toEqual({kind: 'box', half_extents: [0.5, 0.5, 0.5], offset: [0, 0, 0]});
        editing.collider({kind: 'box', half_extents: [1, 2, 3], offset: [4, 5, 6]});
        expect(editing.hasChanges()).toBe(true);
        await editing.commit();
        expect(await editing.replaceCollider('sphere')).toBe(true);
        expect(editing.entity()?.collider).toEqual({kind: 'sphere', radius: 0.5, offset: [4, 5, 6]});
        expect(await editing.replaceCollider('none')).toBe(true);
        expect(editing.entity()?.collider).toBeNull();
        expect(bridge.history).toHaveLength(4);
        expect(bridge.types().filter(type => type === 'begin_transaction')).toHaveLength(1);
    });

    it('coalesces collider previews into one history entry and cancels an in-flight gesture', async () => {
        await editing.replaceCollider('sphere');
        const held = bridge.holdNext('set_collider');
        editing.collider({kind: 'sphere', radius: 1, offset: [0, 0, 0]});
        await held.started;
        for (const radius of [2, 3, 4]) editing.collider({kind: 'sphere', radius, offset: [0, 0, 0]});
        expect(editing.entity()?.collider).toEqual({kind: 'sphere', radius: 4, offset: [0, 0, 0]});
        held.release();
        await editing.commit();
        expect(bridge.commands.filter(command => command.type === 'set_collider')).toHaveLength(3);
        expect(bridge.history).toHaveLength(2);

        const cancellation = bridge.holdNext('set_collider');
        editing.collider({kind: 'sphere', radius: 5, offset: [0, 0, 0]});
        await cancellation.started;
        const cancel = editing.cancel();
        cancellation.release();
        await cancel;
        expect(editing.entity()?.collider).toEqual({kind: 'sphere', radius: 4, offset: [0, 0, 0]});
        expect(bridge.history).toHaveLength(2);
    });

    it('rolls back the complete collider gesture if its final draft is invalid', async () => {
        await editing.replaceCollider('sphere');
        editing.collider({kind: 'sphere', radius: 2, offset: [0, 0, 0]});
        await vi.waitFor(() => expect(bridge.types().filter(type => type === 'set_collider')).toHaveLength(2));
        editing.invalidateCollider();
        await editing.commit();
        expect(editing.entity()?.collider).toEqual({kind: 'sphere', radius: 0.5, offset: [0, 0, 0]});
        expect(bridge.history).toHaveLength(1);
        expect(bridge.types()).not.toContain('commit_transaction');
        expect(bridge.types()).toContain('cancel_transaction');
        expect(editing.error()).toBeNull();
    });

    it.each(['begin_transaction', 'set_collider'] as const)(
        'keeps a late collider %s response from changing a replacement workspace', async type => {
            const held = bridge.holdNext(type);
            editing.collider({kind: 'sphere', radius: 2, offset: [0, 0, 0]});
            await held.started;
            const replacement = initialSnapshot();
            bridge.workspaceGeneration.set(2);
            bridge.snapshot.set(replacement);
            await fixture.whenStable();
            held.release();
            await editing.commit();
            expect(bridge.snapshot()).toBe(replacement);
            expect(bridge.types()).not.toContain('cancel_transaction');
            expect(editing.ownsTransaction()).toBe(false);
            expect(editing.error()).toBeNull();
        },
    );

    it('requeues a corrected valid value when an invalid draft discarded its pending preview', async () => {
        await editing.replaceCollider('sphere');
        const held = bridge.holdNext('set_collider');
        editing.collider({kind: 'sphere', radius: 1, offset: [0, 0, 0]});
        await held.started;
        editing.collider({kind: 'sphere', radius: 2, offset: [0, 0, 0]});
        editing.invalidateCollider();
        editing.collider({kind: 'sphere', radius: 2, offset: [0, 0, 0]});
        held.release();
        await editing.commit();
        expect(bridge.snapshot().document.entities[0].collider).toEqual({kind: 'sphere', radius: 2, offset: [0, 0, 0]});
        expect(bridge.history).toHaveLength(2);
        expect(bridge.types().filter(type => type === 'set_collider')).toHaveLength(3);
    });

    it('cancels collider drafts across scenes that reuse entity keys', async () => {
        const held = bridge.holdNext('set_collider');
        editing.collider({kind: 'sphere', radius: 2, offset: [0, 0, 0]});
        await held.started;
        const replacement = {...initialSnapshot(), active_scene_id: 'second', revision: 2};
        bridge.snapshot.set(replacement);
        await fixture.whenStable();
        held.release();
        await editing.commit();
        expect(bridge.snapshot()).toBe(replacement);
        expect(editing.entity()?.collider).toBeUndefined();
        expect(bridge.snapshot().transaction).toBeNull();
        expect(bridge.history).toHaveLength(0);
        expect(bridge.types()).not.toContain('cancel_transaction');
    });

    it('leaves equal f32 values and invalid collider numbers outside history', async () => {
        await editing.replaceCollider('sphere');
        editing.collider({kind: 'sphere', radius: 0.5 + Number.EPSILON, offset: [0, 0, 0]});
        await editing.commit();
        editing.collider({kind: 'sphere', radius: 1e-50, offset: [0, 0, 0]});
        expect(editing.error()).toContain('positive');
        await editing.commit();
        expect(bridge.types()).toEqual(['set_collider']);
        expect(bridge.history).toHaveLength(1);
        expect(editing.error()).toBeNull();
    });

    it('rebases rapid interaction changes on the latest bits and preserves dimensions through shape changes', async () => {
        await editing.replaceCollider('sphere');
        const held = bridge.holdNext('set_collider');
        const sensor = editing.changeColliderInteraction({kind: 'sensor', sensor: true});
        await held.started;
        const topLayer = editing.changeColliderInteraction({kind: 'layer', field: 'layers', bit: 31, enabled: true});
        const mask = editing.changeColliderInteraction({kind: 'layer', field: 'mask', bit: 0, enabled: false});
        held.release();
        expect(await Promise.all([sensor, topLayer, mask])).toEqual([true, true, true]);
        const interaction = {sensor: true, layers: 0x80000001, mask: 0xfffffffe};
        expect(editing.entity()?.collider).toEqual({kind: 'sphere', radius: 0.5, offset: [0, 0, 0], interaction});
        await editing.replaceCollider('box');
        expect(editing.entity()?.collider?.interaction).toEqual(interaction);
        editing.collider({kind: 'box', half_extents: [1, 2, 3], offset: [4, 5, 6], interaction});
        await editing.commit();
        expect(editing.entity()?.collider).toEqual({kind: 'box', half_extents: [1, 2, 3], offset: [4, 5, 6], interaction});
        expect(bridge.history).toHaveLength(6);
    });

    it('commits a pending dimension gesture before applying an interaction intent', async () => {
        await editing.replaceCollider('sphere');
        const held = bridge.holdNext('set_collider');
        editing.collider({kind: 'sphere', radius: 2, offset: [3, 0, 0]});
        await held.started;
        const filter = editing.changeColliderInteraction({kind: 'sensor', sensor: true});
        held.release();
        expect(await filter).toBe(true);
        expect(editing.entity()?.collider).toEqual({
            kind: 'sphere', radius: 2, offset: [3, 0, 0], interaction: {sensor: true, layers: 1, mask: 0xffffffff},
        });
        expect(bridge.types().slice(-4)).toEqual(['begin_transaction', 'set_collider', 'commit_transaction', 'set_collider']);
        expect(bridge.history).toHaveLength(3);
    });

    it('flushes accepted interaction changes before an immediate Save or Play barrier', async () => {
        await editing.replaceCollider('sphere');
        const filter = editing.changeColliderInteraction({kind: 'sensor', sensor: true});
        const action = vi.fn(async () => {
            expect(editing.entity()?.collider?.interaction?.sensor).toBe(true);
            expect(bridge.snapshot().transaction).toBeNull();
        });
        const barrier = editing.withCommittedEdit(action);
        expect(await filter).toBe(true);
        expect(await barrier).toBe(true);
        expect(action).toHaveBeenCalledOnce();
        expect(bridge.history).toHaveLength(2);
    });

    it('drops queued interaction intents after workspace replacement without applying old errors', async () => {
        await editing.replaceCollider('sphere');
        const held = bridge.holdNext('set_collider');
        const first = editing.changeColliderInteraction({kind: 'sensor', sensor: true});
        await held.started;
        const queued = editing.changeColliderInteraction({kind: 'layer', field: 'layers', bit: 31, enabled: true});
        const action = vi.fn(async () => undefined);
        const barrier = editing.withCommittedEdit(action);
        const replacement = initialSnapshot();
        bridge.workspaceGeneration.set(2);
        bridge.snapshot.set(replacement);
        held.release();
        expect(await Promise.all([first, queued])).toEqual([false, false]);
        expect(await barrier).toBe(false);
        expect(action).not.toHaveBeenCalled();
        expect(bridge.snapshot()).toBe(replacement);
        expect(editing.error()).toBeNull();
    });

    it('keeps interaction settings on cancelled dimension gestures and ignores unchanged defaults', async () => {
        await editing.replaceCollider('sphere');
        await editing.changeColliderInteraction({kind: 'sensor', sensor: false});
        expect(bridge.history).toHaveLength(1);
        await editing.changeColliderInteraction({kind: 'sensor', sensor: true});
        const original = editing.entity()!.collider!;
        editing.collider({...original, offset: [8, 0, 0]});
        await vi.waitFor(() => expect(bridge.snapshot().transaction).not.toBeNull());
        await editing.cancel();
        expect(editing.entity()?.collider).toEqual(original);
        expect(bridge.history).toHaveLength(2);
    });

    it('applies a bulk filter selection as one command and history entry', async () => {
        await editing.replaceCollider('sphere');
        await editing.changeColliderInteraction({kind: 'sensor', sensor: true});
        const original = editing.entity()!.collider!;
        const commandCount = bridge.commands.length;
        const historyCount = bridge.history.length;
        await editing.changeColliderInteraction({kind: 'filter', field: 'mask', value: 0});
        expect(bridge.commands.slice(commandCount)).toHaveLength(1);
        expect(bridge.history).toHaveLength(historyCount + 1);
        expect(bridge.history.at(-1)!.entities[0].collider).toEqual(original);
        expect(editing.entity()?.collider).toEqual({
            ...original, interaction: {...original.interaction!, mask: 0},
        });
    });

    it('coalesces pending previews to the latest value and commits one history entry', async () => {
        const held = bridge.holdNext('set_transforms');
        editing.transform({field: 'position', axis: 0, value: 1});
        await held.started;
        editing.transform({field: 'position', axis: 0, value: 2});
        editing.transform({field: 'position', axis: 0, value: 3});
        editing.transform({field: 'position', axis: 0, value: 4});
        expect(editing.entity()?.transform.position[0]).toBe(4);

        const completion = editing.commit();
        held.release();
        expect(await completion).toBe(true);
        expect(bridge.types()).toEqual(['begin_transaction', 'set_transforms', 'set_transforms', 'commit_transaction']);
        expect(bridge.snapshot().document.entities[0].transform.position[0]).toBe(4);
        expect(bridge.history).toHaveLength(1);
        expect(bridge.snapshot().transaction).toBeNull();
    });

    it.each(['position', 'rotation'] as const)('applies the linked %s offset across the original vector', async field => {
        bridge.setVector(field, [1, 2, 3]);
        editing.transform({field, axis: 1, value: 5, linked: true});
        expect(editing.entity()?.transform[field]).toEqual([4, 5, 6]);
        expect(await editing.commit()).toBe(true);
        expect(bridge.snapshot().document.entities[0].transform[field]).toEqual([4, 5, 6]);
        expect(bridge.history).toHaveLength(1);
    });

    it('retains signed scale ratios through coalesced edits and an intermediate zero', async () => {
        bridge.setVector('scale', [1, 2, -3]);
        const held = bridge.holdNext('set_transforms');
        editing.transform({field: 'scale', axis: 0, value: 4, linked: true});
        await held.started;
        expect(editing.entity()?.transform.scale).toEqual([4, 8, -12]);
        editing.transform({field: 'scale', axis: 0, value: 0, linked: true});
        editing.transform({field: 'scale', axis: 0, value: 2, linked: true});
        expect(editing.entity()?.transform.scale).toEqual([2, 4, -6]);

        const completion = editing.commit();
        held.release();
        expect(await completion).toBe(true);
        expect(bridge.types()).toEqual(['begin_transaction', 'set_transforms', 'set_transforms', 'commit_transaction']);
        expect(bridge.snapshot().document.entities[0].transform.scale).toEqual([2, 4, -6]);
        expect(bridge.history).toHaveLength(1);
    });

    it('lets a linked scale recover from all-zero while guarding zero axes and overflow', async () => {
        bridge.setVector('scale', [0, 2, 3]);
        editing.transform({field: 'scale', axis: 0, value: 2, linked: true});
        expect(editing.error()).toContain('Unlink Scale');
        editing.transform({field: 'scale', axis: 1, value: 3e38, linked: true});
        expect(editing.error()).toContain('finite 32-bit');
        expect(await editing.cancel()).toBe(true);
        expect(bridge.types()).toEqual([]);

        bridge.setVector('scale', [0, 0, 0]);
        editing.transform({field: 'scale', axis: 0, value: 2, linked: true});
        expect(await editing.commit()).toBe(true);
        expect(bridge.snapshot().document.entities[0].transform.scale).toEqual([2, 2, 2]);
    });

    it('cancels an edit even when the transaction begin request is still pending', async () => {
        const held = bridge.holdNext('begin_transaction');
        editing.transform({field: 'position', axis: 0, value: 5});
        await held.started;
        const cancellation = editing.cancel();
        held.release();

        expect(await cancellation).toBe(true);
        expect(bridge.types()).toEqual(['begin_transaction', 'cancel_transaction']);
        expect(editing.entity()?.transform.position[0]).toBe(0);
        expect(editing.ownsTransaction()).toBe(false);
        expect(bridge.history).toHaveLength(0);
    });

    it('commits the previous field before beginning another field transaction', async () => {
        editing.transform({field: 'position', axis: 0, value: 2});
        editing.transform({field: 'scale', axis: 1, value: 3});

        expect(await editing.commit()).toBe(true);
        expect(bridge.types()).toEqual([
            'begin_transaction', 'set_transforms', 'commit_transaction',
            'begin_transaction', 'set_transforms', 'commit_transaction',
        ]);
        expect(bridge.snapshot().document.entities[0].transform.position[0]).toBe(2);
        expect(bridge.snapshot().document.entities[0].transform.scale[1]).toBe(3);
        expect(bridge.history).toHaveLength(2);
    });

    it('finishes the pending edit before changing the selected entity', async () => {
        const held = bridge.holdNext('set_transforms');
        editing.transform({field: 'position', axis: 0, value: 7});
        await held.started;
        const selection = editing.select('right');
        expect(bridge.types()).not.toContain('select');
        held.release();
        await selection;

        expect(bridge.types()).toEqual(['begin_transaction', 'set_transforms', 'commit_transaction', 'select']);
        expect(bridge.snapshot().selection).toEqual(['right']);
        expect(editing.entity()?.key).toBe('right');
        expect(bridge.snapshot().document.entities[0].transform.position[0]).toBe(7);
    });

    it('reserves the edit surface until the following Save or Play action finishes', async () => {
        const preview = bridge.holdNext('set_transforms');
        const entered = deferred();
        const released = deferred();
        const action = vi.fn(async () => {
            entered.resolve();
            await released.promise;
        });
        editing.transform({field: 'position', axis: 0, value: 4});
        await preview.started;
        const completion = editing.withCommittedEdit(action);
        expect(editing.editable()).toBe(false);
        editing.rename('Must not start while committing');
        preview.release();
        await entered.promise;
        expect(bridge.snapshot().transaction).toBeNull();
        expect(editing.editable()).toBe(false);
        editing.transform({field: 'scale', axis: 0, value: 10});
        expect(bridge.types()).toEqual(['begin_transaction', 'set_transforms', 'commit_transaction']);

        released.resolve();
        expect(await completion).toBe(true);
        expect(action).toHaveBeenCalledOnce();
        expect(editing.editable()).toBe(true);
        expect(editing.entity()?.name).toBe('left robot');
        expect(editing.entity()?.transform.scale).toEqual([1, 1, 1]);
    });

    it('rolls back a rejected preview and exposes the backend error', async () => {
        bridge.failNext('set_transforms', 'Scale cannot be zero.');
        editing.transform({field: 'scale', axis: 1, value: 0});

        expect(await editing.commit()).toBe(false);
        expect(bridge.types()).toEqual(['begin_transaction', 'set_transforms', 'cancel_transaction']);
        expect(editing.error()).toBe('Scale cannot be zero.');
        expect(editing.entity()?.transform.scale).toEqual([1, 1, 1]);
        expect(editing.ownsTransaction()).toBe(false);
        expect(bridge.history).toHaveLength(0);
    });

    it('cancels a transaction accepted before its begin response failed', async () => {
        const execute = bridge.execute.getMockImplementation()!;
        bridge.execute.mockImplementationOnce(async command => {
            await execute(command);
            throw new Error('The transaction response could not be decoded.');
        });
        editing.transform({field: 'position', axis: 0, value: 5});
        expect(await editing.commit()).toBe(false);
        expect(bridge.types()).toEqual(['begin_transaction', 'cancel_transaction']);
        expect(bridge.snapshot().transaction).toBeNull();
        expect(editing.ownsTransaction()).toBe(false);
    });

    it('rejects queued fields after cancellation fails and allows cancellation to be retried', async () => {
        const held = bridge.holdNext('set_transforms');
        editing.transform({field: 'position', axis: 0, value: 2});
        await held.started;
        bridge.failNext('cancel_transaction', 'Temporary cancellation failure');
        const cancellation = editing.cancel();
        editing.transform({field: 'scale', axis: 0, value: 7});
        held.release();
        expect(await cancellation).toBe(false);
        await editing.commit();
        expect(bridge.types().filter(type => type === 'set_transforms')).toHaveLength(1);
        expect(editing.editable()).toBe(false);
        expect(await editing.cancel()).toBe(true);
        expect(bridge.snapshot().transaction).toBeNull();
        expect(editing.editable()).toBe(true);
        expect(bridge.snapshot().document.entities[0].transform.scale).toEqual([1, 1, 1]);
    });

    it('blocks authoring input in Play mode', async () => {
        bridge.snapshot.update(snapshot => ({...snapshot, mode: 'play'}));
        editing.rename('Changed');
        editing.transform({field: 'position', axis: 0, value: 8});
        await editing.commit();

        expect(editing.editable()).toBe(false);
        expect(bridge.execute).not.toHaveBeenCalled();
        expect(editing.entity()?.name).toBe('left robot');
        expect(editing.entity()?.transform.position).toEqual([0, 0, 0]);
    });

    it('cancels its transaction when the selected entity changes externally', async () => {
        const held = bridge.holdNext('set_transforms');
        editing.transform({field: 'position', axis: 0, value: 9});
        await held.started;
        bridge.snapshot.update(snapshot => ({...snapshot, selection: ['right']}));
        await fixture.whenStable();
        held.release();
        await editing.commit();

        expect(bridge.types()).toEqual(['begin_transaction', 'set_transforms', 'cancel_transaction']);
        expect(bridge.snapshot().selection).toEqual(['right']);
        expect(bridge.snapshot().document.entities[0].transform.position[0]).toBe(0);
        expect(editing.entity()?.key).toBe('right');
        expect(editing.ownsTransaction()).toBe(false);
    });

    it('rolls back pending edits when its workspace is destroyed', async () => {
        const held = bridge.holdNext('set_transforms');
        editing.transform({field: 'position', axis: 0, value: 6});
        await held.started;
        fixture.destroy();
        held.release();
        await editing.commit();

        expect(bridge.types()).toEqual(['begin_transaction', 'set_transforms', 'cancel_transaction']);
        expect(bridge.snapshot().document.entities[0].transform.position[0]).toBe(0);
        expect(bridge.snapshot().transaction).toBeNull();
        expect(bridge.history).toHaveLength(0);
    });

    it('keeps invalid names and overflowing numbers out of native commands', async () => {
        editing.transform({field: 'position', axis: 0, value: Number.MAX_VALUE});
        expect(editing.error()).toContain('finite 32-bit');
        expect(bridge.execute).not.toHaveBeenCalled();

        editing.rename('   ');
        expect(editing.entity()?.name).toBe('   ');
        expect(editing.error()).toContain('visible name');
        await editing.commit();
        expect(bridge.execute).not.toHaveBeenCalled();
        expect(editing.entity()?.name).toBe('left robot');

        editing.rename('Valid name');
        await editing.commit();
        expect(bridge.snapshot().document.entities[0].name).toBe('Valid name');
        expect(editing.error()).toBeNull();
        expect(bridge.history).toHaveLength(1);
    });
});
