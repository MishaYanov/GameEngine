import { Component, computed, effect, input, output, signal } from '@angular/core';
import { form, FormField } from '@angular/forms/signals';
import type {
	ComponentData,
	ComponentRecord,
	ComponentTypeDescriptor,
} from '../../../../core/editor/editor-components';
import type { Collider, ModelAsset } from '../../../../core/editor/editor.models';
import { ColliderInspector } from '../collider/collider-inspector';
import {
	changeColliderInteraction,
	defaultCollider,
	type ColliderInteractionChange,
	type ColliderKind,
} from '../collider/collider';
import { componentDraft, componentFromDraft, type ComponentTarget } from './component-draft';
import { MachineEditor } from './machine-editor';
import { CONTROLLER_FIELDS } from '../../../../core/editor/character-controller.models';
import type { MachineDefinition } from '../../../../core/editor/state-machine.models';
import type { AnimationSettings } from '../../../../core/editor/animation.models';
import { AnimationEditor } from './animation-editor';

export type ComponentEditIntent =
	| { readonly kind: 'add'; readonly component: ComponentRecord; readonly expected: string | null }
	| {
			readonly kind: 'replace';
			readonly component: ComponentRecord;
			readonly expected: string | null;
	  }
	| { readonly kind: 'remove'; readonly component_id: string; readonly expected: string }
	| { readonly kind: 'enabled'; readonly component_id: string; readonly enabled: boolean };

@Component({
	selector: 'editor-component-editor',
	imports: [FormField, ColliderInspector, MachineEditor, AnimationEditor],
	templateUrl: './component-editor.html',
	styleUrl: './component-editor.scss',
})
export class ComponentEditor {
	readonly components = input<readonly ComponentRecord[]>([]);
	readonly types = input<readonly ComponentTypeDescriptor[]>([]);
	readonly entities = input<readonly ComponentTarget[]>([]);
	readonly models = input<readonly ModelAsset[]>([]);
	readonly ownerKey = input('');
	readonly contextKey = input('');
	readonly editable = input(false);
	readonly pending = input(false);
	readonly error = input<string | null>(null);
	readonly acknowledged = input(0);
	readonly previewAllowed = input(false);
	readonly actionRequested = output<ComponentEditIntent>();
	readonly draftOpenChanged = output<boolean>();
	protected readonly open = signal(false);
	protected readonly selectedKind = signal<ComponentData['kind']>('native_behavior');
	protected readonly draft = signal(componentDraft('native_behavior', '', ''));
	protected readonly fields = form(this.draft);
	protected readonly collider = signal<Collider>(defaultCollider('box')!);
	protected readonly colliderInvalid = signal(false);
	protected readonly animationPending = signal(false);
	protected readonly original = signal<ComponentRecord | undefined>(undefined);
	protected readonly axes = ['x', 'y', 'z'] as const;
	protected readonly colors = ['r', 'g', 'b', 'a'] as const;
	protected readonly controllerFields = CONTROLLER_FIELDS;
	protected readonly previewSourceMatches = computed(() => {
		const original = this.original();
		return (
			this.previewAllowed() &&
			original?.data.kind === 'animation' &&
			JSON.stringify(original.data.settings.source) ===
				JSON.stringify(this.draft().animation?.source)
		);
	});
	protected readonly available = computed(() => this.editable() && !this.pending());
	protected readonly addTypes = computed(() =>
		this.components().length >= 16
			? []
			: this.types().filter(
					(type) =>
						this.components().filter((component) => component.data.kind === type.kind).length <
						type.max_per_object,
				),
	);
	protected readonly canAdd = computed(() =>
		this.addTypes().some((type) => type.kind === this.selectedKind()),
	);
	protected readonly candidate = computed(() => {
		try {
			if (this.animationPending()) throw new Error('Wait for animation preparation to finish.');
			if (this.colliderInvalid()) throw new Error('Finish or discard the invalid collider value.');
			return {
				value: componentFromDraft(
					this.draft(),
					this.collider(),
					this.entities(),
					this.models(),
					this.original(),
				),
				error: null,
			};
		} catch (error) {
			return { value: null, error: error instanceof Error ? error.message : String(error) };
		}
	});
	protected readonly references = computed<
		readonly {
			key: 'actor' | 'key_trigger' | 'door' | 'key_visual' | 'trigger' | 'visual';
			label: string;
			optional?: boolean;
		}[]
	>(() => {
		if (this.draft().behavior_kind === 'key_door')
			return [
				{ key: 'actor', label: 'Actor' },
				{ key: 'key_trigger', label: 'Key sensor' },
				{ key: 'door', label: 'Door' },
				{ key: 'key_visual', label: 'Key visual', optional: true },
			];
		return [
			{ key: 'actor', label: 'Actor' },
			{ key: 'trigger', label: 'Trigger sensor' },
			{ key: 'visual', label: 'Visual', optional: true },
		];
	});
	protected readonly modelListId = `component-models-${crypto.randomUUID()}`;
	protected readonly prefabListId = `component-prefabs-${crypto.randomUUID()}`;

	constructor() {
		effect(() => {
			this.contextKey();
			this.acknowledged();
			this.open.set(false);
			this.colliderInvalid.set(false);
		});
		effect(() => {
			this.draftOpenChanged.emit(this.open());
		});
		effect(() => {
			const types = this.addTypes();
			if (!types.some((type) => type.kind === this.selectedKind()) && types[0])
				this.selectedKind.set(types[0].kind);
		});
	}

	protected chooseKind(value: string): void {
		const type = this.addTypes().find((type) => type.kind === value);
		if (type) this.selectedKind.set(type.kind);
	}

	protected label(component: ComponentRecord): string {
		if (component.data.kind === 'native_behavior')
			return {
				key_door: 'Key and Door',
				collectible: 'Collectible',
				timed_spawner: 'Timed Spawner',
				state_machine: 'State Machine',
			}[component.data.behavior.kind];
		return (
			this.types().find((type) => type.kind === component.data.kind)?.label ??
			(component.data.kind === 'collider' ? 'Collider' : 'Mesh Renderer')
		);
	}

	protected start(component?: ComponentRecord): void {
		if (!this.available() || this.open()) return;
		const kind = component?.data.kind ?? this.selectedKind();
		if (!component && !this.addTypes().some((type) => type.kind === kind)) return;
		this.original.set(component);
		this.draft.set(
			componentDraft(kind, component?.id ?? crypto.randomUUID(), this.ownerKey(), component),
		);
		this.collider.set(
			component?.data.kind === 'collider' ? component.data.collider : defaultCollider('box')!,
		);
		this.colliderInvalid.set(false);
		this.open.set(true);
	}

	protected submit(): void {
		const component = this.candidate().value;
		if (!component || !this.available() || !this.open()) return;
		this.actionRequested.emit({
			kind: this.original() ? 'replace' : 'add',
			component,
			expected: this.original() ? JSON.stringify(this.original()) : null,
		});
	}

	protected toggle(component: ComponentRecord): void {
		if (this.available() && !this.open())
			this.actionRequested.emit({
				kind: 'enabled',
				component_id: component.id,
				enabled: !component.enabled,
			});
	}

	protected remove(component: ComponentRecord): void {
		if (this.available() && !this.open())
			this.actionRequested.emit({
				kind: 'remove',
				component_id: component.id,
				expected: JSON.stringify(component),
			});
	}

	protected changeCollider(collider: Collider): void {
		this.collider.set(collider);
		this.colliderInvalid.set(false);
	}
	protected changeInteraction(change: ColliderInteractionChange): void {
		this.changeCollider(changeColliderInteraction(this.collider(), change));
	}
	protected shape(kind: ColliderKind): void {
		const collider = defaultCollider(kind, this.collider().offset, this.collider().interaction);
		if (collider) this.changeCollider(collider);
		else this.colliderInvalid.set(true);
	}
	protected resetCollider(): void {
		const original = this.original();
		this.changeCollider(
			original?.data.kind === 'collider' ? original.data.collider : defaultCollider('box')!,
		);
	}
	protected addBinding(): void {
		if (this.draft().bindings.length < 32)
			this.draft.update((value) => ({
				...value,
				bindings: [...value.bindings, { name: '', target: '' }],
			}));
	}
	protected machineChanged(machine: MachineDefinition): void {
		this.draft.update((draft) => ({ ...draft, machine }));
	}
	protected animationChanged(animation: AnimationSettings): void {
		this.draft.update((draft) => ({ ...draft, animation }));
	}
	protected binding(index: number, field: 'name' | 'target', value: string): void {
		this.draft.update((draft) => ({
			...draft,
			bindings: draft.bindings.map((binding, position) =>
				position === index ? { ...binding, [field]: value } : binding,
			),
		}));
	}
	protected removeBinding(index: number): void {
		this.draft.update((draft) => ({
			...draft,
			bindings: draft.bindings.filter((_, position) => position !== index),
		}));
	}
}
