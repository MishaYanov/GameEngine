import { Component, computed, input, output } from '@angular/core';
import type { ComponentReference } from '../../../../core/editor/state-machine.models';
import type { ComponentTarget } from './component-draft';

@Component({
	selector: 'editor-machine-reference',
	template: `
		<label
			>{{ label() }} entity<select #target [value]="value().entity" (change)="entity(target.value)">
				<option value="">Choose entity</option>
				@for (item of entities(); track item.key) {
					<option [value]="item.key" [selected]="item.key === value().entity">
						{{ item.name }}
					</option>
				}
			</select></label
		>
		<label
			>{{ label() }} component ID<input
				#attachment
				[value]="value().component"
				[attr.list]="listId"
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
	readonly changed = output<ComponentReference>();
	protected readonly listId = `machine-components-${crypto.randomUUID()}`;
	protected readonly componentIds = computed(() => {
		const target = this.entities().find((item) => item.key === this.value().entity);
		return (target?.component_views ?? target?.components ?? [])
			.filter(
				(item) =>
					item.data.kind === 'native_behavior' && item.data.behavior.kind === 'state_machine',
			)
			.map((item) => item.id);
	});
	protected entity(entity: string): void {
		this.changed.emit({ entity, component: '' });
	}
	protected component(component: string): void {
		this.changed.emit({ ...this.value(), component });
	}
}
