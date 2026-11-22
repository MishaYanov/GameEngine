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
import type { EntityKey, ModelAsset, SceneEntity, Vec3 } from '../../../../core/editor/editor.models';
import type { PrefabSourceDraft } from '../../../../core/editor/linked-prefabs.models';
import { EditorService } from '../../../../core/editor/editor.service';
import { EditorDialog } from '../../../../shared/components/dialog/dialog';
import {EditorSearchPicker} from '../../../../shared/components/search-picker/search-picker';
import {Vector3Field, type Vector3Change} from '../../../../shared/components/vector3-field/vector3-field';
import { SceneEditingService } from '../scene-editing.service';
import {sceneReferenceOptions} from '../scene-reference-options';
import { relativeAssetPath } from '../components/component-draft';
import { PrefabSourceEditor } from './prefab-source-editor';
import { readPrefabSource } from './prefab-source-values';
import { ReferenceBindings } from './reference-bindings';

interface PropCreationReview {
	readonly revision: number;
	readonly selection: readonly EntityKey[];
	readonly roots: readonly SceneEntity[];
	readonly members: readonly SceneEntity[];
	readonly memberKeys: ReadonlySet<EntityKey>;
}

const MAX_PROP_MEMBERS = 4096;

@Component({
	selector: 'editor-prefab-authoring',
    imports: [EditorDialog, EditorSearchPicker, PrefabSourceEditor, ReferenceBindings, Vector3Field],
	templateUrl: './prefab-authoring.html',
	styleUrl: '../game-settings/settings.scss',
})
export class PrefabAuthoring {
	readonly selected = input<ModelAsset | null>(null);
	readonly opening = input(false);
	private readonly editor = inject(EditorService);
	private readonly editing = inject(SceneEditingService, { optional: true });
	protected readonly supported = computed(
		() => (this.editor.capabilities()?.component_types?.length ?? 0) > 0,
	);
	protected readonly pending = signal(false);
	protected readonly error = signal<string | null>(null);
	protected readonly message = signal<string | null>(null);
	protected readonly source = signal<PrefabSourceDraft | null>(null);
	protected readonly link = signal<PrefabSourceDraft | null>(null);
	protected readonly linkOpen = signal(false);
	protected readonly createOpen = signal(false);
	protected readonly createPath = signal('prop.json');
	protected readonly createReview = signal<PropCreationReview | null>(null);
	protected readonly createPivot = signal<EntityKey | ''>('');
	protected readonly createSlots = signal<readonly { name: string; target: string }[]>([]);
	protected readonly linkPosition = signal<Vec3>([0, 0, 0]);
	protected readonly bindings = signal<Readonly<Record<string, string>>>({});
	protected readonly entities = computed(() => this.editor.snapshot()?.document.entities ?? []);
	protected readonly pivotOptions = computed(() => sceneReferenceOptions(this.createReview()?.members ?? []));
	protected readonly externalOptions = computed(() => {
		const keys = this.createReview()?.memberKeys;
		return sceneReferenceOptions(this.entities().filter(entity => !keys?.has(entity.key)));
	});
	protected readonly slotNames = computed(() =>
		Object.keys(this.link()?.prefab.reference_slots ?? {}),
	);
	protected readonly canLink = computed(() =>
		this.slotNames().every(
			(name) =>
				Object.hasOwn(this.bindings(), name) &&
				this.entities().some((entity) => entity.key === this.bindings()[name]),
		),
	);
	protected readonly available = computed(
		() =>
			this.supported() &&
			!this.opening() &&
			!this.pending() &&
			this.editor.snapshot()?.mode === 'edit' &&
			(this.editing?.ready() ??
				(!this.editor.busy() && this.editor.snapshot()?.transaction === null)),
	);
	protected readonly isPrefab = computed(() => this.selected()?.kind === 'prefab');
	protected readonly hasSelection = computed(() => !!this.editor.snapshot()?.selection.length);
	private readonly context = computed(() =>
		JSON.stringify([
			this.editor.workspaceGeneration(),
			this.editor.snapshot()?.active_scene_id,
			this.editor.snapshot()?.mode,
		]),
	);
	private positionBeforeEdit: Vec3 | null = null;
	private destroyed = false;

	constructor() {
		effect(() => {
			this.context();
			untracked(() => {
				this.closeSource();
				this.closeLink();
				this.closeCreate();
				this.error.set(null);
				this.message.set(null);
			});
		});
		inject(DestroyRef).onDestroy(() => {
			this.destroyed = true;
			this.closeSource();
			this.closeLink();
		});
	}

	protected async beginCreate(): Promise<void> {
		if (!this.available() || !this.editor.snapshot()?.selection.length) return;
		await this.run(async (_generation, revision) => {
			const snapshot = this.editor.snapshot();
			if (!snapshot?.selection.length) return;
			this.createReview.set(reviewSelection(revision, snapshot.selection, snapshot.document.entities));
			this.createSlots.set([]);
			this.createPath.set('prop.json');
			this.createPivot.set('');
			this.createOpen.set(true);
		});
	}

	protected closeCreate(): void {
		this.createOpen.set(false);
		this.createReview.set(null);
	}

	protected changeSlot(index: number, field: 'name' | 'target', value: string): void {
		this.createSlots.update((rows) =>
			rows.map((row, position) => (position === index ? { ...row, [field]: value } : row)),
		);
	}
	protected addSlot(): void {
		if (this.createSlots().length < 128)
			this.createSlots.update((rows) => [...rows, { name: '', target: '' }]);
	}
	protected removeSlot(index: number): void {
		this.createSlots.update((rows) => rows.filter((_, position) => position !== index));
	}

	protected async create(): Promise<void> {
		const review = this.createReview();
		const path = this.createPath().trim();
		const pivot = this.createPivot();
		if (!this.createOpen() || !review || !this.available()) return;
		if (!relativeAssetPath(path) || !/\.json$/i.test(path)) {
			this.error.set('Choose an asset-relative JSON source path in an existing folder.');
			return;
		}
		const slots = this.createSlots();
		if (pivot && !review.memberKeys.has(pivot)) {
			this.error.set('Choose a pivot from the reviewed Prop objects.');
			return;
		}
		const external = new Set(this.entities().filter(entity => !review.memberKeys.has(entity.key)).map(entity => entity.key));
		if (
			slots.some(
				(slot) =>
					!/^[A-Za-z0-9_-]{1,64}$/.test(slot.name) ||
					!external.has(slot.target),
			) ||
			new Set(slots.map((slot) => slot.name)).size !== slots.length
		) {
			this.error.set('Choose unique slot names and existing external scene references.');
			return;
		}
		await this.run(async (generation, revision, current) => {
			const selected = this.editor.snapshot()?.selection;
			if (
				this.createReview() !== review || revision !== review.revision ||
				selected?.length !== review.selection.length || review.selection.some((key, index) => key !== selected[index])
			)
				throw new Error('The scene or selection changed. Close this review and review your selection again.');
			const draft = await this.editor.createLinkedPrefabSource(
				path,
				Object.fromEntries(slots.map((slot) => [slot.name, slot.target])),
				generation,
				review.revision,
				pivot || undefined,
			);
			if (!(await this.acceptDraft(draft, false, current))) return;
			this.closeCreate();
			this.message.set(`Saved ${draft.path}. You can place it from Assets.`);
			await this.editor.assets(true);
		});
	}

	protected async openSource(link: boolean): Promise<void> {
		const asset = this.selected();
		if (!asset) return;
		await this.inspectSource(asset, link);
	}

	async placeLinkedProp(asset: ModelAsset): Promise<void> {
		await this.inspectSource(asset, true);
	}

	private async inspectSource(asset: ModelAsset, link: boolean): Promise<void> {
		if (asset.kind !== 'prefab' || !this.available()) return;
		await this.run(async (generation, revision, current) => {
			await this.acceptDraft(
				await this.editor.openPrefabSource(asset.path, generation, revision),
				link,
				current,
			);
		});
	}

	private async acceptDraft(
		draft: PrefabSourceDraft,
		link: boolean,
		current: () => boolean,
	): Promise<boolean> {
		if (!current()) {
			void this.editor.closePrefabSource(draft.draft_id).catch(() => undefined);
			return false;
		}
		try {
			readPrefabSource(draft.prefab);
		} catch {
			await this.editor.closePrefabSource(draft.draft_id).catch(() => undefined);
			throw new Error(
				'This file is a gameplay prefab. Choose Place as Copy, or create a Prop from scene objects to make linked placements.',
			);
		}
		if (link) {
			this.link.set(draft);
			this.bindings.set({});
			this.linkPosition.set([0, 0, 0]);
			this.positionBeforeEdit = null;
			this.linkOpen.set(true);
		} else this.source.set(draft);
		return true;
	}

	protected closeSource(): void {
		const source = this.source();
		this.source.set(null);
		if (source) void this.editor.closePrefabSource(source.draft_id).catch(() => undefined);
	}
	protected closeLink(): void {
		const source = this.link();
		this.link.set(null);
		this.linkOpen.set(false);
		if (source) void this.editor.closePrefabSource(source.draft_id).catch(() => undefined);
	}

	protected beginPositionEdit(): void {
		this.positionBeforeEdit = this.linkPosition();
	}

	protected changePosition({axis, value}: Vector3Change): void {
		const position: [number, number, number] = [...this.linkPosition()];
		position[axis] = value;
		this.linkPosition.set(position);
	}

	protected cancelPositionEdit(): void {
		if (this.positionBeforeEdit) this.linkPosition.set(this.positionBeforeEdit);
		this.positionBeforeEdit = null;
	}

	protected async instantiate(): Promise<void> {
		const source = this.link();
		if (!source || !this.canLink() || !this.available()) return;
		await this.run(async (generation, _revision, current) => {
			await this.editor.instantiateLinkedPrefab(
				source.path,
				source.draft_id,
				this.linkPosition(),
				this.bindings(),
				generation,
				source.revision,
			);
			if (!current()) return;
			this.closeLink();
			this.message.set('Linked instance placed. Its source version changes only through Update.');
		});
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
		this.message.set(null);
		try {
			const write = async () => {
				if (!current()) return;
				try {
					await action(generation, this.editor.snapshot()!.revision, current);
				} catch (error) {
					if (current()) this.error.set(error instanceof Error ? error.message : String(error));
				}
			};
			const accepted = this.editing
				? await this.editing.withCommittedEdit(write)
				: (await write(), true);
			if (!accepted && current())
				this.error.set(this.editing?.error() ?? 'Finish the current edit and try again.');
		} catch (error) {
			if (current()) this.error.set(error instanceof Error ? error.message : String(error));
		} finally {
			this.pending.set(false);
		}
	}
}

function reviewSelection(
	revision: number, selection: readonly EntityKey[], entities: readonly SceneEntity[],
): PropCreationReview {
	const children = new Map<EntityKey, SceneEntity[]>();
	const byKey = new Map<EntityKey, SceneEntity>();
	for (const entity of entities) {
		byKey.set(entity.key, entity);
		if (!entity.parent) continue;
		const siblings = children.get(entity.parent);
		if (siblings) siblings.push(entity);
		else children.set(entity.parent, [entity]);
	}
	const pending = [...selection];
	const memberKeys = new Set<EntityKey>();
	while (pending.length) {
		const key = pending.pop()!;
		if (memberKeys.has(key)) continue;
		if (!byKey.has(key)) throw new Error('A selected object has disappeared. Review your selection again.');
		memberKeys.add(key);
		if (memberKeys.size > MAX_PROP_MEMBERS) throw new Error(`A Prop supports at most ${MAX_PROP_MEMBERS} objects.`);
		for (const child of children.get(key) ?? []) pending.push(child.key);
	}
	const members = entities.filter(entity => memberKeys.has(entity.key));
	return {revision, selection, memberKeys, members, roots: members.filter(entity => !entity.parent || !memberKeys.has(entity.parent))};
}
