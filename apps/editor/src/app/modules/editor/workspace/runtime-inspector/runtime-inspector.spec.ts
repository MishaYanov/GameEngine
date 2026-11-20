import { signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import axe from 'axe-core';
import { vi } from 'vitest';
import { EditorService } from '../../../../core/editor/editor.service';
import { ViewportEditorService } from '../../../../core/editor/viewport-editor.service';
import type { PlayStatus } from '../../../../core/editor/viewport-editor.models';
import {
	readRuntimeInspection,
	type RuntimeInspection,
} from '../../../../core/editor/runtime-inspection';
import { LAYOUT_PREFERENCES_STORAGE } from '../../../../shared/services/layout-preferences-storage';
import { RuntimeInspector } from './runtime-inspector';

const status: PlayStatus = {
	play_session_id: 'play-1',
	run_id: 'run-1',
	scene_id: 'main',
	paused: false,
	pending_scene_id: null,
	gameplay: null,
};
const page: RuntimeInspection = {
	...status,
	tick: 12,
	topology_revision: 3,
	total_entities: 1,
	timer_count: 2,
	pending_spawns: 1,
	last_error: null,
	entities: [
		{
			handle: { world_id: 'world-1', key: 'spawned-key', generation: 1 },
			name: 'Spawned pickup',
			spawned: true,
			instance_id: 'instance-1',
			behavior_states: [
				{ component_id: 'collectible-1', kind: 'collectible', activated: true, elapsed_ticks: 8 },
			],
		},
	],
};

async function setup() {
	const gameStatus = signal<PlayStatus | null>(status);
	const generation = signal(1);
	const inspectRuntime = vi.fn(
		async (_offset: number, _limit: number): Promise<RuntimeInspection> => page,
	);
	TestBed.configureTestingModule({
		providers: [
			{
				provide: LAYOUT_PREFERENCES_STORAGE,
				useValue: { load: async () => null, save: async () => undefined },
			},
			{ provide: EditorService, useValue: { workspaceGeneration: generation, inspectRuntime } },
			{ provide: ViewportEditorService, useValue: { gameStatus } },
		],
	});
	const fixture = TestBed.createComponent(RuntimeInspector);
	await fixture.whenStable();
	const root: HTMLElement = fixture.nativeElement;
	const button = (label: string) =>
		[...root.querySelectorAll('button')].find((button) => button.textContent.trim() === label)!;
	return { fixture, root, button, inspectRuntime, gameStatus, generation };
}

describe('RuntimeInspector', () => {
	it('only requests a bounded read-only page on explicit refresh and exposes lifecycle state', async () => {
		const panel = await setup();
		expect(panel.inspectRuntime).not.toHaveBeenCalled();
		panel.button('Refresh Runtime').click();
		await panel.fixture.whenStable();
		expect(panel.inspectRuntime).toHaveBeenCalledExactlyOnceWith(0, 32);
		expect(panel.root.textContent).toContain('Snapshot at tick 12');
		expect(panel.root.textContent).toContain('Spawned during this run');
		expect(panel.root.textContent).toContain('Activated · 8 ticks');
		expect(panel.root.querySelector('input,select,textarea')).toBeNull();
		const report = await axe.run(panel.root, { rules: { 'color-contrast': { enabled: false } } });
		expect(report.violations.map((violation) => violation.id)).toEqual([]);
	});

	it('pages explicitly and returns to a previous bounded page', async () => {
		const panel = await setup();
		panel.inspectRuntime.mockResolvedValue({ ...page, total_entities: 65 });
		panel.button('Refresh Runtime').click();
		await panel.fixture.whenStable();
		panel.button('Next entities').click();
		await panel.fixture.whenStable();
		expect(panel.inspectRuntime).toHaveBeenLastCalledWith(32, 32);
		panel.button('Previous entities').click();
		await panel.fixture.whenStable();
		expect(panel.inspectRuntime).toHaveBeenLastCalledWith(0, 32);
	});

	it.each(['run', 'scene', 'stop', 'workspace'] as const)(
		'drops a late page after %s replacement',
		async (replacement) => {
			const panel = await setup();
			let resolve!: (value: RuntimeInspection) => void;
			panel.inspectRuntime.mockImplementationOnce(() => new Promise((done) => (resolve = done)));
			panel.button('Refresh Runtime').click();
			if (replacement === 'workspace') panel.generation.set(2);
			else
				panel.gameStatus.set(
					replacement === 'stop'
						? null
						: {
								...status,
								...(replacement === 'run' ? { run_id: 'run-2' } : { scene_id: 'finish' }),
							},
				);
			panel.fixture.detectChanges();
			resolve(page);
			await panel.fixture.whenStable();
			expect(panel.root.textContent).not.toContain('Snapshot at tick 12');
			expect(panel.root.textContent).not.toContain('Spawned pickup');
			expect(panel.root.querySelector('[role="alert"]')).toBeNull();
		},
	);

	it('keeps a query failure local and permits retrying without an authored snapshot refresh', async () => {
		const panel = await setup();
		panel.inspectRuntime.mockRejectedValueOnce(new Error('Runtime inspection unavailable'));
		panel.button('Refresh Runtime').click();
		await panel.fixture.whenStable();
		expect(panel.root.querySelector('[role="alert"]')?.textContent).toContain(
			'inspection unavailable',
		);
		panel.button('Refresh Runtime').click();
		await panel.fixture.whenStable();
		expect(panel.root.querySelector('[role="alert"]')).toBeNull();
		expect(panel.inspectRuntime).toHaveBeenCalledTimes(2);
	});

	it('rejects malformed, duplicated or over-sized native pages before rendering', () => {
		expect(readRuntimeInspection(page, 32)).toBe(page);
		for (const value of [
			{ ...page, entities: [page.entities[0], page.entities[0]] },
			{ ...page, tick: -1 },
			{
				...page,
				entities: [
					{ ...page.entities[0], handle: { ...page.entities[0].handle, generation: 0.5 } },
				],
			},
			{ ...page, entities: [{ ...page.entities[0], behavior_states: [{ kind: 'collectible' }] }] },
		])
			expect(() => readRuntimeInspection(value, 32)).toThrow('inspection page');
		expect(() => readRuntimeInspection(page, 0)).toThrow('inspection page');
	});
});
