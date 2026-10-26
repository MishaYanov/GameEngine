import {Component} from '@angular/core';
import {EditorPane} from '../../../../shared/components/pane/pane';
import {EditorIcon} from '../../../../shared/icons/editor-icon';
import {RouterLink} from '@angular/router';

@Component({
    selector: 'editor-grid-wrapper',
    imports: [EditorPane, EditorIcon, RouterLink],
    templateUrl: './editor-grid-wrapper.html',
    styleUrl: './editor-grid-wrapper.scss',
})
export class EditorGridWrapper {
}
