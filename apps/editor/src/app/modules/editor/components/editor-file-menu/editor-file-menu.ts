import {Component, inject, input, output} from '@angular/core';
import type {ProjectTemplate} from '../../../../core/project/project.service';
import {EditorService} from "../../../../core/editor/editor.service";

@Component({
    selector: 'editor-file-menu',
    templateUrl: './editor-file-menu.html',
    styleUrl: './editor-file-menu.scss',
})
export class EditorFileMenu {
    readonly canOpen = input(false);
    readonly canOpenSettings = input(true);
    readonly canExit = input(false);
    readonly opening = input(false);
    readonly openProjectRequested = output<void>();
    readonly projectSetupRequested = output<ProjectTemplate>();
    readonly settingsRequested = output<void>();
    readonly exitRequested = output<void>();

    protected readonly nativeAvailable = inject(EditorService).nativeAvailable;
}
