import {Component, input} from '@angular/core';

@Component({
    selector: 'editor-footer',
    imports: [],
    templateUrl: './editor-footer.html',
    styleUrl: './editor-footer.scss',
})
export class EditorFooter {
    readonly projectName = input<string | null>(null);
}
