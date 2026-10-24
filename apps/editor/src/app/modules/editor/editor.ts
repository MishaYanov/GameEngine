import {Component} from '@angular/core';
import {RouterOutlet} from "@angular/router";
import {EditorHeader} from "./components/editor-header/editor-header";
import {EditorGridWrapper} from "./components/editor-grid-wrapper/editor-grid-wrapper";
import {EditorFooter} from "./components/editor-footer/editor-footer";
import {EditorPortal} from "./components/editor-portal/editor-portal";

@Component({
    selector: 'app-editor',
    imports: [
        RouterOutlet,
        EditorHeader,
        EditorGridWrapper,
        EditorFooter,
        EditorPortal
    ],
    templateUrl: './editor.html',
    styleUrl: './editor.scss',
})
export class Editor {
}
