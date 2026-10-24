import {Component, input} from '@angular/core';

@Component({
    selector: 'editor-pane.scss',
    template: `
        <section [attr.aria-label]="title()">
            <header>
                <h2>{{ title() }}</h2>
                <ng-content select="[pane-actions]"/>
            </header>
            <div class="content">
                <ng-content/>
            </div>
        </section>
    `,
    styleUrl: './pane.scss',
})
export class EditorPane {
    readonly title = input.required<string>();

    readonly visible = input(true);

    readonly paneForm = input<any>()
    readonly paneFormConfig = input<any>()

}
