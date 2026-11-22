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
import type { SceneEntity } from '../../../../core/editor/editor.models';
import type {
	LinkedPrefabOverrideTarget,
	LinkedPrefabStatus,
	LinkedPrefabValue,
	PrefabSourceDraft,
} from '../../../../core/editor/linked-prefabs.models';
import { EditorService } from '../../../../core/editor/editor.service';
import { EditorDialog } from '../../../../shared/components/dialog/dialog';
import { SceneEditingService } from '../scene-editing.service';
import { PrefabSourceEditor } from './prefab-source-editor';
import { ReferenceBindings } from './reference-bindings';
import { readPrefabSource } from './prefab-source-values';
import { PropAssetSelection } from './prop-asset-selection';

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
	protected readonly status = signal<LinkedPrefabStatus | null>(null);
	protected readonly error = signal<string | null>(null);
	protected readonly pending = signal(false);
	protected readonly source = signal<PrefabSourceDraft | null>(null);
	protected readonly updateSource = signal<PrefabSourceDraft | null>(null);
	protected readonly updateOpen = signal(false);
	protected readonly resolution = signal<'reject' | 'keep_local' | 'use_source'>('reject');
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

	protected async openSource(update: boolean): Promise<void> {
		const status = this.status();
		if (!status || !this.available()) return;
		await this.run(async (generation, revision, current) => {
			const draft = await this.editor.openPrefabSource(status.source_path, generation, revision);
			if (!current()) {
				void this.editor.closePrefabSource(draft.draft_id).catch(() => undefined);
				return;
			}
			try {
				readPrefabSource(draft.prefab);
			} catch (error) {
				void this.editor.closePrefabSource(draft.draft_id).catch(() => undefined);
				throw error;
			}
			if (update) {
				this.updateSource.set(draft);
				this.bindings.set(
					Object.fromEntries(
						Object.keys(draft.prefab.reference_slots).map((name) => [
							name,
							Object.hasOwn(status.bindings, name) ? status.bindings[name] : '',
						]),
					),
				);
				this.resolution.set('reject');
				this.updateOpen.set(true);
			} else this.source.set(draft);
		});
	}

	protected closeSource(): void {
		const source = this.source();
		this.source.set(null);
		if (source) void this.editor.closePrefabSource(source.draft_id).catch(() => undefined);
	}
	protected closeUpdate(): void {
		const source = this.updateSource();
		this.updateSource.set(null);
		this.updateOpen.set(false);
		if (source) void this.editor.closePrefabSource(source.draft_id).catch(() => undefined);
	}
	protected chooseResolution(value: string): void {
		if (value === 'reject' || value === 'keep_local' || value === 'use_source')
			this.resolution.set(value);
	}

	protected async update(): Promise<void> {
		const source = this.updateSource(),
			status = this.status();
		if (!source || !status || !this.validBindings()) return;
		await this.run(async (generation, _revision, current) => {
			await this.editor.updateLinkedPrefab(
				status.root_key,
				source.draft_id,
				this.resolution(),
				this.bindings(),
				generation,
				source.revision,
			);
			if (current()) this.closeUpdate();
		});
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
	protected async unpack(): Promise<void> {
		const status = this.status();
		if (!status) return;
		await this.run(async (generation, revision) => {
			await this.editor.execute(
				{ type: 'unpack_linked_prefab', root_key: status.root_key },
				generation,
				revision,
			);
		});
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
