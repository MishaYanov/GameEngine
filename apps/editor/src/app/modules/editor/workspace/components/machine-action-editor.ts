import { Component, input, output } from '@angular/core';
import {
	GAMEPLAY_SIGNALS,
	type GameplaySignal,
	type MachineAction,
} from '../../../../core/editor/state-machine.models';
import type { ComponentTarget } from './component-draft';
import { MachineReferencePicker } from './machine-reference-picker';

@Component({
	selector: 'editor-machine-action',
	imports: [MachineReferencePicker],
	template: `
		<label
			>Entry action<select #kind [value]="value().kind" (change)="choose(kind.value)">
				<option value="emit">Send signal</option>
				<option value="set_visible">Set visibility</option>
				<option value="set_collision_enabled">Set collision</option>
				<option value="despawn_owner">Despawn owner</option>
			</select></label
		>
		@let action = value();
		@if (action.kind === 'emit') {
			<editor-machine-reference
				label="Recipient"
				[entities]="entities()"
				[value]="action.target"
				(changed)="changed.emit({ kind: 'emit', target: $event, signal: action.signal })"
			/>
			<label
				>Signal<select #signal [value]="action.signal" (change)="setSignal(signal.value)">
					@for (item of signals; track item) {
						<option [value]="item" [selected]="item === action.signal">{{ item }}</option>
					}
				</select></label
			>
		} @else if (action.kind === 'set_visible' || action.kind === 'set_collision_enabled') {
			<label
				>Target entity<select #entity [value]="action.entity" (change)="setEntity(entity.value)">
					<option value="">Choose entity</option>
					@for (item of entities(); track item.key) {
						<option [value]="item.key" [selected]="item.key === action.entity">
							{{ item.name }}
						</option>
					}
				</select></label
			>
			@if (action.kind === 'set_visible') {
				<label class="check"
					><input
						#visible
						type="checkbox"
						[checked]="action.visible"
						(change)="
							changed.emit({ kind: 'set_visible', entity: action.entity, visible: visible.checked })
						"
					/>Visible</label
				>
			} @else {
				<label class="check"
					><input
						#enabled
						type="checkbox"
						[checked]="action.enabled"
						(change)="
							changed.emit({
								kind: 'set_collision_enabled',
								entity: action.entity,
								enabled: enabled.checked,
							})
						"
					/>Collision enabled</label
				>
			}
		} @else {
			<p>Retires the spawned owner and its owned children. Authored owners cannot be despawned.</p>
		}
	`,
	styleUrl: './machine-editor.scss',
})
export class MachineActionEditor {
	readonly value = input.required<MachineAction>();
	readonly entities = input<readonly ComponentTarget[]>([]);
	readonly changed = output<MachineAction>();
	protected readonly signals = GAMEPLAY_SIGNALS;
	protected choose(kind: string): void {
		switch (kind) {
			case 'emit':
				this.changed.emit({ kind, target: { entity: '', component: '' }, signal: 'activated' });
				break;
			case 'set_visible':
				this.changed.emit({ kind, entity: '', visible: true });
				break;
			case 'set_collision_enabled':
				this.changed.emit({ kind, entity: '', enabled: true });
				break;
			case 'despawn_owner':
				this.changed.emit({ kind });
				break;
		}
	}
	protected setSignal(value: string): void {
		const action = this.value();
		if (action.kind === 'emit' && this.signals.some((item) => item === value))
			this.changed.emit({ ...action, signal: value as GameplaySignal });
	}
	protected setEntity(entity: string): void {
		const action = this.value();
		if (action.kind === 'set_visible' || action.kind === 'set_collision_enabled')
			this.changed.emit({ ...action, entity });
	}
}
