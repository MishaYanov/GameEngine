import {Component, computed, effect, inject, input, signal, viewChild} from '@angular/core';
import type { EditorCommand, SceneEntity } from '../../../../core/editor/editor.models';
import { EditorService } from '../../../../core/editor/editor.service';
import { ViewportEditorService } from '../../../../core/editor/viewport-editor.service';
import { SceneEditingService } from '../scene-editing.service';
import { WorkspaceViewService } from '../scene-viewport/workspace-view.service';
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
		[gameActions]="gameActions()"
		[interactablePinned]="interactablePinned()"
		[editable]="available()"
		[previewAllowed]="true"
		[pending]="pending()"
		[error]="error()"
		[acknowledged]="acknowledged()"
		(actionRequested)="apply($event)"
	/>
    @if (canShowCameraActions()) {
        <button type="button" [disabled]="!canAlignCamera()" (click)="alignCamera()">Align Camera to Scene View</button>
    }`,
})
export class SceneComponentInspector {
	readonly entity = input<SceneEntity | null>(null);
	readonly editable = input(false);
    private readonly componentEditor = viewChild(ComponentEditor);
	private readonly editor = inject(EditorService);
	private readonly editing = inject(SceneEditingService);
	private readonly viewport = inject(ViewportEditorService, {optional: true});
	private readonly views = inject(WorkspaceViewService, {optional: true});
	protected readonly types = computed(() => this.editor.capabilities()?.component_types ?? []);
	protected readonly targets = computed(() => this.editor.snapshot()?.document.entities ?? []);
	protected readonly models = computed(() => this.editor.modelAssets() ?? []);
    protected readonly gameActions = computed(() =>
        this.editor.snapshot()?.project.input_profile?.actions?.map(action => action.id) ?? [],
    );
    protected readonly interactablePinned = computed(() => {
        const entity = this.entity();
        if (!entity) return false;
        if (entity.linked_prefab) return true;
        if (!entity.parent) return false;
        const byKey = new Map(this.targets().map(candidate => [candidate.key, candidate]));
        const visited = new Set([entity.key]);
        let parentKey: string | null | undefined = entity.parent;
        while (parentKey && !visited.has(parentKey)) {
            visited.add(parentKey);
            const ancestor = byKey.get(parentKey);
            if (ancestor?.linked_prefab) return true;
            parentKey = ancestor?.parent;
        }
        return false;
    });
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
	protected readonly canShowCameraActions = computed(() => !!this.entity()?.camera &&
		(this.editor.capabilities()?.commands?.some(command => command.name === 'align_camera_to_scene_view') ?? false));
	protected readonly canAlignCamera = computed(() => {
		const view = this.viewport?.state();
		const snapshot = this.editor.snapshot();
		return this.canShowCameraActions() && this.available() && snapshot?.mode === 'edit' &&
			snapshot.selection.length === 1 && snapshot.selection[0] === this.entity()?.key &&
			(!this.views || this.views.activeViewKind() === 'scene-viewport') &&
			view?.settings.view === 'scene' && view.workspace_generation !== null &&
			!view.gesture && !view.placement && !this.viewport?.pending();
	});

	constructor() {
		effect(() => {
			this.context();
			this.error.set(null);
		});
	}

    revealReference(componentId: string, field: string): boolean {
        const editor = this.componentEditor();
        if (!editor || !this.entity()?.component_views?.some(component => component.id === componentId))
            return false;
        editor.revealReference(componentId, field);
        return true;
    }

	protected async alignCamera(): Promise<void> {
		const context = this.context();
		const key = this.entity()?.key;
		const viewport = this.viewport;
		if (!key || !viewport || !this.canAlignCamera()) return;
		this.pending.set(true);
		this.error.set(null);
		const current = () => this.context() === context;
		try {
			const accepted = await this.editing.withCommittedEdit(async () => {
				const snapshot = this.editor.snapshot();
				const view = viewport.state();
				if (!current() || snapshot?.mode !== 'edit' || snapshot.selection.length !== 1 || snapshot.selection[0] !== key ||
					view?.settings.view !== 'scene' || view.workspace_generation === null) return;
				await viewport.action({
					type: 'align_camera_to_scene_view', key, expected_revision: snapshot.revision,
					expected_generation: view.workspace_generation, scene_id: snapshot.active_scene_id,
				});
				if (current()) await this.editor.refresh();
			});
			if (!accepted && current()) this.error.set(this.editing.error() ?? 'Finish the current edit before aligning the Camera.');
		} catch (error) {
			if (current()) this.error.set(error instanceof Error ? error.message : String(error));
		} finally {
			this.pending.set(false);
		}
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
