import {Component, input} from '@angular/core';

let nextSectionId = 0;

@Component({
    selector: 'editor-form-section',
    template: `
        <section [attr.aria-labelledby]="headingId" [attr.aria-describedby]="description() ? descriptionId : null">
            <header>
                <h3 [id]="headingId">{{ title() }}</h3>
                @if (description(); as details) {
                    <p [id]="descriptionId">{{ details }}</p>
                }
            </header>
            <ng-content/>
        </section>
    `,
    styles: `
      :host {
        display: block;
        min-width: 0;
      }

      section, header {
        display: grid;
        gap: var(--space-4);
      }

      header {
        gap: var(--space-1);
      }

      h3, p {
        margin: 0;
      }

      h3 {
        color: var(--color-text);
        font-size: var(--font-size-md);
        font-weight: var(--font-weight-semibold);
      }

      p {
        color: var(--color-text-secondary);
        font-size: var(--font-size-sm);
      }
    `,
})
export class EditorFormSection {
    readonly title = input.required<string>();
    readonly description = input<string>();

    protected readonly headingId = `editor-form-section-${nextSectionId++}`;
    protected readonly descriptionId = `${this.headingId}-description`;
}
