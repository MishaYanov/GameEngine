import {Component, computed, inject, input, signal} from '@angular/core';
import {EditorService} from '../../../../core/editor/editor.service';
import {ViewportEditorService} from '../../../../core/editor/viewport-editor.service';
import type {ViewportAction, ViewportOrientation, ViewportPlane, ViewportPivot, ViewportProjection, ViewportSpace, ViewportTool} from '../../../../core/editor/viewport-editor.models';
import {SceneEditingService} from '../scene-editing.service';

@Component({
    selector: 'editor-viewport-toolbar',
    templateUrl: './viewport-toolbar.html',
    styleUrl: './viewport-toolbar.scss',
})
export class ViewportToolbar {
    readonly opening = input(false);
    protected readonly viewport = inject(ViewportEditorService);
    protected readonly editor = inject(EditorService);
    private readonly editing = inject(SceneEditingService);
    protected readonly settings = this.viewport.settings;
    protected readonly help = signal(false);
    protected readonly available = computed(() => this.viewport.nativeAvailable && !!this.editor.snapshot() &&
        !this.opening() && this.settings().view === 'scene' && !this.viewport.authoringGesture());
    protected readonly authoringAvailable = computed(() => this.available() && !this.editor.playing());
    protected readonly tools: readonly {tool: ViewportTool; label: string; shortcut: string}[] = [
        {tool: 'hand', label: 'Hand', shortcut: 'Q'},
        {tool: 'move', label: 'Move', shortcut: 'W'},
        {tool: 'rotate', label: 'Rotate', shortcut: 'E'},
        {tool: 'scale', label: 'Scale', shortcut: 'R'},
    ];
    protected readonly views: readonly {value: ViewportOrientation; label: string}[] = [
        {value: 'free', label: 'Free'}, {value: 'top', label: 'Top'}, {value: 'bottom', label: 'Bottom'},
        {value: 'front', label: 'Front'}, {value: 'back', label: 'Back'}, {value: 'left', label: 'Left'}, {value: 'right', label: 'Right'},
    ];

    protected async send(action: ViewportAction): Promise<void> {
        if (!this.available() && action.type !== 'cancel') return;
        try {
            if (action.type === 'set_tool' || action.type === 'set_space' || action.type === 'set_pivot') {
                if (action.type === 'set_tool' && action.tool === 'hand' && this.editor.playing()) await this.viewport.action(action);
                else {
                    if (!this.authoringAvailable()) return;
                    await this.editing.withCommittedEdit(async () => {await this.viewport.action(action);});
                }
            } else await this.viewport.action(action);
        } catch {/* The viewport service owns user-visible errors. */}
    }

    protected space(value: string): void {if (value === 'world' || value === 'local') void this.send({type: 'set_space', space: value as ViewportSpace});}
    protected pivot(value: string): void {if (value === 'pivot' || value === 'center') void this.send({type: 'set_pivot', pivot: value as ViewportPivot});}
    protected plane(value: string): void {if (value === 'xz' || value === 'xy' || value === 'yz') void this.send({type: 'set_plane', plane: value as ViewportPlane});}
    protected projection(value: string): void {if (value === 'perspective' || value === 'orthographic') void this.send({type: 'set_projection', projection: value as ViewportProjection});}
    protected orient(value: string): void {if (this.views.some(view => view.value === value)) void this.send({type: 'orient', orientation: value as ViewportOrientation});}
    protected gridSpacing(input: HTMLInputElement): void {
        if (Number.isFinite(input.valueAsNumber) && input.valueAsNumber > 0) void this.send({type: 'set_grid_spacing', spacing: input.valueAsNumber});
        else input.value = String(this.settings().grid_spacing);
    }
    protected snapIncrements(move: HTMLInputElement, rotate: HTMLInputElement, scale: HTMLInputElement): void {
        const values = [move.valueAsNumber, rotate.valueAsNumber, scale.valueAsNumber];
        if (values.every(value => Number.isFinite(value) && value > 0)) {
            void this.send({type: 'set_snap_increments', translation: values[0], rotation_degrees: values[1], scale: values[2]});
        } else {
            move.value = String(this.settings().move_snap); rotate.value = String(this.settings().rotation_snap_degrees); scale.value = String(this.settings().scale_snap);
        }
    }
}
