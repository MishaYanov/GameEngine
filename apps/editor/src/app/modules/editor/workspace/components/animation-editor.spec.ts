import { signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { vi } from 'vitest';
import axe from 'axe-core';
import { EditorService } from '../../../../core/editor/editor.service';
import {
	readAnimationReview,
	readAnimationPreview,
	validAnimationSettings,
	type AnimationPreview,
	type AnimationReview,
	type AnimationSettings,
} from '../../../../core/editor/animation.models';
import { AnimationEditor } from './animation-editor';

const settings: AnimationSettings = {
	source: {
		asset_id: 'asset-1',
		expected_sha256: 'a'.repeat(64),
		scene_index: 0,
		mesh_node: 0,
		skin_index: 0,
	},
	playback: { kind: 'single_clip', clip_index: 0, looping: true },
	speed_numerator: 1,
	speed_denominator: 1,
};
const review: AnimationReview = {
	workspace_generation: 1,
	revision: 4,
	metadata: {
		source: settings.source,
		mesh_index: 0,
		nodes: 3,
		joints: 2,
		clips: [{ index: 0, name: 'Idle', duration_seconds: 1 }],
	},
};
const preview: AnimationPreview = {
	token: '01234567-0123-0123-0123-012345678901',
	workspace_generation: 1,
	revision: 4,
	entity_key: 'player',
	clip_index: 0,
	duration_seconds: 1,
	time_seconds: 0,
};

async function setup() {
	const snapshot = signal({ revision: 4, mode: 'edit' }),
		generation = signal(1);
	const inspectAnimation = vi.fn(async () => review),
		openAnimationPreview = vi.fn(async () => preview);
	const sampleAnimationPreview = vi.fn(async (_token: string, time: number) => ({
		...preview,
		time_seconds: time,
	}));
	const closeAnimationPreview = vi.fn(async (_token: string) => undefined);
	TestBed.configureTestingModule({
		providers: [
			{
				provide: EditorService,
				useValue: {
					snapshot,
					workspaceGeneration: generation,
					inspectAnimation,
					openAnimationPreview,
					sampleAnimationPreview,
					closeAnimationPreview,
				},
			},
		],
	});
	const fixture = TestBed.createComponent(AnimationEditor);
	fixture.componentRef.setInput('value', settings);
	fixture.componentRef.setInput('entityKey', 'player');
	fixture.componentRef.setInput('previewAllowed', true);
	const changes: AnimationSettings[] = [];
	fixture.componentInstance.changed.subscribe((value) => changes.push(value));
	await fixture.whenStable();
	const root: HTMLElement = fixture.nativeElement;
	const button = (name: string) =>
		[...root.querySelectorAll('button')].find((item) => item.textContent?.trim() === name)!;
	return {
		fixture,
		root,
		button,
		snapshot,
		generation,
		inspectAnimation,
		openAnimationPreview,
		sampleAnimationPreview,
		closeAnimationPreview,
		changes,
	};
}
describe('native animation authoring', () => {
	it('validates bounded metadata, source pins, speed ratios and preview times', () => {
		expect(readAnimationReview(review)).toEqual(review);
		expect(validAnimationSettings({ ...settings, speed_numerator: 16, speed_denominator: 1 })).toBe(
			false,
		);
		expect(
			validAnimationSettings({
				...settings,
				source: { ...settings.source, expected_sha256: 'changed' },
			}),
		).toBe(false);
		expect(() =>
			readAnimationReview({
				...review,
				metadata: { ...review.metadata, clips: [{ index: 1, name: 'Wrong', duration_seconds: 1 }] },
			}),
		).toThrow('metadata');
		expect(() => readAnimationPreview({ ...preview, time_seconds: Infinity })).toThrow('invalid');
	});
	it('keeps an opened preview alive, scrubs only native state and closes on context replacement', async () => {
		const p = await setup();
		p.button('Review Animation Asset').click();
		await p.fixture.whenStable();
		expect(p.changes).toHaveLength(1);
		p.button('Open Native Preview').click();
		await p.fixture.whenStable();
		expect(p.openAnimationPreview).toHaveBeenCalledExactlyOnceWith('player', 0, 1, 4);
		expect(p.closeAnimationPreview).not.toHaveBeenCalled();
		const field = p.root.querySelector<HTMLInputElement>('input[inputmode="decimal"]')!;
		field.value = '.5';
		field.dispatchEvent(new Event('input'));
		p.button('Show Pose').click();
		await p.fixture.whenStable();
		expect(p.sampleAnimationPreview).toHaveBeenCalledExactlyOnceWith(preview.token, 0.5, 1);
		expect(p.root.textContent).toContain('Preview at 0.5');
		expect(p.changes).toHaveLength(1);
		const report = await axe.run(p.root, { rules: { 'color-contrast': { enabled: false } } });
		expect(report.violations.map((item) => item.id)).toEqual([]);
		p.snapshot.set({ revision: 5, mode: 'edit' });
		await p.fixture.whenStable();
		expect(p.closeAnimationPreview).toHaveBeenCalledExactlyOnceWith(preview.token);
		expect(p.root.textContent).not.toContain('Preview at');
	});
	it('closes a late preview completion after workspace replacement', async () => {
		const p = await setup();
		p.button('Review Animation Asset').click();
		await p.fixture.whenStable();
		let resolve!: (value: AnimationPreview) => void;
		p.openAnimationPreview.mockImplementationOnce(() => new Promise((done) => (resolve = done)));
		p.button('Open Native Preview').click();
		p.generation.set(2);
		await p.fixture.whenStable();
		resolve(preview);
		await p.fixture.whenStable();
		expect(p.closeAnimationPreview).toHaveBeenCalledExactlyOnceWith(preview.token);
		expect(p.root.textContent).not.toContain('Preview at');
	});
	it('rejects invalid scrub time without a native call and releases the token on destroy', async () => {
		const p = await setup();
		p.button('Review Animation Asset').click();
		await p.fixture.whenStable();
		p.button('Open Native Preview').click();
		await p.fixture.whenStable();
		const field = p.root.querySelector<HTMLInputElement>('input[inputmode="decimal"]')!;
		field.value = '2';
		field.dispatchEvent(new Event('input'));
		p.button('Show Pose').click();
		await p.fixture.whenStable();
		expect(p.sampleAnimationPreview).not.toHaveBeenCalled();
		expect(p.root.textContent).toContain('within this clip');
		p.fixture.destroy();
		expect(p.closeAnimationPreview).toHaveBeenCalledExactlyOnceWith(preview.token);
	});
});
