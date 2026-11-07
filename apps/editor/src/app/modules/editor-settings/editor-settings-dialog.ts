import {Component, inject} from '@angular/core';
import {EDITOR_KEYMAP_SECTIONS, EDITOR_SETTINGS_TREE} from '../../core/editor/settings/editor-settings.models';
import {EditorSettingsService} from '../../core/editor/settings/editor-settings.service';
import {EditorDialog} from '../../shared/components/dialog/dialog';
import {EditorFormSection} from '../../shared/components/form-section/form-section';
import {EditorTree} from '../../shared/components/tree/tree';

@Component({
    selector: 'editor-settings-dialog',
    imports: [EditorDialog, EditorFormSection, EditorTree],
    templateUrl: './editor-settings-dialog.html',
    styleUrl: './editor-settings-dialog.scss',
})
export class EditorSettingsDialog {
    protected readonly settings = inject(EditorSettingsService);
    protected readonly tree = EDITOR_SETTINGS_TREE;
    protected readonly sections = EDITOR_KEYMAP_SECTIONS;
    protected readonly dialogConfig = {width: '50rem', height: '36rem'};
}
