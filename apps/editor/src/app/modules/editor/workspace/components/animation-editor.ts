import {
	Component,
	DestroyRef,
	computed,
	effect,
	inject,
	input,
	output,
	signal,
	untracked,
} from '@angular/core';
import { EditorService } from '../../../../core/editor/editor.service';
import type { ModelAsset } from '../../../../core/editor/editor.models';
import {
	validAnimationSettings,
	type AnimationSettings,
	type AnimationReview,
	type AnimationPreview,
} from '../../../../core/editor/animation.models';

@Component({
	selector: 'editor-animation-editor',
	templateUrl: './animation-editor.html',
	styleUrl: './machine-editor.scss',
})
export class AnimationEditor {
	readonly value = input<AnimationSettings | null>(null);
	readonly models = input<readonly ModelAsset[]>([]);
	readonly entityKey = input('');
	readonly previewAllowed = input(false);
	readonly changed = output<AnimationSettings>();
	readonly pendingChanged = output<boolean>();
	private readonly editor = inject(EditorService);
	protected readonly asset = signal('');
	protected readonly review = signal<AnimationReview | null>(null);
	protected readonly pending = signal(false);
	protected readonly error = signal<string | null>(null);
	protected readonly preview = signal<AnimationPreview | null>(null);
	protected readonly previewClip = signal(0);
	protected readonly locomotionSlots = ['idle', 'walk', 'air'] as const;
	protected readonly time = signal('0');
	protected readonly assets = computed(() =>
		this.models().filter((model) => model.kind !== 'prefab' && model.asset_id),
	);
	protected readonly clips = computed(() => this.review()?.metadata.clips ?? []);
	protected readonly compatible = computed(() => {
		const current = this.value(),
			review = this.review();
		return (
			!!current &&
			!!review &&
			JSON.stringify(current.source) === JSON.stringify(review.metadata.source)
		);
	});
	private request = 0;
	private destroyed = false;
	private readonly context = computed(() =>
		JSON.stringify([
			this.editor.workspaceGeneration(),
			this.editor.snapshot()?.revision,
			this.editor.snapshot()?.mode,
			this.entityKey(),
		]),
	);
	constructor() {
		effect(() => {
			this.context();
			this.request++;
			this.review.set(null);
			this.error.set(null);
			untracked(() => void this.close());
		});
		effect(() => {
			if (!this.asset() && this.value()) this.asset.set(this.value()!.source.asset_id);
		});
		effect(() => this.pendingChanged.emit(this.pending()));
		inject(DestroyRef).onDestroy(() => {
			this.destroyed = true;
			this.request++;
			void this.close();
		});
	}
	protected chooseAsset(asset: string): void {
		this.asset.set(asset);
		this.review.set(null);
		this.request++;
		void this.close();
	}
	protected async inspect(): Promise<void> {
		const snapshot = this.editor.snapshot(),
			asset = this.asset(),
			context = this.context();
		if (!snapshot || !asset || this.pending() || snapshot.mode !== 'edit') return;
		const request = ++this.request,
			generation = this.editor.workspaceGeneration();
		this.pending.set(true);
		this.error.set(null);
		try {
			const review = await this.editor.inspectAnimation(asset, generation, snapshot.revision);
			if (this.destroyed || request !== this.request || context !== this.context()) return;
			if (review.metadata.source.asset_id !== asset)
				throw new Error('Animation review returned a different asset.');
			this.review.set(review);
			const previous = this.value();
			const preserve =
				previous &&
				previous.source.asset_id === asset &&
				(previous.playback.kind === 'single_clip'
					? previous.playback.clip_index < review.metadata.clips.length
					: [previous.playback.idle, previous.playback.walk, previous.playback.air].every(
							(index) => index < review.metadata.clips.length,
						));
			this.changed.emit({
				source: review.metadata.source,
				playback: preserve
					? previous.playback
					: { kind: 'single_clip', clip_index: 0, looping: true },
				speed_numerator: previous?.speed_numerator ?? 1,
				speed_denominator: previous?.speed_denominator ?? 1,
			});
		} catch (error) {
			if (!this.destroyed && request === this.request)
				this.error.set(error instanceof Error ? error.message : String(error));
		} finally {
			this.pending.set(false);
		}
	}
	protected playback(kind: string): void {
		const current = this.value();
		if (!current) return;
		this.changed.emit({
			...current,
			playback:
				kind === 'locomotion3'
					? { kind: 'locomotion3', idle: 0, walk: 0, air: 0 }
					: { kind: 'single_clip', clip_index: 0, looping: true },
		});
	}
	protected clip(slot: 'clip_index' | 'idle' | 'walk' | 'air', value: string): void {
		const current = this.value();
		if (!current) return;
		const index = Number(value);
		if (!Number.isSafeInteger(index) || index < 0 || index >= this.clips().length) return;
		const playback = current.playback;
		if (playback.kind === 'single_clip' && slot === 'clip_index')
			this.changed.emit({ ...current, playback: { ...playback, clip_index: index } });
		else if (playback.kind === 'locomotion3' && slot !== 'clip_index')
			this.changed.emit({ ...current, playback: { ...playback, [slot]: index } });
	}
	protected looping(looping: boolean): void {
		const current = this.value();
		if (current?.playback.kind === 'single_clip')
			this.changed.emit({ ...current, playback: { ...current.playback, looping } });
	}
	protected speed(field: 'speed_numerator' | 'speed_denominator', value: string): void {
		const current = this.value();
		if (current) this.changed.emit({ ...current, [field]: Number(value) });
	}
	protected choosePreview(value: string): void {
		this.previewClip.set(Number(value));
		void this.close();
	}
	protected async openPreview(): Promise<void> {
		const snapshot = this.editor.snapshot(),
			context = this.context();
		if (
			!snapshot ||
			!this.previewAllowed() ||
			this.pending() ||
			!this.compatible() ||
			!validAnimationSettings(this.value())
		)
			return;
		const generation = this.editor.workspaceGeneration(),
			request = ++this.request;
		this.pending.set(true);
		this.error.set(null);
		try {
			const result = await this.editor.openAnimationPreview(
				this.entityKey(),
				this.previewClip(),
				generation,
				snapshot.revision,
			);
			if (this.destroyed || request !== this.request || context !== this.context()) {
				await this.editor.closeAnimationPreview(result.token);
				return;
			}
			this.preview.set(result);
			this.time.set('0');
		} catch (error) {
			if (!this.destroyed && request === this.request)
				this.error.set(error instanceof Error ? error.message : String(error));
		} finally {
			this.pending.set(false);
		}
	}
	protected async sample(): Promise<void> {
		const preview = this.preview(),
			seconds = Number(this.time()),
			context = this.context();
		if (!preview || this.pending()) return;
		if (
			!this.time().trim() ||
			!Number.isFinite(seconds) ||
			seconds < 0 ||
			seconds > preview.duration_seconds
		) {
			this.error.set('Choose a time within this clip.');
			return;
		}
		this.pending.set(true);
		this.error.set(null);
		try {
			const result = await this.editor.sampleAnimationPreview(
				preview.token,
				seconds,
				this.editor.workspaceGeneration(),
			);
			if (!this.destroyed && context === this.context() && this.preview()?.token === preview.token)
				this.preview.set(result);
		} catch (error) {
			if (!this.destroyed && context === this.context())
				this.error.set(error instanceof Error ? error.message : String(error));
		} finally {
			this.pending.set(false);
		}
	}
	protected async close(): Promise<void> {
		const preview = this.preview();
		this.preview.set(null);
		if (preview) {
			try {
				await this.editor.closeAnimationPreview(preview.token);
			} catch (error) {
				if (!this.destroyed) this.error.set(error instanceof Error ? error.message : String(error));
			}
		}
	}
}
