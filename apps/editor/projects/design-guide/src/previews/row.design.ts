import {Component, signal} from '@angular/core';
import {form, FormField} from '@angular/forms/signals';
import {EditorRow} from '../../../../src/app/shared/components/row/row';

@Component({
    imports: [EditorRow, FormField],
    template: `
        <editor-row>
            <label>X <input type="number" aria-label="Row X" [formField]="fields.x"/></label>
            <label>Y <input type="number" aria-label="Row Y" [formField]="fields.y"/></label>
            <label>Z <input type="number" aria-label="Row Z" [formField]="fields.z"/></label>
        </editor-row>
        <editor-row [columns]="1">
            <label>Name <input aria-label="Row Name" [formField]="fields.name"/></label>
        </editor-row>
    `,
    styles: `
      :host {
        display: flex;
        flex-direction: column;
        gap: var(--space-3);
        width: var(--layout-inspector-width);
        max-width: 100%;
      }

      label {
        display: flex;
        align-items: center;
        gap: var(--space-2);
        min-width: 0;
        color: var(--color-text-secondary);
        font-size: var(--font-size-sm);
      }

      input {
        width: 100%;
        min-width: 0;
        height: var(--layout-subject-control-height);
        padding: var(--space-1) var(--space-2);
        color: var(--color-text);
        background: var(--color-input);
        border: var(--border-width) solid var(--color-input-edge);
        border-radius: var(--radius-sm);
      }
    `,
})
class RowPreview {
    readonly data = signal({x: 0, y: 0, z: 0, name: 'Object'});
    readonly fields = form(this.data);
}

export default {component: RowPreview};
