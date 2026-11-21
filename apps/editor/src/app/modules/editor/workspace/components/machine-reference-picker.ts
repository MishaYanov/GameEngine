import { Component, computed, input, output } from '@angular/core';
import type { ComponentReference } from '../../../../core/editor/state-machine.models';
import {EditorSearchPicker} from '../../../../shared/components/search-picker/search-picker';
import {sceneReferenceOptions} from '../scene-reference-options';
import type { ComponentTarget } from './component-draft';

@Component({
	selector: 'editor-machine-reference',
    imports: [EditorSearchPicker],
	template: `
		<editor-search-picker [label]="label() + ' entity'" placeholder="Search entities…"
			[attr.data-component-reference-field]="selectedEntity() ? null : referenceField()"
			[options]="entityOptions()" [value]="value().entity" [clearable]="true"
			[disabled]="disabled()" (picked)="entity($event)" />
		<label
			>{{ label() }} component ID<input
				#attachment
				[value]="value().component"
				[attr.list]="listId"
				[attr.data-component-reference-field]="selectedEntity() ? referenceField() : null"
				[disabled]="disabled()"
				maxlength="128"
				(input)="component(attachment.value)"
		/></label>
		<datalist [id]="listId">
			@for (id of componentIds(); track id) {
				<option [value]="id"></option>
			}
		</datalist>
	`,
	styleUrl: './machine-editor.scss',
})
export class MachineReferencePicker {
	readonly value = input.required<ComponentReference>();
	readonly entities = input<readonly ComponentTarget[]>([]);
	readonly label = input('Target');
    readonly disabled = input(false);
    readonly referenceField = input<string | null>(null);
	readonly changed = output<ComponentReference>();
    protected readonly entityOptions = computed(() => sceneReferenceOptions(this.entities()));
	protected readonly listId = `machine-components-${crypto.randomUUID()}`;
    protected readonly selectedEntity = computed(() => this.entities().find(item => item.key === this.value().entity));
	protected readonly componentIds = computed(() => {
        const target = this.selectedEntity();
		return (target?.component_views ?? target?.components ?? [])
			.filter(
				(item) =>
					item.data.kind === 'native_behavior' && item.data.behavior.kind === 'state_machine',
			)
			.map((item) => item.id);
	});
	protected entity(entity: string): void {
        if (entity === this.value().entity || this.disabled()) return;
		this.changed.emit({ entity, component: '' });
	}
	protected component(component: string): void {
		this.changed.emit({ ...this.value(), component });
	}
}
