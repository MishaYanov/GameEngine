import { signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import axe from 'axe-core';
import { vi } from 'vitest';
import { EditorService } from '../../../../core/editor/editor.service';
import type {
	PrefabSourceDocument,
	PrefabSourceDraft,
	LinkedPrefabStatus,
	LinkedPrefabOverrideReview,
	LinkedPrefabRecoveryReview,
} from '../../../../core/editor/linked-prefabs.models';
import type { ModelAsset, SceneEntity } from '../../../../core/editor/editor.models';
import { LAYOUT_PREFERENCES_STORAGE } from '../../../../shared/services/layout-preferences-storage';
import { SceneEditingService } from '../scene-editing.service';
import { PrefabAuthoring } from './prefab-authoring';
import { PrefabSourceEditor } from './prefab-source-editor';
import { LinkedPrefabInspector } from './linked-prefab-inspector';
import { PropAssetSelection } from './prop-asset-selection';
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
const replacement: PrefabSourceDraft = {
	...source, draft_id: 'replacement-draft', asset_id: 'replacement-id',
	path: 'replacement.json', fingerprint: 'b'.repeat(64),
};
const recoveryReview: LinkedPrefabRecoveryReview = {
	root_key: 'anchor', prior_source_id: source.asset_id, prior_path: source.path,
	prior_fingerprint: source.fingerprint, source_id: replacement.asset_id,
	source_path: replacement.path, source_fingerprint: replacement.fingerprint,
	member_count: 2, binding_count: 1, override_count: 1,
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
			address: { kind: 'object', placement_path: [], source_entity: 'source-door' },
			can_apply_to_root_source: true,
			base: { kind: 'name', value: 'Door' },
			value: { kind: 'name', value: 'Custom door' },
		},
	],
};
const reviewedChanges: readonly LinkedPrefabOverrideReview[] = [
	{
		target: linked.overrides[0].target,
		address: linked.overrides[0].address,
		base: linked.overrides[0].base,
		local: linked.overrides[0].value,
		incoming: { kind: 'name', value: 'New source door' },
		conflict: true,
	},
	{
		target: { source_entity: 'nested-left-key', property: { kind: 'transform_position' } },
		address: { kind: 'placement', placement_path: ['left'] },
		base: { kind: 'vector', value: [0, 0, 0] },
		local: { kind: 'vector', value: [2, 0, 0] },
		incoming: null,
		conflict: true,
	},
];
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
	const reviewLinkedPropOverrides = vi.fn(async () => structuredClone(reviewedChanges));
	const updateLinkedPropReviewed = vi.fn(async () => current());
	const applyLinkedPropOverride = vi.fn(async () => ({ draft: source, snapshot: current() }));
	const reviewLinkedPropRecovery = vi.fn(async () => structuredClone(recoveryReview));
	const recoverLinkedProp = vi.fn(async () => current());
	const execute = vi.fn(async () => current());
	const assets = vi.fn(async () => []);
	const modelAssets = signal<readonly ModelAsset[]>([]);
	TestBed.configureTestingModule({
		providers: [
			PropAssetSelection,
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
					modelAssets,
					openPrefabSource,
					closePrefabSource,
					validatePrefabSource,
					savePrefabSource,
					createLinkedPrefabSource,
					instantiateLinkedPrefab,
					linkedPrefabStatus,
					reviewLinkedPropOverrides,
					updateLinkedPropReviewed,
					applyLinkedPropOverride,
					reviewLinkedPropRecovery,
					recoverLinkedProp,
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
		reviewLinkedPropOverrides,
		updateLinkedPropReviewed,
		applyLinkedPropOverride,
		reviewLinkedPropRecovery,
		recoverLinkedProp,
		modelAssets,
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

	it('reviews the selected assembly after the current edit and saves its explicit pivot and slots', async () => {
		const backend = configure();
		const fixture = TestBed.createComponent(PrefabAuthoring);
		backend.current.update(value => ({
			...value,
			document: {entities: [...value.document.entities, {...entity, key: 'hinge', parent: entity.key, name: 'Hinge'}]},
		}));
		await fixture.whenStable();
		const root: HTMLElement = fixture.nativeElement,
			ui = controls(root);
		backend.commit.mockImplementationOnce(async (action) => {
			backend.current.update((value) => ({ ...value, revision: 12 }));
			await action();
			return true;
		});
		ui.button('Create Prop from Selection…').click();
		await fixture.whenStable();
		expect(root.textContent).toContain('2 included objects across 1 root.');
		expect(root.querySelector('details')?.textContent).toContain('Hinge');
		ui.change('Source path', 'assembly.json');
		ui.change('Pivot object', 'Hinge');
		ui.button('Add External Reference Slot').click();
		await fixture.whenStable();
		ui.change('Reference slot name', 'Actor');
		ui.change('External scene reference', 'player');
		ui.button('Create Prop Asset').click();
		await fixture.whenStable();
		expect(backend.createLinkedPrefabSource).toHaveBeenCalledWith(
			'assembly.json',
			{ Actor: 'player' },
			1,
			12,
			'hinge',
		);
		expect(root.textContent).toContain('Edit this source independently');
	});

	it('refuses to create a Prop from a scene revision that changed after review', async () => {
		const backend = configure();
		const fixture = TestBed.createComponent(PrefabAuthoring);
		await fixture.whenStable();
		const root: HTMLElement = fixture.nativeElement;
		const ui = controls(root);
		ui.button('Create Prop from Selection…').click();
		await fixture.whenStable();
		backend.current.update(value => ({...value, revision: 5}));
		ui.button('Create Prop Asset').click();
		await fixture.whenStable();
		expect(root.querySelector('[role="alert"]')?.textContent).toContain('scene or selection changed');
		expect(backend.createLinkedPrefabSource).not.toHaveBeenCalled();
	});

	it('closes a cancelled Prop review without creating a source or placing a scene instance', async () => {
		const backend = configure();
		const fixture = TestBed.createComponent(PrefabAuthoring);
		await fixture.whenStable();
		const root: HTMLElement = fixture.nativeElement;
		const ui = controls(root);
		ui.button('Create Prop from Selection…').click();
		await fixture.whenStable();
		root.querySelector<HTMLButtonElement>('button[aria-label="Close Create Prop from Selection"]')!.click();
		await fixture.whenStable();
		expect(backend.createLinkedPrefabSource).not.toHaveBeenCalled();
		expect(backend.instantiateLinkedPrefab).not.toHaveBeenCalled();
	});

	it('requires reference bindings for a link and retains its reviewed revision', async () => {
		const backend = configure();
		const fixture = TestBed.createComponent(PrefabAuthoring);
		fixture.componentRef.setInput('selected', { name: 'Door', path: 'door.json', kind: 'prefab' });
		await fixture.whenStable();
		const root: HTMLElement = fixture.nativeElement,
			ui = controls(root);
		ui.button('Place Linked Prop…').click();
		await fixture.whenStable();
		expect(ui.button('Place Linked Instance').disabled).toBe(true);
		ui.change('Actor', 'player');
		await fixture.whenStable();
		backend.current.update((value) => ({ ...value, revision: 9 }));
		backend.instantiateLinkedPrefab.mockRejectedValueOnce(new Error('Authoring revision changed'));
		const x = root.querySelector<HTMLInputElement>('input[aria-label="Placement position X"]')!;
		x.value = '7';
		x.dispatchEvent(new Event('input'));
		await fixture.whenStable();
		ui.button('Place Linked Instance').click();
		await fixture.whenStable();
		expect(backend.instantiateLinkedPrefab).toHaveBeenCalledWith(
			'door.json',
			'draft-1',
			[7, 0, 0],
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
		ui.button('Edit Prop Source…').click();
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
		expect(root.textContent).toContain('Source update available.');
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

	it('routes duplicate and both unpack depths to the assembly root from a member', async () => {
		const backend = configure();
		backend.linkedPrefabStatus.mockResolvedValue({...linked, direct_placements: 2});
		const fixture = TestBed.createComponent(LinkedPrefabInspector);
		fixture.componentRef.setInput('entity', entity);
		fixture.componentRef.setInput('editable', true);
		await fixture.whenStable();
		const ui = controls(fixture.nativeElement);
		for (const [label, type] of [
			['Duplicate Assembly', 'duplicate_linked_prefab'],
			['Unpack Outer Link', 'unpack_linked_prefab_outer'],
			['Unpack All Levels', 'unpack_linked_prefab'],
		]) {
			ui.button(label).click();
			await fixture.whenStable();
			expect(backend.execute).toHaveBeenLastCalledWith({type, root_key: 'anchor'}, 1, 4);
		}
	});

	it('keeps a missing instance pinned until a matching replacement is explicitly reviewed and applied', async () => {
		const backend = configure();
		backend.linkedPrefabStatus.mockResolvedValue({...linked, direct_placements: 2, source_error: 'Source missing'});
		backend.modelAssets.set([{name: 'Replacement', kind: 'prefab', path: replacement.path,
			asset_id: replacement.asset_id, fingerprint: replacement.fingerprint}]);
		backend.openPrefabSource.mockResolvedValue(replacement);
		backend.reviewLinkedPropRecovery.mockRejectedValueOnce(new Error('The replacement differs from the pinned objects.'));
		const fixture = TestBed.createComponent(LinkedPrefabInspector);
		fixture.componentRef.setInput('entity', entity);
		fixture.componentRef.setInput('editable', true);
		await fixture.whenStable();
		const root: HTMLElement = fixture.nativeElement;
		controls(root).button('Recover Source Link…').click();
		await fixture.whenStable();
		const dialog = root.querySelector<HTMLDialogElement>('dialog[open]')!;
		const ui = controls(dialog);
		expect(ui.button('Relink Instance').disabled).toBe(true);
		ui.change('Replacement Prop', replacement.asset_id);
		await fixture.whenStable();
		ui.button('Review Replacement').click();
		await fixture.whenStable();
		expect(dialog.textContent).toContain('The replacement differs');
		expect(ui.button('Relink Instance').disabled).toBe(true);
		expect(backend.recoverLinkedProp).not.toHaveBeenCalled();
		ui.button('Review Replacement').click();
		await fixture.whenStable();
		expect(dialog.textContent).toContain('Objects preserved2');
		expect(dialog.textContent).toContain(replacement.fingerprint);
		expect(ui.button('Relink Instance').disabled).toBe(false);
		ui.button('Relink Instance').click();
		await fixture.whenStable();
		expect(backend.recoverLinkedProp).toHaveBeenCalledWith('anchor', replacement.path, replacement.draft_id, 1, 4);
		expect(backend.closePrefabSource).toHaveBeenCalledWith(replacement.draft_id);
		expect(root.textContent).toContain('Recovered the source link');
	});

	it('shows linked source provenance and reveals the source asset across distinct instance keys', async () => {
		const backend = configure();
		const fixture = TestBed.createComponent(LinkedPrefabInspector);
		const selection = TestBed.inject(PropAssetSelection);
		fixture.componentRef.setInput('entity', entity);
		await fixture.whenStable();
		const root: HTMLElement = fixture.nativeElement;
		const ui = controls(root);
		expect(root.textContent).toContain('Saved source version');
		expect(root.textContent).toContain(linked.source_fingerprint);
		expect(root.textContent).toContain('Player');
		ui.button('Select Prop Asset').click();
		expect(selection.request()?.sourceId).toBe('source-id');

		const moved = {...linked, root_key: 'another-anchor', source_path: 'props/moved/door.json', source_changed: false};
		backend.linkedPrefabStatus.mockResolvedValueOnce(moved);
		fixture.componentRef.setInput('entity', {...entity, key: 'another-instance', name: 'Renamed placement'});
		await fixture.whenStable();
		expect(root.textContent).toContain('props/moved/door.json');
		expect(root.textContent).toContain('Source is current.');
		ui.button('Select Prop Asset').click();
		expect(selection.request()?.sourceId).toBe('source-id');
		expect(backend.execute).not.toHaveBeenCalled();

		backend.linkedPrefabStatus.mockResolvedValueOnce({...moved, source_error: 'File is missing'});
		backend.current.update(snapshot => ({...snapshot, revision: 5}));
		await fixture.whenStable();
		expect(root.textContent).toContain('This placement retains its saved objects.');
		expect(ui.button('Select Prop Asset').disabled).toBe(true);
		expect(backend.execute).not.toHaveBeenCalled();
	});

	it('requires one explicit resolution per addressed conflict and preserves a rejected review', async () => {
		const backend = configure();
		const fixture = TestBed.createComponent(LinkedPrefabInspector);
		fixture.componentRef.setInput('entity', entity);
		fixture.componentRef.setInput('editable', true);
		await fixture.whenStable();
		const root: HTMLElement = fixture.nativeElement;
		const ui = controls(root);
		ui.button('Review Source Update…').click();
		await fixture.whenStable();
		expect(backend.reviewLinkedPropOverrides).toHaveBeenCalledWith('anchor', 'draft-1', { Actor: 'player' }, 1, 4);
		expect(root.textContent).toContain('New source door');
		expect(root.textContent).toContain('The source removed this object.');
		const update = ui.button('Update Linked Instance');
		expect(update.disabled).toBe(true);
		const decisions = root.querySelectorAll<HTMLSelectElement>('select');
		expect(decisions).toHaveLength(2);
		decisions[0].value = 'keep_local';
		decisions[0].dispatchEvent(new Event('change'));
		await fixture.whenStable();
		expect(update.disabled).toBe(true);
		expect(Array.from(decisions[1].options, option => option.value)).toEqual(['', 'use_source']);
		decisions[1].value = 'use_source';
		decisions[1].dispatchEvent(new Event('change'));
		await fixture.whenStable();
		backend.updateLinkedPropReviewed.mockRejectedValueOnce(new Error('The source changed during review'));
		update.click();
		await fixture.whenStable();
		expect(root.textContent).toContain('The source changed during review');
		expect(backend.editingError()).toBeNull();
		expect(decisions[0].value).toBe('keep_local');
		update.click();
		await fixture.whenStable();
		expect(backend.updateLinkedPropReviewed).toHaveBeenLastCalledWith('anchor', 'draft-1', { Actor: 'player' }, [
			{ address: reviewedChanges[0].address, property: reviewedChanges[0].target.property, resolution: 'keep_local' },
			{ address: reviewedChanges[1].address, property: reviewedChanges[1].target.property, resolution: 'use_source' },
		], 1, 4);
	});

	it('requires confirmation to save an owned override and refreshes the catalog without updating the instance', async () => {
		const backend = configure();
		const unowned = {
			...linked.overrides[0],
			address: { kind: 'object', placement_path: ['grate-left'], source_entity: 'inner-door' } as const,
			can_apply_to_root_source: false,
		};
		backend.linkedPrefabStatus.mockResolvedValueOnce({...linked, overrides: [linked.overrides[0], unowned]});
		const fixture = TestBed.createComponent(LinkedPrefabInspector);
		fixture.componentRef.setInput('entity', entity);
		fixture.componentRef.setInput('editable', true);
		await fixture.whenStable();
		const root: HTMLElement = fixture.nativeElement;
		const ui = controls(root);
		expect(root.textContent).toContain('Placement grate-left');
		expect([...root.querySelectorAll('button')].filter(button => button.textContent.trim() === 'Apply to Source…')).toHaveLength(1);
		ui.button('Apply to Source…').click();
		await fixture.whenStable();
		expect(root.textContent).toContain('Save this property change to door.json');
		expect(backend.applyLinkedPropOverride).not.toHaveBeenCalled();
		root.querySelector<HTMLButtonElement>('button[aria-label="Close Apply Override to Source"]')!.click();
		await fixture.whenStable();
		expect(backend.applyLinkedPropOverride).not.toHaveBeenCalled();
		ui.button('Apply to Source…').click();
		await fixture.whenStable();
		ui.button('Save Override to Source').click();
		await fixture.whenStable();
		expect(backend.applyLinkedPropOverride).toHaveBeenCalledWith(
			'anchor', 'draft-1', linked.overrides[0].address, linked.overrides[0].target.property, 1, 4,
		);
		expect(backend.assets).toHaveBeenCalledWith(true);
		expect(backend.closePrefabSource).toHaveBeenCalledWith('draft-1');
		expect(backend.updateLinkedPropReviewed).not.toHaveBeenCalled();
		expect(root.querySelector('[role="status"]')?.textContent).toContain('Saved to door.json');
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
