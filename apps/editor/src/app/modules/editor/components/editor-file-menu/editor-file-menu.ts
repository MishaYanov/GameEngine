import {Component, inject, input, output} from '@angular/core';
import {RouterLink} from '@angular/router';
import {EditorService} from "../../../../core/editor/editor.service";

@Component({
    selector: 'editor-file-menu',
    imports: [RouterLink],
    templateUrl: './editor-file-menu.html',
    styleUrl: './editor-file-menu.scss',
})
export class EditorFileMenu {
    readonly canOpen = input(false);
    readonly opening = input(false);
    readonly openProjectRequested = output<void>();

    protected readonly nativeAvailable = inject(EditorService).nativeAvailable;
}
