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
import type { ModelAsset } from '../../../../core/editor/editor.models';
import type { PrefabSourceDraft } from '../../../../core/editor/linked-prefabs.models';
import { EditorService } from '../../../../core/editor/editor.service';
import { EditorDialog } from '../../../../shared/components/dialog/dialog';
import { SceneEditingService } from '../scene-editing.service';
import { relativeAssetPath } from '../components/component-draft';
import { PrefabSourceEditor } from './prefab-source-editor';
import { readPrefabSource } from './prefab-source-values';
import { ReferenceBindings } from './reference-bindings';

@Component({
	selector: 'editor-prefab-authoring',
	imports: [EditorDialog, PrefabSourceEditor, ReferenceBindings],
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
	protected readonly createPath = signal('linked-source.json');
	protected readonly createSlots = signal<readonly { name: string; target: string }[]>([]);
	protected readonly bindings = signal<Readonly<Record<string, string>>>({});
	protected readonly entities = computed(() => this.editor.snapshot()?.document.entities ?? []);
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
	private selection = '';
	private createContext = '';
	private destroyed = false;

	constructor() {
		effect(() => {
			this.context();
			untracked(() => {
				this.closeSource();
				this.closeLink();
				this.createOpen.set(false);
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

	protected beginCreate(): void {
		if (!this.available() || !this.editor.snapshot()?.selection.length) return;
		this.selection = JSON.stringify(this.editor.snapshot()!.selection);
		this.createContext = this.context();
		this.createSlots.set([]);
		this.createPath.set('linked-source.json');
		this.error.set(null);
		this.createOpen.set(true);
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
		const path = this.createPath().trim();
		if (!this.createOpen() || !this.available()) return;
		if (!relativeAssetPath(path) || !/\.json$/i.test(path)) {
			this.error.set('Choose an asset-relative JSON source path in an existing folder.');
			return;
		}
		const slots = this.createSlots();
		if (
			slots.some(
				(slot) =>
					!/^[A-Za-z0-9_-]{1,64}$/.test(slot.name) ||
					!this.entities().some((entity) => entity.key === slot.target),
			) ||
			new Set(slots.map((slot) => slot.name)).size !== slots.length
		) {
			this.error.set('Choose unique slot names and existing external scene references.');
			return;
		}
		await this.run(async (generation, revision, current) => {
			if (
				this.createContext !== this.context() ||
				this.selection !== JSON.stringify(this.editor.snapshot()!.selection)
			)
				throw new Error('The selection changed. Review it before creating a source.');
			const draft = await this.editor.createLinkedPrefabSource(
				path,
				Object.fromEntries(slots.map((slot) => [slot.name, slot.target])),
				generation,
				revision,
			);
			if (!(await this.acceptDraft(draft, false, current))) return;
			await this.editor.assets(true);
			this.createOpen.set(false);
		});
	}

	protected async openSource(link: boolean): Promise<void> {
		const asset = this.selected();
		if (!asset || !this.isPrefab() || !this.available()) return;
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
				'This is a copied legacy gameplay prefab. Use Instantiate Prefab for a copy, or create a component source to link it.',
			);
		}
		if (link) {
			this.link.set(draft);
			this.bindings.set({});
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

	protected async instantiate(): Promise<void> {
		const source = this.link();
		if (!source || !this.canLink() || !this.available()) return;
		await this.run(async (generation, _revision, current) => {
			await this.editor.instantiateLinkedPrefab(
				source.path,
				[0, 0, 0],
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
