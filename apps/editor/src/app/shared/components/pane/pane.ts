import {Component, inject, input} from '@angular/core';
import {EditorIcon, EditorIconName} from '../../icons/editor-icon';
import {Drawer} from '../drawer/drawer';

@Component({
    selector: 'editor-pane',
    imports: [EditorIcon],
    template: `
        @if (visible()) {
            <section class="pane" [attr.aria-label]="title()">
                <header class="pane-header">
                    <div class="pane-title">
                        @if (drawer; as drawer) {
                            <button
                                    type="button"
                                    class="drawer-toggle"
                                    data-drawer-toggle
                                    [attr.aria-label]="'Minimize ' + title()"
                                    [title]="'Minimize ' + title()"
                                    aria-expanded="true"
                                    (click)="drawer.toggleCollapsed()"
                            >
                                <editor-icon [name]="icon() ?? 'layers'"/>
                            </button>
                        } @else if (icon(); as name) {
                            <editor-icon [name]="name"/>
                        }
                        <ng-content select="[pane-icon]"/>
                        <h2>{{ title() }}</h2>
                    </div>
                    <ng-content select="[pane-actions]"/>
                </header>
                <div class="content" [class.cards]="layout() === 'cards'">
                    <ng-content/>
                </div>
            </section>
        }
    `,
    styleUrl: './pane.scss',
    host: {'[hidden]': '!visible()'},
})
export class EditorPane {
    readonly title = input.required<string>();
    readonly icon = input<EditorIconName>();
    readonly layout = input<'fill' | 'cards'>('fill');
    readonly visible = input(true);
    readonly paneForm = input<unknown>();
    readonly paneFormConfig = input<unknown>();
    protected readonly drawer = inject(Drawer, {optional: true});
}
