import { signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import axe from 'axe-core';
import { vi } from 'vitest';
import { EditorService } from '../../../../core/editor/editor.service';
import type {
	PrefabSourceDocument,
	PrefabSourceDraft,
	LinkedPrefabStatus,
} from '../../../../core/editor/linked-prefabs.models';
import type { SceneEntity } from '../../../../core/editor/editor.models';
import { LAYOUT_PREFERENCES_STORAGE } from '../../../../shared/services/layout-preferences-storage';
import { SceneEditingService } from '../scene-editing.service';
import { PrefabAuthoring } from './prefab-authoring';
import { PrefabSourceEditor } from './prefab-source-editor';
import { LinkedPrefabInspector } from './linked-prefab-inspector';
import { entityDraft, sourceEntityFromDraft, slotValues } from './prefab-source-values';

const prefab: PrefabSourceDocument = {
	schema_version: 3,
	gameplay: null,
	reference_slots: { Actor: 'external-player' },
	entities: [
		{
			key: 'source-door',
			name: 'Door',
			parent: null,
			transform: { position: [0, 0, 0], rotation: [0, 0, 0], scale: [1, 1, 1] },
			components: [],
		},
		{
			key: 'source-sensor',
			name: 'Sensor',
			parent: null,
			transform: { position: [1, 0, 0], rotation: [0, 0, 0], scale: [1, 1, 1] },
			components: [],
		},
	],
};
const source: PrefabSourceDraft = {
	draft_id: 'draft-1',
	workspace_generation: 101,
	revision: 4,
	path: 'door.json',
	asset_id: 'source-id',
	fingerprint: 'original',
	prefab,
};
const linked: LinkedPrefabStatus = {
	root_key: 'anchor',
	source_id: 'source-id',
	source_path: 'door.json',
	source_fingerprint: 'original',
	source_changed: true,
	source_error: null,
	bindings: { Actor: 'player' },
	overrides: [
		{
			target: { source_entity: 'source-door', property: { kind: 'name' } },
			base: { kind: 'name', value: 'Door' },
			value: { kind: 'name', value: 'Custom door' },
		},
	],
};
const entity: SceneEntity = {
	key: 'scene-door',
	name: 'Custom door',
	model_path: 'door.glb',
	renderable: true,
	transform: { position: [0, 0, 0], rotation: [0, 0, 0], scale: [1, 1, 1] },
	component_views: [],
};

function configure() {
	const current = signal({
		revision: 4,
		active_scene_id: 'main',
		mode: 'edit',
		selection: ['scene-door'],
		transaction: null,
		document: { entities: [entity, { ...entity, key: 'player', name: 'Player' }] },
	});
	const generation = signal(1);
	const editingError = signal<string | null>(null);
	const commit = vi.fn(async (action: () => Promise<void>) => {
		try {
			await action();
			return true;
		} catch (error) {
			editingError.set(error instanceof Error ? error.message : String(error));
			return false;
		}
	});
	const openPrefabSource = vi.fn(async () => structuredClone(source));
	const closePrefabSource = vi.fn(async () => undefined);
	const validatePrefabSource = vi.fn(async () => ({
		draft_id: 'draft-1',
		entity_count: 2,
		bytes: 256,
	}));
	const savePrefabSource = vi.fn(
		async (_id: string, value: PrefabSourceDocument, _generation: number, _revision: number) => ({
			draft: { ...source, fingerprint: 'saved', prefab: value },
			snapshot: current(),
		}),
	);
	const createLinkedPrefabSource = vi.fn(async () => ({ ...source, revision: current().revision }));
	const instantiateLinkedPrefab = vi.fn(async () => current());
	const linkedPrefabStatus = vi.fn(async (): Promise<LinkedPrefabStatus | null> =>
		structuredClone(linked),
	);
	const updateLinkedPrefab = vi.fn(async () => current());
	const execute = vi.fn(async () => current());
	const assets = vi.fn(async () => []);
	TestBed.configureTestingModule({
		providers: [
			{
				provide: LAYOUT_PREFERENCES_STORAGE,
				useValue: { load: async () => null, save: async () => undefined },
			},
			{
				provide: EditorService,
				useValue: {
					snapshot: current,
					workspaceGeneration: generation,
					busy: () => false,
					capabilities: () => ({
						component_types: [
							{
								type_id: 'engine.native_behavior',
								kind: 'native_behavior',
								label: 'Native Behavior',
								description: 'State',
								max_per_object: 8,
							},
						],
					}),
					modelAssets: () => [],
					openPrefabSource,
					closePrefabSource,
					validatePrefabSource,
					savePrefabSource,
					createLinkedPrefabSource,
					instantiateLinkedPrefab,
					linkedPrefabStatus,
					updateLinkedPrefab,
					execute,
					assets,
				},
			},
			{
				provide: SceneEditingService,
				useValue: { ready: () => true, withCommittedEdit: commit, error: editingError },
			},
		],
	});
	return {
		current,
		generation,
		commit,
		editingError,
		openPrefabSource,
		closePrefabSource,
		validatePrefabSource,
		savePrefabSource,
		createLinkedPrefabSource,
		instantiateLinkedPrefab,
		linkedPrefabStatus,
		updateLinkedPrefab,
		execute,
		assets,
	};
}
function controls(root: HTMLElement) {
	const button = (text: string) =>
		[...root.querySelectorAll('button')].find((button) => button.textContent.trim() === text)!;
    const field = (text: string) => {
        const label = [...root.querySelectorAll('label')]
            .find((label) => label.firstChild?.textContent?.trim() === text)!;
        return label.htmlFor
            ? root.querySelector<HTMLInputElement>(`#${label.htmlFor}`)!
            : label.querySelector<HTMLInputElement | HTMLSelectElement>('input,select')!;
    };
	const change = (label: string, value: string) => {
		const input = field(label);
		input.value = value;
		input.dispatchEvent(new Event('input'));
		input.dispatchEvent(new Event('change'));
        if (input.getAttribute('role') === 'combobox') input.dispatchEvent(new KeyboardEvent('keydown', {key: 'Enter'}));
	};
	return { button, field, change };
}

describe('typed prefab authoring', () => {
	const prototype = HTMLDialogElement.prototype;
	const show = Object.getOwnPropertyDescriptor(prototype, 'showModal');
	const close = Object.getOwnPropertyDescriptor(prototype, 'close');
	beforeAll(() => {
		Object.defineProperty(prototype, 'showModal', {
			configurable: true,
			value(this: HTMLDialogElement) {
				this.setAttribute('open', '');
			},
		});
		Object.defineProperty(prototype, 'close', {
			configurable: true,
			value(this: HTMLDialogElement) {
				this.removeAttribute('open');
				this.dispatchEvent(new Event('close'));
			},
		});
	});
	afterAll(() => {
		if (show) Object.defineProperty(prototype, 'showModal', show);
		else Reflect.deleteProperty(prototype, 'showModal');
		if (close) Object.defineProperty(prototype, 'close', close);
		else Reflect.deleteProperty(prototype, 'close');
	});

	it('keeps typed source edits isolated across object selection and saves the canonical draft', async () => {
		const backend = configure();
		const fixture = TestBed.createComponent(PrefabSourceEditor);
		fixture.componentRef.setInput('source', structuredClone(source));
		fixture.componentInstance.saved.subscribe((value) =>
			fixture.componentRef.setInput('source', value),
		);
		await fixture.whenStable();
		const root: HTMLElement = fixture.nativeElement,
			ui = controls(root);
		ui.change('Name', 'Edited source door');
		await fixture.whenStable();
		expect(ui.button('Save Prefab Source').disabled).toBe(true);
		ui.button('Apply Source Object Draft').click();
		await fixture.whenStable();
		ui.button('Sensor').click();
		await fixture.whenStable();
		ui.button('Edited source door').click();
		await fixture.whenStable();
		expect(ui.field('Name').value).toBe('Edited source door');
		expect(source.prefab.entities[0].name).toBe('Door');
		const report = await axe.run(root, { rules: { 'color-contrast': { enabled: false } } });
		expect(report.violations.map((violation) => violation.id)).toEqual([]);
		ui.button('Save Prefab Source').click();
		await fixture.whenStable();
		expect(backend.savePrefabSource).toHaveBeenCalledWith(
			'draft-1',
			expect.objectContaining({
				schema_version: 3,
				entities: expect.arrayContaining([
					expect.objectContaining({ key: 'source-door', name: 'Edited source door' }),
				]),
			}),
			1,
			4,
		);
		expect(backend.assets).toHaveBeenCalledWith(true);
		expect(root.textContent).not.toContain('Unsaved source changes');
	});

	it('guards dirty source closure until changes are explicitly discarded', async () => {
		configure();
		const fixture = TestBed.createComponent(PrefabSourceEditor);
		fixture.componentRef.setInput('source', source);
		const closed = vi.fn();
		fixture.componentInstance.closed.subscribe(closed);
		await fixture.whenStable();
		const root: HTMLElement = fixture.nativeElement,
			ui = controls(root);
		ui.change('Name', 'Applied but not saved');
		await fixture.whenStable();
		ui.button('Apply Source Object Draft').click();
		await fixture.whenStable();
		expect(root.textContent).toContain('Unsaved source changes');
		const dialog = root.querySelector('dialog')!;
		dialog.dispatchEvent(new Event('cancel', { cancelable: true }));
		await fixture.whenStable();
		expect(dialog.open).toBe(true);
		expect(closed).not.toHaveBeenCalled();
		expect(root.textContent).toContain('Save the source changes or choose');
		ui.button('Discard Source Changes and Close').click();
		await fixture.whenStable();
		expect(closed).toHaveBeenCalledTimes(1);
	});

	it('keeps a clean source open while a native validation request is pending', async () => {
		const backend = configure();
		let resolve!: (value: { draft_id: string; entity_count: number; bytes: number }) => void;
		backend.validatePrefabSource.mockImplementationOnce(
			() => new Promise((done) => (resolve = done)),
		);
		const fixture = TestBed.createComponent(PrefabSourceEditor);
		fixture.componentRef.setInput('source', source);
		const closed = vi.fn();
		fixture.componentInstance.closed.subscribe(closed);
		await fixture.whenStable();
		const root: HTMLElement = fixture.nativeElement,
			ui = controls(root);
		ui.button('Validate Prefab Source').click();
		await Promise.resolve();
		fixture.detectChanges();
		root.querySelector<HTMLButtonElement>('button[aria-label="Close Prefab Source"]')!.click();
		expect(closed).not.toHaveBeenCalled();
		resolve({ draft_id: 'draft-1', entity_count: 2, bytes: 256 });
		await fixture.whenStable();
		expect(root.textContent).toContain('Valid source');
	});

	it('preserves an edited source after fingerprint rejection and drops feedback after replacement', async () => {
		const backend = configure();
		const fixture = TestBed.createComponent(PrefabSourceEditor);
		fixture.componentRef.setInput('source', source);
		await fixture.whenStable();
		const root: HTMLElement = fixture.nativeElement,
			ui = controls(root);
		ui.change('Name', 'Unpublished');
		await fixture.whenStable();
		ui.button('Apply Source Object Draft').click();
		await fixture.whenStable();
		backend.savePrefabSource.mockRejectedValueOnce(
			new Error('Source fingerprint changed; reopen it.'),
		);
		ui.button('Save Prefab Source').click();
		await fixture.whenStable();
		expect(root.querySelector('[role="alert"]')?.textContent).toContain('fingerprint changed');
		expect(backend.editingError()).toBeNull();
		expect(ui.field('Name').value).toBe('Unpublished');
		backend.commit.mockImplementationOnce(async (action) => {
			backend.generation.set(2);
			await action();
			return true;
		});
		ui.button('Save Prefab Source').click();
		await fixture.whenStable();
		expect(backend.savePrefabSource).toHaveBeenCalledTimes(1);
	});

	it('creates a component source with explicit external slots after the current Inspector edit', async () => {
		const backend = configure();
		const fixture = TestBed.createComponent(PrefabAuthoring);
		await fixture.whenStable();
		const root: HTMLElement = fixture.nativeElement,
			ui = controls(root);
		ui.button('Create Linked Prefab Source…').click();
		await fixture.whenStable();
		ui.change('Source path', 'assembly.json');
		ui.button('Add External Reference Slot').click();
		await fixture.whenStable();
		ui.change('Reference slot name', 'Actor');
		ui.change('External scene reference', 'player');
		backend.commit.mockImplementationOnce(async (action) => {
			backend.current.update((value) => ({ ...value, revision: 12 }));
			await action();
			return true;
		});
		ui.button('Create Component Source').click();
		await fixture.whenStable();
		expect(backend.createLinkedPrefabSource).toHaveBeenCalledWith(
			'assembly.json',
			{ Actor: 'player' },
			1,
			12,
		);
		expect(root.textContent).toContain('Edit this source independently');
	});

	it('requires reference bindings for a link and retains its reviewed revision', async () => {
		const backend = configure();
		const fixture = TestBed.createComponent(PrefabAuthoring);
		fixture.componentRef.setInput('selected', { name: 'Door', path: 'door.json', kind: 'prefab' });
		await fixture.whenStable();
		const root: HTMLElement = fixture.nativeElement,
			ui = controls(root);
		ui.button('Link Prefab…').click();
		await fixture.whenStable();
		expect(ui.button('Instantiate Linked Prefab').disabled).toBe(true);
		ui.change('Actor', 'player');
		await fixture.whenStable();
		backend.current.update((value) => ({ ...value, revision: 9 }));
		backend.instantiateLinkedPrefab.mockRejectedValueOnce(new Error('Authoring revision changed'));
		ui.button('Instantiate Linked Prefab').click();
		await fixture.whenStable();
		expect(backend.instantiateLinkedPrefab).toHaveBeenCalledWith(
			'door.json',
			'draft-1',
			[0, 0, 0],
			{ Actor: 'player' },
			1,
			4,
		);
		expect(root.querySelector('[role="alert"]')?.textContent).toContain('revision changed');
        expect(ui.field('Actor').value).toBe('Player');
	});

	it('closes a late native draft instead of opening it in another workspace', async () => {
		const backend = configure();
		let resolve!: (value: PrefabSourceDraft) => void;
		backend.openPrefabSource.mockImplementationOnce(() => new Promise((done) => (resolve = done)));
		const fixture = TestBed.createComponent(PrefabAuthoring);
		fixture.componentRef.setInput('selected', { name: 'Door', path: 'door.json', kind: 'prefab' });
		await fixture.whenStable();
		const root: HTMLElement = fixture.nativeElement,
			ui = controls(root);
		ui.button('Edit Prefab Source…').click();
		await Promise.resolve();
		backend.generation.set(2);
		fixture.detectChanges();
		resolve(source);
		await fixture.whenStable();
		expect(backend.closePrefabSource).toHaveBeenCalledWith('draft-1');
		expect(root.textContent).not.toContain('Edit this source independently');
	});

	it('shows typed overrides and routes property revert/unpack through undoable commands', async () => {
		const backend = configure();
		const fixture = TestBed.createComponent(LinkedPrefabInspector);
		fixture.componentRef.setInput('entity', entity);
		fixture.componentRef.setInput('editable', true);
		await fixture.whenStable();
		const root: HTMLElement = fixture.nativeElement,
			ui = controls(root);
		expect(root.textContent).toContain('Custom door');
		expect(root.textContent).toContain('Source changed');
		ui.button('Revert Property').click();
		await fixture.whenStable();
		expect(backend.execute).toHaveBeenCalledWith(
			{ type: 'revert_linked_prefab', root_key: 'anchor', property: linked.overrides[0].target },
			1,
			4,
		);
		ui.button('Unpack Linked Prefab').click();
		await fixture.whenStable();
		expect(backend.execute).toHaveBeenCalledWith(
			{ type: 'unpack_linked_prefab', root_key: 'anchor' },
			1,
			4,
		);
	});

	it('keeps an update conflict reviewable and retries only the chosen resolution', async () => {
		const backend = configure();
		const fixture = TestBed.createComponent(LinkedPrefabInspector);
		fixture.componentRef.setInput('entity', entity);
		fixture.componentRef.setInput('editable', true);
		await fixture.whenStable();
		const root: HTMLElement = fixture.nativeElement,
			ui = controls(root);
		ui.button('Review Source Update…').click();
		await fixture.whenStable();
		backend.updateLinkedPrefab.mockRejectedValueOnce(
			new Error('Conflicting local and source changes'),
		);
		ui.button('Update Linked Instance').click();
		await fixture.whenStable();
		expect(backend.updateLinkedPrefab).toHaveBeenCalledWith(
			'anchor',
			'draft-1',
			'reject',
			{ Actor: 'player' },
			1,
			4,
		);
		expect(root.textContent).toContain('Conflicting local');
		expect(backend.editingError()).toBeNull();
		ui.change('Conflict resolution', 'keep_local');
		await fixture.whenStable();
		ui.button('Update Linked Instance').click();
		await fixture.whenStable();
		expect(backend.updateLinkedPrefab).toHaveBeenLastCalledWith(
			'anchor',
			'draft-1',
			'keep_local',
			{ Actor: 'player' },
			1,
			4,
		);
	});

	it('validates source hierarchy and special slot names without changing identities', () => {
		const draft = { ...entityDraft(prefab.entities[0]), parent: 'source-sensor' };
		expect(() =>
			sourceEntityFromDraft(draft, prefab.entities[0], [
				prefab.entities[0],
				{ ...prefab.entities[1], parent: 'source-door' },
			]),
		).toThrow('cycle');
		const slots = slotValues([{ name: '__proto__', key: 'external' }], prefab.entities);
		expect(Object.hasOwn(slots, '__proto__')).toBe(true);
		expect(slots['__proto__']).toBe('external');
		expect(() => slotValues([{ name: 'Actor', key: 'source-door' }], prefab.entities)).toThrow(
			'external',
		);
	});
});
