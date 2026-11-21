import { TestBed } from '@angular/core/testing';
import axe from 'axe-core';
import {
	defaultMachine,
	MACHINE_DESCRIPTOR,
	validMachineDescriptor,
	machineError,
	type MachineDefinition,
} from '../../../../core/editor/state-machine.models';
import {
	validComponentViews,
	type ComponentTypeDescriptor,
} from '../../../../core/editor/editor-components';
import { componentDraft, componentFromDraft } from './component-draft';
import { ComponentEditor, type ComponentEditIntent } from './component-editor';

const box = {
	kind: 'box' as const,
	half_extents: [0.5, 0.5, 0.5] as const,
	offset: [0, 0, 0] as const,
};
const types: ComponentTypeDescriptor[] = [
	{
		type_id: 'engine.native_behavior',
		kind: 'native_behavior',
		label: 'Native Behavior',
		description: 'Tables and native rules.',
		max_per_object: 8,
	},
	{
		type_id: 'engine.character_controller',
		kind: 'character_controller',
		label: 'Character Controller',
		description: 'Grounded box body.',
		max_per_object: 1,
	},
];
function button(root: HTMLElement, name: string): HTMLButtonElement {
	return [...root.querySelectorAll('button')].find((item) => item.textContent?.trim() === name)!;
}
function field(root: HTMLElement, label: string): HTMLInputElement | HTMLSelectElement {
	return [...root.querySelectorAll('label')]
		.find((item) => item.firstChild?.textContent?.trim() === label)!
		.querySelector('input,select')!;
}
function change(root: HTMLElement, label: string, value: string): void {
	const element = field(root, label);
	element.value = value;
	element.dispatchEvent(new Event('input'));
	if (element instanceof HTMLSelectElement) element.dispatchEvent(new Event('change'));
}

describe('state-machine authoring', () => {
	it('accepts the supported native descriptor and rejects changed definition limits', () => {
		expect(validMachineDescriptor(MACHINE_DESCRIPTOR)).toBe(true);
		expect(validMachineDescriptor({...MACHINE_DESCRIPTOR, max_states: 64})).toBe(false);
		expect(validMachineDescriptor({...MACHINE_DESCRIPTOR, signals: ['invented']})).toBe(false);
	});
	it('rejects ambiguous transitions, missing destinations, invalid timers and duplicate sensor IDs', () => {
		const base = defaultMachine();
		const machine = (transitions: unknown[]) => ({
			...base,
			states: [{ ...base.states[0], transitions }],
		});
		expect(
			machineError(
				machine([{ when: { kind: 'signal', signal: 'activated', source: null }, next: 'missing' }]),
			),
		).toContain('destinations');
		expect(
			machineError(machine([{ when: { kind: 'after_ticks', ticks: 0 }, next: 'ready' }])),
		).toContain('Timers');
		expect(
			machineError(
				machine([
					{ when: { kind: 'signal', signal: 'activated', source: null }, next: 'ready' },
					{
						when: {
							kind: 'signal',
							signal: 'activated',
							source: { entity: 'door', component: 'logic' },
						},
						next: 'ready',
					},
				]),
			),
		).toContain('overlapping');
		expect(
			machineError({
				...base,
				inputs: [
					{ id: 'touch', actor: 'actor', sensor: 'sensor', signal: 'activated' },
					{ id: 'touch', actor: 'actor', sensor: 'sensor', signal: 'reset' },
				],
			}),
		).toContain('unique');
		expect(
			machineError(machine([{ when: { kind: 'after_ticks', ticks: 1 }, next: 'ready' }])),
		).toBeNull();
	});
	it('keeps draft tables isolated and rejects references removed before Apply', () => {
		const draft = componentDraft('native_behavior', 'logic', 'door');
		draft.behavior_kind = 'state_machine';
		draft.machine = {
			...defaultMachine(),
			inputs: [{ id: 'touch', actor: 'actor', sensor: 'sensor', signal: 'activated' }],
		};
		expect(() => componentFromDraft(draft, box, [{ key: 'actor', name: 'Actor' }], [])).toThrow(
			'existing sensor',
		);
		const record = componentFromDraft(
			draft,
			box,
			[
				{ key: 'actor', name: 'Actor' },
				{ key: 'sensor', name: 'Sensor' },
			],
			[],
		);
		expect(validComponentViews([record])).toBe(true);
		if (record.data.kind !== 'native_behavior' || record.data.behavior.kind !== 'state_machine')
			throw new Error('Wrong component');
		expect(record.data.behavior.definition).not.toBe(draft.machine);
	});
	it('edits a table locally, emits one complete Apply, and discards without another command', async () => {
		const fixture = TestBed.createComponent(ComponentEditor);
		fixture.componentRef.setInput('types', types);
		fixture.componentRef.setInput('editable', true);
		fixture.componentRef.setInput('ownerKey', 'door');
		fixture.componentRef.setInput('entities', [{ key: 'door', name: 'Door' }]);
		await fixture.whenStable();
		const root: HTMLElement = fixture.nativeElement;
		const intents: ComponentEditIntent[] = [];
		fixture.componentInstance.actionRequested.subscribe((event) => intents.push(event));
		button(root, 'Add Component').click();
		await fixture.whenStable();
		change(root, 'Native behavior', 'state_machine');
		await fixture.whenStable();
		change(root, 'State ID', 'closed');
		await fixture.whenStable();
		button(root, 'Add transition').click();
		await fixture.whenStable();
		change(root, 'When', 'after_ticks');
		await fixture.whenStable();
		change(root, 'Delay (ticks)', '180');
		await fixture.whenStable();
		expect(intents).toHaveLength(0);
		const report = await axe.run(root, { rules: { 'color-contrast': { enabled: false } } });
		expect(report.violations.map((item) => item.id)).toEqual([]);
		button(root, 'Apply Component').click();
		expect(intents).toHaveLength(1);
		const intent = intents[0];
		if (
			intent.kind !== 'add' ||
			intent.component.data.kind !== 'native_behavior' ||
			intent.component.data.behavior.kind !== 'state_machine'
		)
			throw new Error('Expected machine Add');
		const definition: MachineDefinition = intent.component.data.behavior.definition;
		expect(definition.initial).toBe('closed');
		expect(definition.states[0].transitions).toEqual([
			{ when: { kind: 'after_ticks', ticks: 180 }, next: 'closed' },
		]);
		button(root, 'Discard Component Draft').click();
		await fixture.whenStable();
		expect(root.querySelector('editor-machine-editor')).toBeNull();
		expect(intents).toHaveLength(1);
	});
});

describe('character-controller authoring', () => {
	it('validates finite tuning and integer windows, preserves disabled settings, and rejects a second singleton', () => {
		const draft = componentDraft('character_controller', 'controller', 'actor');
		draft.enabled = false;
		const record = componentFromDraft(draft, box, [], []);
		expect(record.enabled).toBe(false);
		expect(validComponentViews([record])).toBe(true);
		expect(validComponentViews([record, { ...record, id: 'controller2' }])).toBe(false);
		draft.controller.gravity = 'NaN';
		expect(() => componentFromDraft(draft, box, [], [])).toThrow('finite');
		draft.controller.gravity = '20';
		draft.controller.coyote_ticks = '1.5';
		expect(() => componentFromDraft(draft, box, [], [])).toThrow('whole number');
	});
	it('shows the root/box contract and authors named jump bindings without silently changing input', async () => {
		const fixture = TestBed.createComponent(ComponentEditor);
		fixture.componentRef.setInput('types', [types[1]]);
		fixture.componentRef.setInput('editable', true);
		await fixture.whenStable();
		const root: HTMLElement = fixture.nativeElement;
		const intents: ComponentEditIntent[] = [];
		fixture.componentInstance.actionRequested.subscribe((event) => intents.push(event));
		button(root, 'Add Component').click();
		await fixture.whenStable();
		expect(root.textContent).toContain('Unpack and detachment');
		change(root, 'Jump action ID', 'vault');
		await fixture.whenStable();
		button(root, 'Apply Component').click();
		expect(intents).toHaveLength(1);
		const intent = intents[0];
		if (intent.kind !== 'add' || intent.component.data.kind !== 'character_controller')
			throw new Error('Expected controller Add');
		expect(intent.component.data.settings.jump_action).toBe('vault');
	});
});
