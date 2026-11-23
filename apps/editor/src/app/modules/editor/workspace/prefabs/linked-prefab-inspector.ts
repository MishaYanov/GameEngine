import {
	Component,
	computed,
	DestroyRef,
	effect,
	inject,
	input,
	signal,
	untracked,
} from '@angular/core';
import type { EntityKey, SceneEntity } from '../../../../core/editor/editor.models';
import type {
	LinkedPrefabConflictChoice,
	LinkedPrefabOverride,
	LinkedPrefabOverrideResolution,
	LinkedPrefabOverrideReview,
	LinkedPrefabOverrideTarget,
	LinkedPrefabRecoveryReview,
	LinkedPrefabStatus,
	LinkedPrefabValue,
	PrefabSourceDraft,
	PropAddress,
} from '../../../../core/editor/linked-prefabs.models';
import { EditorService } from '../../../../core/editor/editor.service';
import { EditorDialog } from '../../../../shared/components/dialog/dialog';
import { SceneEditingService } from '../scene-editing.service';
import { PrefabSourceEditor } from './prefab-source-editor';
import { ReferenceBindings } from './reference-bindings';
import { readPrefabSource } from './prefab-source-values';
import { PropAssetSelection } from './prop-asset-selection';
import { PropWorkspaceService } from '../prop-workspace/prop-workspace.service';

interface OverrideReview {
	readonly draftId: string;
	readonly revision: number;
	readonly bindings: Readonly<Record<string, EntityKey>>;
	readonly rows: readonly LinkedPrefabOverrideReview[];
}

interface OverrideApply {
	readonly root: EntityKey;
	readonly assetId: string;
	readonly path: string;
	readonly revision: number;
	readonly change: LinkedPrefabOverride;
}

interface RecoveryCapture {
	readonly draft: PrefabSourceDraft;
	readonly review: LinkedPrefabRecoveryReview;
	readonly revision: number;
}

@Component({
	selector: 'editor-linked-prefab-inspector',
	imports: [EditorDialog, PrefabSourceEditor, ReferenceBindings],
	templateUrl: './linked-prefab-inspector.html',
	styleUrls: ['../game-settings/settings.scss', './linked-prefab-inspector.scss'],
})
export class LinkedPrefabInspector {
	readonly entity = input<SceneEntity | null>(null);
	readonly editable = input(false);
	private readonly editor = inject(EditorService);
	private readonly editing = inject(SceneEditingService);
	private readonly propAssets = inject(PropAssetSelection, { optional: true });
	protected readonly propWorkspace = inject(PropWorkspaceService, { optional: true });
	protected readonly status = signal<LinkedPrefabStatus | null>(null);
	protected readonly error = signal<string | null>(null);
	protected readonly pending = signal(false);
	protected readonly source = signal<PrefabSourceDraft | null>(null);
	protected readonly updateSource = signal<PrefabSourceDraft | null>(null);
	protected readonly updateOpen = signal(false);
	protected readonly applyOpen = signal(false);
	protected readonly applyRequest = signal<OverrideApply | null>(null);
	protected readonly recoveryOpen = signal(false);
	protected readonly recoveryAssetId = signal('');
	protected readonly recoveryAssets = computed(() => (this.editor.modelAssets() ?? [])
		.filter(asset => asset.kind === 'prefab' && asset.asset_id && asset.fingerprint));
	private readonly recoveryCapture = signal<RecoveryCapture | null>(null);
	protected readonly recoveryReview = computed(() => {
		const capture = this.recoveryCapture();
		const asset = this.recoveryAssets().find(candidate => candidate.asset_id === this.recoveryAssetId());
		const status = this.status();
		return capture && status?.source_error && status.root_key === capture.review.root_key &&
			status.source_fingerprint === capture.review.prior_fingerprint &&
			asset?.asset_id === capture.draft.asset_id && asset.fingerprint === capture.draft.fingerprint &&
			asset.path === capture.draft.path && capture.revision === this.editor.snapshot()?.revision
			? capture.review : null;
	});
	protected readonly savedMessage = signal<string | null>(null);
	private readonly review = signal<OverrideReview | null>(null);
	protected readonly choices = signal<ReadonlyMap<string, LinkedPrefabConflictChoice>>(new Map());
	protected readonly reviewRows = computed(() => {
		const review = this.review();
		return review && review.draftId === this.updateSource()?.draft_id
			&& review.revision === this.editor.snapshot()?.revision && review.bindings === this.bindings()
			? review.rows : null;
	});
	protected readonly updateReady = computed(() => {
		const rows = this.reviewRows();
		return this.available() && this.validBindings() && rows !== null
			&& rows.every(row => !row.conflict || this.choices().has(this.overrideKey(row)));
	});
	protected readonly bindings = signal<Readonly<Record<string, string>>>({});
	protected readonly names = computed(() =>
		Object.keys(this.updateSource()?.prefab.reference_slots ?? {}),
	);
	protected readonly entities = computed(() => this.editor.snapshot()?.document.entities ?? []);
    protected readonly boundSlots = computed(() => {
        const names = new Map(this.entities().map(entity => [entity.key, entity.name]));
        const bindings = this.entity()?.linked_prefab?.bindings ?? this.status()?.bindings ?? {};
        return Object.entries(bindings).map(([slot, key]) => ({
            slot,
            target: names.get(key) ?? `Missing entity: ${key}`
        }));
    });
	protected readonly validBindings = computed(() =>
		this.names().every(
			(name) =>
				Object.hasOwn(this.bindings(), name) &&
				this.entities().some((entity) => entity.key === this.bindings()[name]),
		),
	);
	protected readonly available = computed(
		() => this.editable() && !this.pending() && !!this.status(),
	);
	protected readonly canSelectPropAsset = computed(() => {
		const status = this.status();
		return !!this.propAssets && !!status && !status.source_error && !this.pending();
	});
	private readonly context = computed(() =>
		JSON.stringify([
			this.editor.workspaceGeneration(),
			this.editor.snapshot()?.active_scene_id,
			this.entity()?.key,
			this.editor.snapshot()?.mode,
		]),
	);
	private readonly inspectionContext = computed(() =>
		JSON.stringify([
			this.context(),
			this.editor.snapshot()?.revision,
			this.editor.snapshot()?.transaction,
		]),
	);
	private request = 0;
	private destroyed = false;
	private observedContext = '';

	constructor() {
		effect(() => {
			const context = this.context();
			this.inspectionContext();
			this.editor.modelAssets();
			untracked(() => {
				const snapshot = this.editor.snapshot();
				if (context !== this.observedContext) {
					this.observedContext = context;
					this.closeSource();
					this.closeUpdate();
					this.closeApply();
					this.closeRecovery();
					this.savedMessage.set(null);
					this.status.set(null);
					this.error.set(null);
					this.request++;
				}
				if (!snapshot || snapshot.mode !== 'edit' || !this.entity()) {
					this.status.set(null);
					return;
				}
				if (!snapshot.transaction) void this.refresh(context);
			});
		});
		inject(DestroyRef).onDestroy(() => {
			this.destroyed = true;
			this.request++;
			this.closeSource();
			this.closeUpdate();
			this.closeApply();
			this.closeRecovery();
		});
	}

	private async refresh(context = this.context()): Promise<void> {
		const key = this.entity()?.key;
		if (!key) return;
		const request = ++this.request,
			generation = this.editor.workspaceGeneration();
		try {
			const status = await this.editor.linkedPrefabStatus(key, generation);
			if (!this.destroyed && context === this.context() && request === this.request)
				this.status.set(status);
		} catch (error) {
			if (!this.destroyed && context === this.context() && request === this.request)
				this.error.set(error instanceof Error ? error.message : String(error));
		}
	}

	protected selectPropAsset(): void {
		const status = this.status();
		const snapshot = this.editor.snapshot();
		if (!this.canSelectPropAsset() || !status || snapshot?.mode !== 'edit') return;
		this.propAssets?.reveal({
			sourceId: status.source_id,
			workspaceGeneration: this.editor.workspaceGeneration(),
			sceneId: snapshot.active_scene_id,
		});
	}

	protected async openPropWorkspace(): Promise<void> {
		const prop = this.propWorkspace, entity = this.entity(), context = this.context();
		if (!prop || !entity?.linked_prefab || !this.editable() || !prop.canOpen()) return;
		if (!await prop.openInstance(entity.key) && context === this.context()) this.error.set(prop.error());
	}

	protected async openSource(update: boolean): Promise<void> {
		const status = this.status();
		if (!status || status.source_error) return;
		await this.run(async (generation, revision, current) => {
			const draft = await this.openReviewedSource(status.source_path, status.source_id, generation, revision, current);
			if (!draft) return;
			if (update) {
				this.updateSource.set(draft);
				this.setBindings(this.bindingsFor(draft, status.bindings));
				this.updateOpen.set(true);
				if (this.validBindings()) await this.loadReview(status.root_key, draft, generation, revision, current);
			} else {
				this.source.set(draft);
			}
		});
	}

	protected closeSource(): void {
		const source = this.source();
		this.source.set(null);
		if (source) this.releaseDraft(source.draft_id);
	}

	protected closeUpdate(): void {
		const source = this.updateSource();
		this.updateSource.set(null);
		this.updateOpen.set(false);
		this.review.set(null);
		this.choices.set(new Map());
		if (source) this.releaseDraft(source.draft_id);
	}

	protected setBindings(bindings: Readonly<Record<string, EntityKey>>): void {
		this.bindings.set(bindings);
		this.review.set(null);
		this.choices.set(new Map());
	}

	protected chooseResolution(row: LinkedPrefabOverrideReview, value: string): void {
		this.choices.update(prior => {
			const next = new Map(prior);
			const key = this.overrideKey(row);
			if (value === 'use_source' || (value === 'keep_local' && row.incoming !== null)) next.set(key, value);
			else next.delete(key);
			return next;
		});
	}

	protected async reviewChanges(): Promise<void> {
		const status = this.status(), previous = this.updateSource();
		if (!status || status.source_error || !previous || !this.validBindings()) return;
		await this.run(async (generation, revision, current) => {
			this.review.set(null);
			this.choices.set(new Map());
			const draft = await this.openReviewedSource(status.source_path, status.source_id, generation, revision, current);
			if (!draft) return;
			this.updateSource.set(draft);
			if (previous.draft_id !== draft.draft_id) this.releaseDraft(previous.draft_id);
			this.setBindings(this.bindingsFor(draft, this.bindings()));
			if (this.validBindings()) await this.loadReview(status.root_key, draft, generation, revision, current);
		});
	}

	protected async update(): Promise<void> {
		const status = this.status(), review = this.review(), rows = this.reviewRows();
		if (!status || !review || !rows || !this.updateReady()) return;
		const resolutions: LinkedPrefabOverrideResolution[] = [];
		for (const row of rows) {
			const resolution = this.choices().get(this.overrideKey(row));
			if (row.conflict && resolution) resolutions.push({ address: row.address, property: row.target.property, resolution });
		}
		await this.run(async (generation, revision, current) => {
			if (revision !== review.revision) throw new Error('The scene changed. Review the linked Prop again.');
			await this.editor.updateLinkedPropReviewed(
				status.root_key, review.draftId, review.bindings, resolutions, generation, revision,
			);
			if (current()) this.closeUpdate();
		});
	}

	protected confirmApply(change: LinkedPrefabOverride): void {
		const status = this.status(), revision = this.editor.snapshot()?.revision;
		if (!status || status.source_error || revision === undefined || !change.can_apply_to_root_source || !this.available()) return;
		this.applyRequest.set({ root: status.root_key, assetId: status.source_id, path: status.source_path, revision, change });
		this.applyOpen.set(true);
	}

	protected closeApply(): void {
		this.applyOpen.set(false);
		this.applyRequest.set(null);
	}

	protected async apply(): Promise<void> {
		const request = this.applyRequest();
		if (!request) return;
		await this.run(async (generation, revision, current) => {
			if (request.revision !== revision) throw new Error('The scene changed. Select the override and review it again.');
			const draft = await this.openReviewedSource(request.path, request.assetId, generation, revision, current);
			if (!draft) return;
			try {
				const result = await this.editor.applyLinkedPropOverride(
					request.root, draft.draft_id, request.change.address, request.change.target.property, generation, revision,
				);
				if (!current()) return;
				this.closeApply();
				this.savedMessage.set(`Saved to ${result.draft.path}. Review Source Update to update this instance.`);
				await this.editor.assets(true);
			} finally {
				this.releaseDraft(draft.draft_id);
			}
		});
	}

	private async openReviewedSource(
		path: string, assetId: string, generation: number, revision: number, current: () => boolean,
	): Promise<PrefabSourceDraft | null> {
		const draft = await this.editor.openPrefabSource(path, generation, revision);
		if (!current()) {
			this.releaseDraft(draft.draft_id);
			return null;
		}
		try {
			if (draft.asset_id !== assetId) throw new Error('The Prop asset changed. Select its source again.');
			readPrefabSource(draft.prefab);
			return draft;
		} catch (error) {
			this.releaseDraft(draft.draft_id);
			throw error;
		}
	}

	private releaseDraft(draftId: string): void {
		void this.editor.closePrefabSource(draftId).catch(() => undefined);
	}

	private bindingsFor(draft: PrefabSourceDraft, bindings: Readonly<Record<string, EntityKey>>): Readonly<Record<string, EntityKey>> {
		return Object.fromEntries(
			Object.keys(draft.prefab.reference_slots).map(name => [name, Object.hasOwn(bindings, name) ? bindings[name] : '']),
		);
	}

	private async loadReview(
		root: EntityKey, draft: PrefabSourceDraft, generation: number, revision: number, current: () => boolean,
	): Promise<void> {
		const bindings = this.bindings();
		const rows = await this.editor.reviewLinkedPropOverrides(root, draft.draft_id, bindings, generation, revision);
		if (current() && this.updateSource()?.draft_id === draft.draft_id) {
			this.review.set({ draftId: draft.draft_id, revision, bindings, rows });
		}
	}

	protected async revert(property: LinkedPrefabOverrideTarget | null): Promise<void> {
		const status = this.status();
		if (!status) return;
		await this.run(async (generation, revision) => {
			await this.editor.execute(
				{ type: 'revert_linked_prefab', root_key: status.root_key, property },
				generation,
				revision,
			);
		});
	}
	protected async duplicate(): Promise<void> {
		const status = this.status();
		if (!status || status.direct_placements === undefined) return;
		await this.run(async (generation, revision) => {
			await this.editor.execute(
				{ type: 'duplicate_linked_prefab', root_key: status.root_key }, generation, revision,
			);
		});
	}

	protected async unpack(outer = false): Promise<void> {
		const status = this.status();
		if (!status || (outer && !status.direct_placements)) return;
		await this.run(async (generation, revision) => {
			await this.editor.execute(
				{ type: outer ? 'unpack_linked_prefab_outer' : 'unpack_linked_prefab', root_key: status.root_key },
				generation,
				revision,
			);
		});
	}

	protected beginRecovery(): void {
		const status = this.status();
		if (!this.available() || !status?.source_error || status.direct_placements === undefined) return;
		this.recoveryOpen.set(true);
		void this.run(async () => { await this.editor.assets(true); });
	}

	protected closeRecovery(): void {
		this.recoveryOpen.set(false);
		this.selectRecoveryAsset('');
	}

	protected selectRecoveryAsset(assetId: string): void {
		this.recoveryAssetId.set(assetId);
		this.releaseRecoveryCapture();
	}

	private releaseRecoveryCapture(): void {
		const capture = this.recoveryCapture();
		this.recoveryCapture.set(null);
		if (capture) this.releaseDraft(capture.draft.draft_id);
	}

	protected async reviewRecovery(): Promise<void> {
		const status = this.status();
		const asset = this.recoveryAssets().find(candidate => candidate.asset_id === this.recoveryAssetId());
		if (!status?.source_error || !asset?.asset_id) return;
		const assetId = asset.asset_id;
		await this.run(async (generation, revision, current) => {
			this.releaseRecoveryCapture();
			const draft = await this.openReviewedSource(asset.path, assetId, generation, revision, current);
			if (!draft) return;
			try {
				if (draft.fingerprint !== asset.fingerprint) throw new Error('The replacement changed. Refresh Assets and review the current version.');
				const review = await this.editor.reviewLinkedPropRecovery(status.root_key, draft.path, draft.draft_id, generation, revision);
				if (!current() || this.recoveryAssetId() !== assetId) return;
				if (review.root_key !== status.root_key || review.prior_source_id !== status.source_id ||
					review.prior_fingerprint !== status.source_fingerprint || review.source_id !== draft.asset_id ||
					review.source_path !== draft.path || review.source_fingerprint !== draft.fingerprint) {
					throw new Error('The editor returned a different reviewed replacement.');
				}
				this.recoveryCapture.set({draft, review, revision});
			} finally {
				if (this.recoveryCapture()?.draft.draft_id !== draft.draft_id) this.releaseDraft(draft.draft_id);
			}
		});
	}

	protected async recover(): Promise<void> {
		const capture = this.recoveryCapture();
		const review = this.recoveryReview();
		if (!capture || !review) return;
		await this.run(async (generation, revision, current) => {
			if (capture.revision !== revision) throw new Error('The scene changed. Review the replacement again.');
			await this.editor.recoverLinkedProp(review.root_key, capture.draft.path, capture.draft.draft_id, generation, revision);
			if (!current()) return;
			this.closeRecovery();
			this.savedMessage.set(`Recovered the source link to ${review.source_path}.`);
		});
	}

	protected overrideKey(row: Pick<LinkedPrefabOverride, 'address' | 'target'>): string {
		return JSON.stringify([row.address, row.target.property]);
	}

	protected addressLabel(address: PropAddress): string {
		const root = this.entities().find(entity => entity.key === this.status()?.root_key);
		const prefab = root?.linked_prefab?.source.prefab;
		const composition = prefab?.composition;
		let placements = composition?.placements;
		const labels = address.placement_path.map(id => {
			const placement = placements?.find(entry => entry.id === id);
			placements = placement && composition?.pins.find(pin => pin.version.asset_id === placement.target.asset_id
				&& pin.version.fingerprint === placement.target.fingerprint)?.placements;
			return placement?.name ?? `Placement ${id}`;
		});
		if (address.kind === 'object') {
			const object = address.placement_path.length === 0 ? prefab?.entities.find(entity => entity.key === address.source_entity) : null;
			labels.push(object?.name ?? `Source object ${address.source_entity}`);
		}
		return labels.join(' › ');
	}

	protected property(target: LinkedPrefabOverrideTarget): string {
		return {
			name: 'Name',
			transform_position: 'Position',
			transform_rotation: 'Rotation',
			transform_scale: 'Scale',
			mesh_renderer: 'Mesh Renderer',
			collider: 'Collider',
			character_controller: 'Character Controller',
			animation: 'Animation',
			native_behavior: 'Native Behavior',
			script: 'Script',
            particle_emitter: 'Particle Emitter',
		}[target.property.kind];
	}
	protected value(value: LinkedPrefabValue): string {
		if (value.kind === 'name') return value.value;
		if (value.kind === 'vector') return value.value.join(', ');
		const component = value.value;
		if (!component) return 'Component removed';
		const data = component.data;
		const enabled = component.enabled ? 'Enabled' : 'Disabled';
		if (data.kind === 'mesh_renderer')
			return `${enabled} · ${data.model_path} · Mesh ${data.mesh_index ?? 'default'}${data.material ? ' · Color ' + data.material.base_color_factor.join(', ') : ''}`;
		if (data.kind === 'collider')
			return `${enabled} · ${data.collider.kind === 'box' ? 'Box ' + data.collider.half_extents.join(', ') : 'Sphere radius ' + data.collider.radius} · Offset ${data.collider.offset.join(', ')}`;
		if (data.kind === 'character_controller')
			return `${enabled} · Walk ${data.settings.walk_speed} · Jump ${data.settings.jump_action}`;
		if (data.kind === 'animation')
			return `${enabled} · ${data.settings.playback.kind.replaceAll('_', ' ')} · ${data.settings.source.asset_id}`;
        if (data.kind === 'interactable') return [enabled, data.settings.action, data.settings.reach + ' m'].join(' · ');
        if (data.kind === 'camera') return `${enabled} · ${data.settings.rig.mode} Camera`;
        if (data.kind === 'player_controller') return `${enabled} · Player Controller`;
        if (data.kind === 'audio_source') return `${enabled} · ${data.settings.clip?.asset ?? 'No Audio Clip'}`;
        if (data.kind === 'animator') return `${enabled} · Animator ${data.settings.asset_id}`;
        if (data.kind === 'sky') return `${enabled} · Sky`;
        if (data.kind === 'particle_emitter') return `${enabled} · ${data.settings.rate_per_second} / s`;
		if (data.kind === 'script') return `${enabled} · ${data.settings.script_id}`;
		return `${enabled} · ${data.behavior.kind.replaceAll('_', ' ')} · ${JSON.stringify(data.behavior)}`;
	}

	private async run(
		action: (generation: number, revision: number, current: () => boolean) => Promise<void>,
	): Promise<void> {
		if (!this.available()) return;
		const context = this.context(),
			generation = this.editor.workspaceGeneration();
		const current = () => !this.destroyed && this.context() === context;
		this.pending.set(true);
		this.error.set(null);
		try {
			const accepted = await this.editing.withCommittedEdit(async () => {
				if (!current()) return;
				try {
					await action(generation, this.editor.snapshot()!.revision, current);
				} catch (error) {
					if (current()) this.error.set(error instanceof Error ? error.message : String(error));
				}
			});
			if (!accepted && current())
				this.error.set(this.editing.error() ?? 'Finish the current edit and try again.');
			if (current()) await this.refresh();
		} catch (error) {
			if (current()) this.error.set(error instanceof Error ? error.message : String(error));
		} finally {
			this.pending.set(false);
		}
	}
}
