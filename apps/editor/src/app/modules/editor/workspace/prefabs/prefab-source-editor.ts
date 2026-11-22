import {
	Component,
	computed,
	DestroyRef,
	effect,
	inject,
	input,
	output,
	signal,
	untracked,
} from '@angular/core';
import { form, FormField } from '@angular/forms/signals';
import type {
	PrefabSourceDocument,
	PrefabSourceDraft,
	PrefabSourceEntity,
} from '../../../../core/editor/linked-prefabs.models';
import { EditorService } from '../../../../core/editor/editor.service';
import { EditorDialog } from '../../../../shared/components/dialog/dialog';
import { SceneEditingService } from '../scene-editing.service';
import { ComponentEditor, type ComponentEditIntent } from '../components/component-editor';
import {
	entityDraft,
	readPrefabSource,
	slotValues,
	sourceEntityFromDraft,
	type ReferenceSlotDraft,
} from './prefab-source-values';

@Component({
	selector: 'editor-prefab-source-editor',
	imports: [EditorDialog, FormField, ComponentEditor],
	templateUrl: './prefab-source-editor.html',
	styleUrls: ['../game-settings/settings.scss', './prefab-source-editor.scss'],
})
export class PrefabSourceEditor {
	readonly source = input.required<PrefabSourceDraft>();
	readonly closed = output<void>();
	readonly saved = output<PrefabSourceDraft>();
	private readonly editor = inject(EditorService);
	private readonly editing = inject(SceneEditingService);
	protected readonly open = signal(true);
	protected readonly document = signal<PrefabSourceDocument | null>(null);
	protected readonly selected = signal('');
	protected readonly entity = computed(
		() => this.document()?.entities.find((entity) => entity.key === this.selected()) ?? null,
	);
	protected readonly draft = signal(entityDraft());
	protected readonly fields = form(this.draft);
	protected readonly slots = signal<readonly ReferenceSlotDraft[]>([]);
	protected readonly pending = signal(false);
	protected readonly error = signal<string | null>(null);
	protected readonly message = signal<string | null>(null);
	protected readonly componentOpen = signal(false);
	protected readonly acknowledged = signal(0);
	protected readonly componentTypes = computed(
		() => this.editor.capabilities()?.component_types?.filter(type => type.kind !== 'player_controller') ?? [],
	);
	protected readonly models = computed(() => this.editor.modelAssets() ?? []);
	protected readonly axes = ['x', 'y', 'z'] as const;
	protected readonly vectors = [
		{ key: 'position', label: 'Position' },
		{ key: 'rotation', label: 'Rotation (radians)' },
		{ key: 'scale', label: 'Scale' },
	] as const;
	protected readonly entityDirty = computed(
		() => JSON.stringify(this.draft()) !== JSON.stringify(entityDraft(this.entity() ?? undefined)),
	);
	protected readonly sourceDirty = computed(
		() =>
			this.entityDirty() ||
			this.componentOpen() ||
			JSON.stringify(this.candidate().value) !== JSON.stringify(this.source().prefab),
	);
	protected readonly available = computed(
		() => !this.pending() && !this.componentOpen() && !this.entityDirty(),
	);
	protected readonly targets = computed(() => [
		...(this.document()?.entities ?? []),
		...this.slots().map((slot) => ({
			key: slot.key,
			name: `Reference slot: ${slot.name || '(unnamed)'}`,
		})),
	]);
	protected readonly candidate = computed(() => {
		try {
			const document = this.document();
			if (!document) throw new Error('Open a supported component source.');
			return {
				value: { ...document, reference_slots: slotValues(this.slots(), document.entities) },
				error: null,
			};
		} catch (error) {
			return { value: null, error: error instanceof Error ? error.message : String(error) };
		}
	});
	private readonly context = computed(() =>
		JSON.stringify([
			this.editor.workspaceGeneration(),
			this.editor.snapshot()?.active_scene_id,
			this.source().draft_id,
		]),
	);
	private destroyed = false;

	constructor() {
		effect(() => {
			const source = this.source();
			untracked(() => {
				try {
					const document = structuredClone(readPrefabSource(source.prefab));
					this.document.set(document);
					const key = document.entities.some((entity) => entity.key === this.selected())
						? this.selected()
						: document.entities[0].key;
					this.select(key, true);
					this.slots.set(
						Object.entries(document.reference_slots).map(([name, key]) => ({ name, key })),
					);
					this.error.set(null);
				} catch (error) {
					this.document.set(null);
					this.error.set(error instanceof Error ? error.message : String(error));
				}
			});
		});
		inject(DestroyRef).onDestroy(() => {
			this.destroyed = true;
		});
	}

	protected close(open: boolean): void {
		if (!open) this.closed.emit();
	}
	protected attemptedClose(): void {
		this.message.set(
			this.pending()
				? 'Wait for the source operation to finish before closing.'
				: 'Save the source changes or choose Discard Source Changes and Close.',
		);
	}
	protected discardAndClose(): void {
		if (this.pending()) return;
		this.open.set(false);
		this.closed.emit();
	}

	protected select(key: string, force = false): void {
		if (!force && !this.available()) return;
		this.selected.set(key);
		this.draft.set(entityDraft(this.document()?.entities.find((entity) => entity.key === key)));
		this.error.set(null);
	}

	protected applyEntity(): void {
		const entity = this.entity(),
			document = this.document();
		if (!entity || !document || this.pending() || this.componentOpen()) return;
		try {
			const next = sourceEntityFromDraft(this.draft(), entity, document.entities);
			this.document.set({
				...document,
				entities: document.entities.map((entity) => (entity.key === next.key ? next : entity)),
			});
			this.draft.set(entityDraft(next));
			this.error.set(null);
			this.message.set(null);
		} catch (error) {
			this.error.set(error instanceof Error ? error.message : String(error));
		}
	}

	protected addEntity(): void {
		const document = this.document();
		if (!document || !this.available() || document.entities.length >= 4096) return;
		const entity: PrefabSourceEntity = {
			key: crypto.randomUUID(),
			name: 'Object',
			parent: null,
			transform: { position: [0, 0, 0], rotation: [0, 0, 0], scale: [1, 1, 1] },
			components: [],
		};
		this.document.set({ ...document, entities: [...document.entities, entity] });
		this.select(entity.key, true);
		this.message.set(null);
	}

	protected removeEntity(): void {
		const entity = this.entity(),
			document = this.document();
		if (!entity || !document || !this.available() || document.entities.length <= 1) return;
		if (document.entities.some((candidate) => candidate.parent === entity.key)) {
			this.error.set('Reparent or remove this object’s children before removing it.');
			return;
		}
		this.document.set({
			...document,
			entities: document.entities.filter((candidate) => candidate.key !== entity.key),
		});
		this.select(this.document()!.entities[0].key, true);
		this.message.set('Object removed from this draft. Validate to check remaining references.');
	}

	protected component(intent: ComponentEditIntent): void {
		const entity = this.entity(),
			document = this.document();
		if (!entity || !document || this.pending() || this.entityDirty()) return;
		const id = 'component' in intent ? intent.component.id : intent.component_id;
		const existing = entity.components.find((component) => component.id === id);
		if ('expected' in intent && JSON.stringify(existing ?? null) !== (intent.expected ?? 'null')) {
			this.error.set(
				'This source component changed. Discard its draft and review the current values.',
			);
			return;
		}
		let components = entity.components;
		if (intent.kind === 'add') components = [...components, intent.component];
		else if (intent.kind === 'replace')
			components = components.map((component) =>
				component.id === id ? intent.component : component,
			);
		else if (intent.kind === 'remove')
			components = components.filter((component) => component.id !== id);
		else
			components = components.map((component) =>
				component.id === id ? { ...component, enabled: intent.enabled } : component,
			);
		this.document.set({
			...document,
			entities: document.entities.map((candidate) =>
				candidate.key === entity.key ? { ...entity, components } : candidate,
			),
		});
		this.acknowledged.update((value) => value + 1);
		this.error.set(null);
		this.message.set(null);
	}

	protected addSlot(): void {
		if (this.available() && this.slots().length < 128)
			this.slots.update((rows) => [...rows, { name: '', key: `slot-${crypto.randomUUID()}` }]);
	}
	protected renameSlot(index: number, name: string): void {
		this.slots.update((rows) =>
			rows.map((row, position) => (position === index ? { ...row, name } : row)),
		);
		this.message.set(null);
	}
	protected removeSlot(index: number): void {
		this.slots.update((rows) => rows.filter((_, position) => position !== index));
		this.message.set(null);
	}

	protected async persist(save: boolean): Promise<void> {
		const prefab = this.candidate().value;
		if (!prefab || !this.available()) return;
		const source = this.source(),
			context = this.context(),
			generation = this.editor.workspaceGeneration();
		const current = () =>
			!this.destroyed &&
			this.open() &&
			context === this.context() &&
			this.editor.snapshot()?.mode === 'edit';
		this.pending.set(true);
		this.message.set(null);
		try {
			const accepted = await this.editing.withCommittedEdit(async () => {
				if (!current()) return;
				try {
					const revision = this.editor.snapshot()!.revision;
					if (save) {
						const result = await this.editor.savePrefabSource(
							source.draft_id,
							prefab,
							generation,
							revision,
						);
						if (!current()) return;
						this.error.set(null);
						this.saved.emit(result.draft);
						await this.editor.assets(true);
						if (current())
							this.message.set(
								'Prefab source saved. Existing linked instances keep their current version until Update.',
							);
					} else {
						const result = await this.editor.validatePrefabSource(
							source.draft_id,
							prefab,
							generation,
							revision,
						);
						if (current()) {
							this.error.set(null);
							this.message.set(
								`Valid source: ${result.entity_count} objects, ${result.bytes} bytes.`,
							);
						}
					}
				} catch (error) {
					if (current()) this.error.set(error instanceof Error ? error.message : String(error));
				}
			});
			if (!accepted && current())
				this.error.set(this.editing.error() ?? 'Finish the current edit and try again.');
		} catch (error) {
			if (current()) this.error.set(error instanceof Error ? error.message : String(error));
		} finally {
			this.pending.set(false);
		}
	}
}
