import {Component, DestroyRef, ElementRef, inject, input, output, viewChild} from '@angular/core';
import {EditorFileMenu} from '../editor-file-menu/editor-file-menu';
import type {ProjectTemplate} from '../../../../core/project/project.service';
import {EditorService} from "../../../../core/editor/editor.service";
import {OBJECT_MENU_GROUPS, type ObjectMenuAction} from '../../../../core/editor/editor-object-menu';
import type {ContextMenuItem} from '../../../../shared/builder/context-menu-builder';
import {ContextMenuService} from '../../../../shared/components/context-menu/context-menu.service';

@Component({
    selector: 'editor-header',
    imports: [EditorFileMenu],
    templateUrl: './editor-header.html',
    styleUrl: './editor-header.scss',
})
export class EditorHeader {
    readonly canOpen = input(false);
    readonly canOpenControls = input(false);
    readonly canOpenSettings = input(true);
    readonly canExit = input(false);
    readonly opening = input(false);
    readonly availableObjectActions = input<readonly ObjectMenuAction[]>([]);
    readonly openProjectRequested = output<void>();
    readonly projectSetupRequested = output<ProjectTemplate>();
    readonly sceneInformationRequested = output<void>();
    readonly controlsRequested = output<void>();
    readonly physicsLayersRequested = output<void>();
    readonly objectActionRequested = output<ObjectMenuAction>();
    readonly settingsRequested = output<void>();
    readonly exitRequested = output<void>();

    protected readonly editorService = inject(EditorService);
    protected readonly contextMenu = inject(ContextMenuService);
    private readonly objectsTrigger = viewChild<ElementRef<HTMLButtonElement>>('objectsTrigger');

    constructor() {
        inject(DestroyRef).onDestroy(() => {
            const trigger = this.objectsTrigger()?.nativeElement;
            if (trigger) this.contextMenu.closeFor(trigger);
        });
    }

    protected openObjects(trigger: HTMLButtonElement): void {
        if (this.opening() || !this.availableObjectActions().length) return;
        if (this.contextMenu.request()?.owner === trigger) {
            this.contextMenu.close();
            return;
        }
        const generation = this.editorService.workspaceGeneration();
        const scene = this.editorService.snapshot()?.active_scene_id;
        const items: ContextMenuItem[] = [];
        for (const group of OBJECT_MENU_GROUPS) {
            if (items.length) items.push({kind: 'separator', id: group[0].id + '-separator'});
            for (const {id, label} of group) {
                items.push({
                    kind: 'action',
                    id,
                    label,
                    destructive: false,
                    disabled: !this.availableObjectActions().includes(id),
                    execute: () => {
                        if (!this.opening() && this.availableObjectActions().includes(id) &&
                            generation === this.editorService.workspaceGeneration() &&
                            scene === this.editorService.snapshot()?.active_scene_id) this.objectActionRequested.emit(id);
                    },
                });
            }
        }
        const bounds = trigger.getBoundingClientRect();
        this.contextMenu.open({
            items,
            label: 'Objects',
            x: bounds.left,
            y: bounds.bottom,
            owner: trigger,
            returnFocus: trigger
        });
    }

    protected openObjectsFromKeyboard(event: KeyboardEvent, trigger: HTMLButtonElement): void {
        if (event.key !== 'ArrowDown' || event.altKey || event.ctrlKey || event.metaKey ||
            this.contextMenu.request()?.owner === trigger) return;
        event.preventDefault();
        this.openObjects(trigger);
    }
}
