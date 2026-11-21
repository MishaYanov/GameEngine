import { signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import axe from 'axe-core';
import { vi } from 'vitest';
import type {
	ComponentRecord,
	ComponentTypeDescriptor,
} from '../../../../core/editor/editor-components';
import {
	validComponentTypes,
	validComponentViews,
} from '../../../../core/editor/editor-components';
import { EditorService } from '../../../../core/editor/editor.service';
import type { EditorCommand, SceneEntity } from '../../../../core/editor/editor.models';
import { SceneEditingService } from '../scene-editing.service';
import { componentDraft, componentFromDraft } from './component-draft';
import { ComponentEditor, type ComponentEditIntent } from './component-editor';
import { SceneComponentInspector } from './scene-component-inspector';

const types: readonly ComponentTypeDescriptor[] = [
	{
		type_id: 'engine.mesh_renderer',
		kind: 'mesh_renderer',
		label: 'Mesh Renderer',
		description: 'Visible model.',
		max_per_object: 1,
	},
	{
		type_id: 'engine.collider',
		kind: 'collider',
		label: 'Collider',
		description: 'Shape and interaction.',
		max_per_object: 1,
	},
	{
		type_id: 'engine.native_behavior',
		kind: 'native_behavior',
		label: 'Native Behavior',
		description: 'Independent state.',
		max_per_object: 8,
	},
];
const mesh: ComponentRecord = {
	id: 'mesh',
	schema_version: 1,
	enabled: true,
	data: { kind: 'mesh_renderer', model_path: 'actor.glb', asset_id: 'original-id', material: null },
};
const targets = [
	{ key: 'actor', name: 'Player' },
	{ key: 'sensor', name: 'Sensor' },
	{ key: 'door', name: 'Door' },
];
const box = {
	kind: 'box' as const,
	half_extents: [0.5, 0.5, 0.5] as const,
	offset: [0, 0, 0] as const,
};

function controls(root: HTMLElement) {
	const button = (text: string) =>
		[...root.querySelectorAll('button')].find((element) => element.textContent.trim() === text)!;
	const field = (label: string) => {
		const element = [...root.querySelectorAll('label')].find(
			(element) => element.firstChild?.textContent?.trim() === label,
		);
        return element!.htmlFor
            ? root.querySelector<HTMLInputElement>(`#${element!.htmlFor}`)!
            : element!.querySelector<HTMLInputElement | HTMLSelectElement>('input,select')!;
	};
	const change = (label: string, value: string) => {
		const input = field(label);
		input.value = value;
		input.dispatchEvent(new Event('input'));
		if (input instanceof HTMLSelectElement) input.dispatchEvent(new Event('change'));
        if (input.getAttribute('role') === 'combobox') input.dispatchEvent(new KeyboardEvent('keydown', {key: 'Enter'}));
	};
	return { button, field, change };
}

describe('ComponentEditor', () => {
	async function setup() {
		const fixture = TestBed.createComponent(ComponentEditor);
		fixture.componentRef.setInput('components', [mesh]);
		fixture.componentRef.setInput('types', types);
		fixture.componentRef.setInput('entities', targets);
		fixture.componentRef.setInput('ownerKey', 'door');
		fixture.componentRef.setInput('editable', true);
		fixture.componentRef.setInput('contextKey', 'workspace:scene:door');
		await fixture.whenStable();
		const intents: ComponentEditIntent[] = [];
		fixture.componentInstance.actionRequested.subscribe((intent) => intents.push(intent));
		const root: HTMLElement = fixture.nativeElement;
		return { fixture, root, intents, ...controls(root) };
	}

	it('replaces a reviewed mesh without optimistic state or losing its asset identity', async () => {
		const panel = await setup();
		panel.button('Edit').click();
		await panel.fixture.whenStable();
		panel.change('Mesh index', '2');
		await panel.fixture.whenStable();
		panel.button('Apply Component').click();
		expect(panel.intents).toEqual([
			{
				kind: 'replace',
				expected: JSON.stringify(mesh),
				component: {
					...mesh,
					data: { ...mesh.data, mesh_index: 2 },
				},
			},
		]);
		expect(panel.root.textContent).toContain('Discard Component Draft');
		expect(mesh.data.kind === 'mesh_renderer' && mesh.data.mesh_index).toBeUndefined();
		panel.fixture.componentRef.setInput('acknowledged', 1);
		await panel.fixture.whenStable();
		expect(panel.root.textContent).not.toContain('Discard Component Draft');
	});

	it('authors a separate collectible instance through typed entity references', async () => {
		const panel = await setup();
        panel.change('Component type', 'native independent');
        await panel.fixture.whenStable();
        expect(panel.field('Component type').value).toBe('Native Behavior');
		panel.button('Add Component').click();
		await panel.fixture.whenStable();
		panel.change('Native behavior', 'collectible');
		await panel.fixture.whenStable();
		panel.change('Actor', 'actor');
		panel.change('Trigger sensor', 'sensor');
		panel.change('Visual', 'door');
		await panel.fixture.whenStable();
		const report = await axe.run(panel.root, { rules: { 'color-contrast': { enabled: false } } });
		expect(report.violations.map((violation) => violation.id)).toEqual([]);
		panel.button('Apply Component').click();
		const intent = panel.intents[0];
		expect(intent.kind).toBe('add');
		if (intent.kind !== 'add') throw new Error('Expected new component');
		expect(intent.component.id).toMatch(/^[0-9a-f-]{36}$/);
		expect(intent.component.data).toEqual({
			kind: 'native_behavior',
			behavior: {
				kind: 'collectible',
				actor: 'actor',
				trigger: 'sensor',
				visual: 'door',
				despawn_owner: false,
			},
		});
	});

	it('emits enable/remove intents and makes every component action read-only when unavailable', async () => {
		const panel = await setup();
		panel.button('Disable').click();
		panel.button('Remove').click();
		expect(panel.intents).toEqual([
			{ kind: 'enabled', component_id: 'mesh', enabled: false },
			{ kind: 'remove', component_id: 'mesh', expected: JSON.stringify(mesh) },
		]);
		panel.fixture.componentRef.setInput('editable', false);
		await panel.fixture.whenStable();
		expect(panel.button('Edit').disabled).toBe(true);
		expect(panel.button('Remove').disabled).toBe(true);
	});

    it('opens a referenced behavior at its exact field and preserves a different draft', async () => {
        const panel = await setup();
        const collectible: ComponentRecord = {
            id: 'collectible', schema_version: 1, enabled: true,
            data: {
                kind: 'native_behavior', behavior: {
                    kind: 'collectible', actor: 'actor', trigger: 'sensor', visual: null, despawn_owner: false,
                }
            },
        };
        panel.fixture.componentRef.setInput('components', [mesh, collectible]);
        await panel.fixture.whenStable();
        panel.button('Edit').click();
        await panel.fixture.whenStable();
        panel.fixture.componentInstance.revealReference(collectible.id, 'Trigger');
        await panel.fixture.whenStable();
        expect(document.activeElement?.getAttribute('data-reference-component')).toBe(collectible.id);
        expect(panel.field('Model path')).toBeTruthy();

        panel.button('Discard Component Draft').click();
        await panel.fixture.whenStable();
        panel.fixture.componentInstance.revealReference(collectible.id, 'Trigger');
        await panel.fixture.whenStable();
        expect(document.activeElement).toBe(panel.field('Trigger sensor'));
        expect(panel.intents).toEqual([]);
    });

	it('discards local drafts when the same entity key belongs to a replacement scene', async () => {
		const panel = await setup();
		panel.button('Edit').click();
		await panel.fixture.whenStable();
		panel.change('Model path', 'unaccepted.glb');
		panel.fixture.componentRef.setInput('contextKey', 'workspace:other-scene:door');
		await panel.fixture.whenStable();
		expect(panel.root.textContent).not.toContain('Discard Component Draft');
		expect(panel.intents).toEqual([]);
	});

	it('validates spawner budgets and typed slots without inherited-property loss', () => {
		const draft = componentDraft('native_behavior', 'spawner', 'door');
		draft.behavior_kind = 'timed_spawner';
		draft.prefab_path = 'pickup.json';
		draft.bindings = [{ name: '__proto__', target: 'actor' }];
		const value = componentFromDraft(draft, box, targets, []);
		if (value.data.kind !== 'native_behavior' || value.data.behavior.kind !== 'timed_spawner')
			throw new Error('Expected spawner');
		expect(Object.hasOwn(value.data.behavior.bindings, '__proto__')).toBe(true);
		expect(value.data.behavior.bindings['__proto__']).toBe('actor');
		expect(() => componentFromDraft({ ...draft, max_alive: '129' }, box, targets, [])).toThrow(
			'Maximum alive',
		);
		expect(() => componentFromDraft({ ...draft, interval_ticks: '1e2' }, box, targets, [])).toThrow(
			'whole number',
		);
		expect(() =>
			componentFromDraft(
				{ ...draft, spawn_offset: { x: '1e-99', y: '0', z: '0' } },
				box,
				targets,
				[],
			),
		).toThrow('finite');
		expect(() =>
			componentFromDraft(
				{ ...draft, bindings: [...draft.bindings, ...draft.bindings] },
				box,
				targets,
				[],
			),
		).toThrow('unique');
		expect(() =>
			componentFromDraft(
				{ ...draft, bindings: [{ name: 'actor', target: 'missing' }] },
				box,
				targets,
				[],
			),
		).toThrow('existing');
	});

	it('validates native metadata and per-entity component projection shape', () => {
		expect(validComponentTypes(types)).toBe(true);
		expect(validComponentTypes([...types, types[0]])).toBe(false);
		expect(validComponentTypes([{ ...types[0], max_per_object: 0 }])).toBe(false);
		expect(validComponentViews([mesh])).toBe(true);
		expect(validComponentViews([mesh, mesh])).toBe(false);
		expect(validComponentViews([{ ...mesh, schema_version: 2 }])).toBe(false);
		expect(validComponentViews([{ ...mesh, data: { kind: 'unknown' } }])).toBe(false);
	});
});

describe('SceneComponentInspector', () => {
	async function setup() {
		const entity: SceneEntity = {
			key: 'door',
			name: 'Door',
			model_path: 'actor.glb',
			renderable: true,
			transform: { position: [0, 0, 0], rotation: [0, 0, 0], scale: [1, 1, 1] },
			component_views: [mesh],
		};
		const snapshot = signal({
			revision: 4,
			active_scene_id: 'main',
			mode: 'edit',
			document: { entities: [entity] },
		});
		const generation = signal(1);
		const editingError = signal<string | null>(null);
		const execute = vi.fn(async (_command: EditorCommand, _generation: number, _revision: number) =>
			snapshot(),
		);
		const commit = vi.fn(async (action: () => Promise<void>) => {
			try {
				await action();
				return true;
			} catch (error) {
				editingError.set(String(error));
				return false;
			}
		});
		TestBed.configureTestingModule({
			providers: [
				{
					provide: EditorService,
					useValue: {
						snapshot,
						workspaceGeneration: generation,
						execute,
						capabilities: () => ({ component_types: types }),
						modelAssets: () => [],
					},
				},
				{
					provide: SceneEditingService,
					useValue: { withCommittedEdit: commit, error: editingError },
				},
			],
		});
		const fixture = TestBed.createComponent(SceneComponentInspector);
		fixture.componentRef.setInput('entity', entity);
		fixture.componentRef.setInput('editable', true);
		await fixture.whenStable();
		const root: HTMLElement = fixture.nativeElement;
		return { fixture, root, snapshot, generation, execute, commit, ...controls(root) };
	}

	it('finishes pending edits before capturing the command revision', async () => {
		const panel = await setup();
		panel.commit.mockImplementationOnce(async (action) => {
			panel.snapshot.update((value) => ({ ...value, revision: 9 }));
			await action();
			return true;
		});
		panel.button('Disable').click();
		await panel.fixture.whenStable();
		expect(panel.execute).toHaveBeenCalledExactlyOnceWith(
			{ type: 'set_component_enabled', key: 'door', component_id: 'mesh', enabled: false },
			1,
			9,
		);
	});

	it('refuses a stale full component draft after a concurrent component change', async () => {
		const panel = await setup();
		panel.button('Edit').click();
		await panel.fixture.whenStable();
		panel.change('Mesh index', '2');
		await panel.fixture.whenStable();
		panel.commit.mockImplementationOnce(async (action) => {
			panel.snapshot.update((value) => ({
				...value,
				revision: 9,
				document: {
					entities: [
						{ ...value.document.entities[0], component_views: [{ ...mesh, enabled: false }] },
					],
				},
			}));
			try {
				await action();
				return true;
			} catch {
				return false;
			}
		});
		panel.button('Apply Component').click();
		await panel.fixture.whenStable();
		expect(panel.execute).not.toHaveBeenCalled();
		expect(panel.root.querySelector('[role="alert"]')).not.toBeNull();
	});

	it('does not target a replacement workspace when the pending commit finishes late', async () => {
		const panel = await setup();
		panel.commit.mockImplementationOnce(async (action) => {
			panel.generation.set(2);
			await action();
			return true;
		});
		panel.button('Disable').click();
		await panel.fixture.whenStable();
		expect(panel.execute).not.toHaveBeenCalled();
		expect(panel.root.querySelector('[role="alert"]')).toBeNull();
	});
});
