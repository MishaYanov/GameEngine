import {Component, computed, input, output} from '@angular/core';
import {
	GAMEPLAY_SIGNALS,
	MACHINE_DESCRIPTOR,
	type GameplaySignal,
	type MachineDefinition,
	type MachineState,
	type MachineAction,
	type MachineTransition,
	type MachineTrigger,
	type SensorInput,
} from '../../../../core/editor/state-machine.models';
import type { ComponentTarget } from './component-draft';
import { MachineReferencePicker } from './machine-reference-picker';
import { MachineActionEditor } from './machine-action-editor';
import {EditorSearchPicker} from '../../../../shared/components/search-picker/search-picker';
import {sceneReferenceOptions} from '../scene-reference-options';

@Component({
	selector: 'editor-machine-editor',
    imports: [MachineReferencePicker, MachineActionEditor, EditorSearchPicker],
	templateUrl: './machine-editor.html',
	styleUrl: './machine-editor.scss',
})
export class MachineEditor {
	readonly value = input.required<MachineDefinition>();
	readonly entities = input<readonly ComponentTarget[]>([]);
    readonly disabled = input(false);
	readonly changed = output<MachineDefinition>();
    protected readonly entityOptions = computed(() => sceneReferenceOptions(this.entities()));
	protected readonly signals = GAMEPLAY_SIGNALS;
	protected readonly limits = MACHINE_DESCRIPTOR;
	protected initial(initial: string): void {
		this.changed.emit({ ...this.value(), initial });
	}
	protected state(index: number, update: Partial<MachineState>): void {
		this.changed.emit({
			...this.value(),
			states: this.value().states.map((state, i) =>
				i === index ? { ...state, ...update } : state,
			),
		});
	}
	protected rename(index: number, id: string): void {
		const definition = this.value(),
			previous = definition.states[index].id;
		this.changed.emit({
			...definition,
			initial: definition.initial === previous ? id : definition.initial,
			states: definition.states.map((state, i) => ({
				...state,
				id: i === index ? id : state.id,
				transitions: state.transitions.map((item) =>
					item.next === previous ? { ...item, next: id } : item,
				),
			})),
		});
	}
	protected addState(): void {
		if (this.value().states.length >= 32) return;
		let suffix = 1;
		while (this.value().states.some((item) => item.id === `state_${suffix}`)) suffix++;
		this.changed.emit({
			...this.value(),
			states: [...this.value().states, { id: `state_${suffix}`, on_enter: [], transitions: [] }],
		});
	}
	protected removeState(index: number): void {
		if (this.value().states.length > 1)
			this.changed.emit({
				...this.value(),
				states: this.value().states.filter((_, i) => i !== index),
			});
	}
	protected action(state: number, index: number, value: MachineAction): void {
		this.state(state, {
			on_enter: this.value().states[state].on_enter.map((item, i) => (i === index ? value : item)),
		});
	}
	protected addAction(state: number): void {
		const actions = this.value().states[state].on_enter;
		if (actions.length < 8)
			this.state(state, {
				on_enter: [...actions, { kind: 'set_visible', entity: '', visible: true }],
			});
	}
	protected removeAction(state: number, index: number): void {
		this.state(state, {
			on_enter: this.value().states[state].on_enter.filter((_, i) => i !== index),
		});
	}
	protected transition(state: number, index: number, update: Partial<MachineTransition>): void {
		this.state(state, {
			transitions: this.value().states[state].transitions.map((item, i) =>
				i === index ? { ...item, ...update } : item,
			),
		});
	}
	protected transitionKind(state: number, index: number, kind: string): void {
		const when: MachineTrigger =
			kind === 'after_ticks'
				? { kind: 'after_ticks', ticks: 60 }
				: { kind: 'signal', signal: 'activated', source: null };
		this.transition(state, index, { when });
	}
	protected signal(state: number, index: number, value: string): void {
		const when = this.value().states[state].transitions[index].when;
		if (when.kind === 'signal' && this.signals.some((item) => item === value))
			this.transition(state, index, { when: { ...when, signal: value as GameplaySignal } });
	}
	protected timer(state: number, index: number, value: string): void {
		this.transition(state, index, { when: { kind: 'after_ticks', ticks: Number(value) } });
	}
	protected filter(state: number, index: number, enabled: boolean): void {
		const when = this.value().states[state].transitions[index].when;
		if (when.kind === 'signal')
			this.transition(state, index, {
				when: { ...when, source: enabled ? { entity: '', component: '' } : null },
			});
	}
	protected addTransition(state: number): void {
		if (this.value().states.reduce((n, item) => n + item.transitions.length, 0) >= 128) return;
		const current = this.value().states[state];
		this.state(state, {
			transitions: [
				...current.transitions,
				{ when: { kind: 'signal', signal: 'activated', source: null }, next: current.id },
			],
		});
	}
	protected removeTransition(state: number, index: number): void {
		this.state(state, {
			transitions: this.value().states[state].transitions.filter((_, i) => i !== index),
		});
	}
	protected sensor(index: number, update: Partial<SensorInput>): void {
		this.changed.emit({
			...this.value(),
			inputs: this.value().inputs.map((item, i) => (i === index ? { ...item, ...update } : item)),
		});
	}
	protected sensorSignal(index: number, value: string): void {
		if (this.signals.some((item) => item === value))
			this.sensor(index, { signal: value as GameplaySignal });
	}
	protected addSensor(): void {
		if (this.value().inputs.length >= 8) return;
		let suffix = 1;
		while (this.value().inputs.some((item) => item.id === `touch_${suffix}`)) suffix++;
		this.changed.emit({
			...this.value(),
			inputs: [
				...this.value().inputs,
				{ id: `touch_${suffix}`, actor: '', sensor: '', signal: 'activated' },
			],
		});
	}
	protected removeSensor(index: number): void {
		this.changed.emit({
			...this.value(),
			inputs: this.value().inputs.filter((_, i) => i !== index),
		});
	}
}
