import {Component, inject, input, output} from '@angular/core';
import {EditorFileMenu} from '../editor-file-menu/editor-file-menu';
import {EditorService} from "../../../../core/editor/editor.service";

@Component({
    selector: 'editor-header',
    imports: [EditorFileMenu],
    templateUrl: './editor-header.html',
    styleUrl: './editor-header.scss',
})
export class EditorHeader {
    readonly canOpen = input(false);
    readonly canOpenSettings = input(true);
    readonly canExit = input(false);
    readonly opening = input(false);
    readonly openProjectRequested = output<void>();
    readonly settingsRequested = output<void>();
    readonly exitRequested = output<void>();

    protected readonly editorService = inject(EditorService);

}
