import {afterNextRender, Component, computed, ElementRef, inject, output, viewChild} from '@angular/core';
import type {ComponentRecord} from '../../../../core/editor/editor-components';
import {EditorIcon} from '../../../../shared/icons/editor-icon';
import {PropWorkspaceService} from './prop-workspace.service';
import {PropPreviewService} from './prop-preview.service';

@Component({
    selector: 'editor-prop-workspace',
    imports: [EditorIcon],
    templateUrl: './prop-workspace.html',
    styleUrl: './prop-workspace.scss',
})
export class PropWorkspace {
    readonly closed = output<void>();
    protected readonly prop = inject(PropWorkspaceService);
    protected readonly preview = inject(PropPreviewService);
    protected readonly parentName = computed(() => {
        const key = this.prop.selectedObject()?.parent;
        return this.prop.document()?.objects.find(object => object.key === key)?.name ?? 'Prop root';
    });
    private readonly heading = viewChild<ElementRef<HTMLElement>>('heading');

    constructor() {
        afterNextRender(() => this.heading()?.nativeElement.focus({preventScroll: true}));
    }

    protected componentLabel(component: ComponentRecord): string {
        return component.data.kind.replaceAll('_', ' ');
    }

    protected handleKey(event: KeyboardEvent): void {
        if (event.key !== 'Escape' || event.defaultPrevented) return;
        event.preventDefault();
        event.stopPropagation();
        this.closed.emit();
    }
}
