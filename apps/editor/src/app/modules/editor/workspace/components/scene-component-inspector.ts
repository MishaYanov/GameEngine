import { Component, computed, effect, inject, input, signal } from '@angular/core';
import type { EditorCommand, SceneEntity } from '../../../../core/editor/editor.models';
import { EditorService } from '../../../../core/editor/editor.service';
import { SceneEditingService } from '../scene-editing.service';
import { ComponentEditor, type ComponentEditIntent } from './component-editor';

@Component({
	selector: 'editor-scene-component-inspector',
	imports: [ComponentEditor],
	template: `<editor-component-editor
		[components]="entity()?.component_views ?? []"
		[types]="types()"
		[ownerKey]="entity()?.key ?? ''"
		[contextKey]="context()"
		[entities]="targets()"
		[models]="models()"
		[editable]="available()"
		[pending]="pending()"
		[error]="error()"
		[acknowledged]="acknowledged()"
		(actionRequested)="apply($event)"
	/>`,
})
export class SceneComponentInspector {
	readonly entity = input<SceneEntity | null>(null);
	readonly editable = input(false);
	private readonly editor = inject(EditorService);
	private readonly editing = inject(SceneEditingService);
	protected readonly types = computed(() => this.editor.capabilities()?.component_types ?? []);
	protected readonly targets = computed(() => this.editor.snapshot()?.document.entities ?? []);
	protected readonly models = computed(() => this.editor.modelAssets() ?? []);
	protected readonly pending = signal(false);
	protected readonly error = signal<string | null>(null);
	protected readonly acknowledged = signal(0);
	protected readonly context = computed(() =>
		JSON.stringify([
			this.editor.workspaceGeneration(),
			this.editor.snapshot()?.active_scene_id,
			this.entity()?.key,
		]),
	);
	protected readonly available = computed(
		() => this.editable() && !this.pending() && this.types().length > 0,
	);

	constructor() {
		effect(() => {
			this.context();
			this.error.set(null);
		});
	}

	protected async apply(intent: ComponentEditIntent): Promise<void> {
		const entity = this.entity();
		const context = this.context();
		const generation = this.editor.workspaceGeneration();
		if (!entity || !this.available()) return;
		const current = () => context === this.context() && this.editor.snapshot()?.mode === 'edit';
		this.pending.set(true);
		this.error.set(null);
		try {
			const accepted = await this.editing.withCommittedEdit(async () => {
				try {
					const snapshot = this.editor.snapshot();
					if (!snapshot || !current()) return;
					const latest = snapshot.document.entities.find(
						(candidate) => candidate.key === entity.key,
					);
					if (!latest) return;
					const id =
						intent.kind === 'add' || intent.kind === 'replace'
							? intent.component.id
							: intent.component_id;
					const existing = latest.component_views?.find((component) => component.id === id);
					if (
						'expected' in intent &&
						JSON.stringify(existing ?? null) !== (intent.expected ?? 'null')
					) {
						throw new Error(
							'This component changed. Discard the draft and review its current values.',
						);
					}
					let command: EditorCommand;
					if (intent.kind === 'add' || intent.kind === 'replace')
						command = {
							type: intent.kind === 'add' ? 'add_component' : 'replace_component',
							key: entity.key,
							component: intent.component,
						};
					else if (intent.kind === 'remove')
						command = {
							type: 'remove_component',
							key: entity.key,
							component_id: intent.component_id,
						};
					else {
						if (!existing) throw new Error('This component is no longer attached.');
						if (existing.enabled === intent.enabled) return;
						command = {
							type: 'set_component_enabled',
							key: entity.key,
							component_id: intent.component_id,
							enabled: intent.enabled,
						};
					}
					await this.editor.execute(command, generation, snapshot.revision);
					if (current()) this.acknowledged.update((value) => value + 1);
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
