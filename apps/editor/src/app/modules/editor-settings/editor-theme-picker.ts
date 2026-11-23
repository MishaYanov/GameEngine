import {Component, inject} from '@angular/core';
import {EDITOR_THEMES, EditorThemeService} from '../../core/editor/settings/editor-theme.service';

@Component({
    selector: 'editor-theme-picker',
    template: `
        <label>
            <span>Theme</span>
            <select #theme (change)="themes.setTheme(theme.value)">
                @for (option of options; track option.id) {
                    <option [value]="option.id" [selected]="option.id === themes.theme()">{{ option.label }}</option>
                }
            </select>
        </label>
    `,
    styles: `
        label {
            display: inline-flex;
            flex-wrap: wrap;
            align-items: center;
            gap: var(--space-2);
            color: var(--color-text);
            font-size: var(--font-size-sm);
        }

        select {
            min-height: var(--layout-subject-control-height);
            max-width: 100%;
            padding-inline: var(--space-2);
            border: var(--border-width) solid var(--color-input-edge);
            border-radius: var(--radius-sm);
            background: var(--color-input);
            color: var(--color-text);
            font: inherit;
        }
    `,
})
export class EditorThemePicker {
    protected readonly themes = inject(EditorThemeService);
    protected readonly options = EDITOR_THEMES;
}
