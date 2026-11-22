import { Component, computed, DestroyRef, effect, inject, input, signal } from '@angular/core';
import { EditorService } from '../../../../core/editor/editor.service';
import { ViewportEditorService } from '../../../../core/editor/viewport-editor.service';
import {
	runtimeHandleKey,
	type RuntimeInspection,
} from '../../../../core/editor/runtime-inspection';
import {ControlGraphOutputView} from '../control-graph/control-graph-output';
import { Drawer } from '../../../../shared/components/drawer/drawer';
import { EditorPane } from '../../../../shared/components/pane/pane';
import type { DrawerSettings } from '../../../../shared/components/drawer/drawer-settings';
import type { Config } from '../../../../shared/models/config.model';

@Component({
	selector: 'editor-runtime-inspector',
	imports: [Drawer, EditorPane, ControlGraphOutputView],
	templateUrl: './runtime-inspector.html',
	styleUrl: './runtime-inspector.scss',
})
export class RuntimeInspector {
	readonly opening = input(false);
	private readonly editor = inject(EditorService);
	private readonly viewport = inject(ViewportEditorService);
	protected readonly drawerConfig: Config<DrawerSettings> = {
		width: '22rem',
		minWidth: '14rem',
		maxWidth: '45%',
	};
	protected readonly handleKey = runtimeHandleKey;
	protected readonly pageSize = 32;
	protected readonly offset = signal(0);
	protected readonly selected = signal<string | null>(null);
	protected readonly result = signal<RuntimeInspection | null>(null);
	protected readonly error = signal<string | null>(null);
	protected readonly pending = signal(false);
	protected readonly entity = computed(
		() =>
			this.result()?.entities.find(
				(entity) => runtimeHandleKey(entity.handle) === this.selected(),
			) ?? null,
	);
	protected readonly available = computed(
		() => !!this.context() && !this.opening() && !this.pending(),
	);
	protected readonly pageEnd = computed(
		() => this.offset() + (this.result()?.entities.length ?? 0),
	);
	protected readonly motion = computed(
		() =>
			this.result()?.character_motion?.find(
				(item) => runtimeHandleKey(item.entity) === this.selected(),
			) ?? null,
	);
	protected readonly animation = computed(
		() =>
			this.result()?.animations?.find(
				(item) => runtimeHandleKey(item.entity) === this.selected(),
			) ?? null,
	);
	protected readonly animator = computed(
		() => this.result()?.animators?.find(item => runtimeHandleKey(item.entity) === this.selected()) ?? null,
	);
	private readonly controlsScope = signal<{context: string; selection: string | null} | null>(null);
    private selectionChange = 0;
    protected readonly activeControls = computed(() => {
        const page = this.result();
        const status = this.viewport.gameStatus();
        const controls = page?.control_graph;
        if (!controls || !status || page.run_id !== status.run_id || page.play_session_id !== status.play_session_id ||
            page.scene_id !== status.scene_id || this.controlsScope()?.context !== this.context() ||
            this.controlsScope()?.selection !== this.selected()) return null;
        return runtimeHandleKey(controls.actor) === this.selected() ? controls : null;
    });
	private readonly context = computed(() => {
		const status = this.viewport.gameStatus();
		if (!status) return null;
		return JSON.stringify([
			this.editor.workspaceGeneration(),
			status.play_session_id,
			status.run_id,
			status.scene_id,
		]);
	});
	private request = 0;
	private destroyed = false;

	constructor() {
		effect(() => {
			this.context();
			this.request++;
			this.offset.set(0);
			this.selected.set(null);
			this.controlsScope.set(null);
			this.result.set(null);
			this.error.set(null);
			this.pending.set(false);
		});
		inject(DestroyRef).onDestroy(() => {
			this.destroyed = true;
			this.request++;
		});
	}

	protected selectEntity(key: string): void {
        if (key === this.selected()) return;
        this.selected.set(key);
        this.selectionChange++;
        this.controlsScope.set(null);
    }

	protected async refresh(offset = this.offset()): Promise<void> {
		const context = this.context();
		const status = this.viewport.gameStatus();
		if (!context || !status || !this.available()) return;
        const selectionChange = this.selectionChange;
		const request = ++this.request;
		const current = () => !this.destroyed && this.context() === context && this.request === request;
		this.pending.set(true);
		this.error.set(null);
		try {
			const result = await this.editor.inspectRuntime(offset, this.pageSize);
			if (
				!current() ||
				result.play_session_id !== status.play_session_id ||
				result.run_id !== status.run_id ||
				result.scene_id !== status.scene_id
			)
				return;
			this.offset.set(offset);
			this.result.set(result);
			if (!result.entities.some((entity) => runtimeHandleKey(entity.handle) === this.selected())) {
				this.selected.set(result.entities[0] ? runtimeHandleKey(result.entities[0].handle) : null);
			}
            this.controlsScope.set(selectionChange === this.selectionChange ? {context, selection: this.selected()} : null);
		} catch (error) {
			if (current()) this.error.set(error instanceof Error ? error.message : String(error));
		} finally {
			if (current()) this.pending.set(false);
		}
	}
}
