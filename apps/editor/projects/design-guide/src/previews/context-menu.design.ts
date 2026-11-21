import {Component, signal} from '@angular/core';
import {ContextActionBuilder} from '../../../../src/app/shared/actions/context-action';
import {ContextMenuBuilder} from '../../../../src/app/shared/builder/context-menu-builder';
import {EditorContextMenu} from '../../../../src/app/shared/components/context-menu/context-menu';
import {EditorContextMenuTrigger} from '../../../../src/app/shared/components/context-menu/context-menu-trigger';

@Component({
    imports: [EditorContextMenu, EditorContextMenuTrigger],
    template: `
        <p>Right-click a component, or focus it and press Shift+F10.</p>
        <div class="components">
            @for (name of names; track name) {
                <button type="button" [editorContextMenu]="actions.build(name)"
                        [editorContextMenuLabel]="name + ' actions'">{{ name }}</button>
            }
        </div>
        <p role="status">{{ status() }}</p>
        <editor-context-menu/>
    `,
    styles: `
        :host { display: grid; gap: var(--space-3); color: var(--color-text); }
        p { margin: 0; font-size: var(--font-size-sm); color: var(--color-text-secondary); }
        .components { display: flex; gap: var(--space-3); flex-wrap: wrap; }
        button { padding: var(--space-4); border: var(--border-width) solid var(--color-border);
            border-radius: var(--radius-md); background: var(--color-control); color: var(--color-text); }
    `,
})
class ContextMenuPreview {
    readonly names = ['Player', 'Camera', 'Directional Light'];
    readonly status = signal('Choose a component action.');
    readonly actions = ContextMenuBuilder.create<string>()
        .action(ContextActionBuilder.create<string>('select', 'Select').icon('object')
            .run(name => this.status.set(`Selected ${name}.`)).build())
        .submenu('component', 'Component', builder => builder
            .action(ContextActionBuilder.create<string>('duplicate', 'Duplicate').icon('layers').shortcut('Ctrl+D')
                .run(name => this.status.set(`Duplicated ${name}.`)).build())
            .action(ContextActionBuilder.create<string>('rename', 'Rename…')
                .enabledWhen(name => name !== 'Camera')
                .run(name => this.status.set(`Rename ${name}.`)).build()))
        .separator()
        .action(ContextActionBuilder.create<string>('delete', 'Delete').destructive().shortcut('Delete')
            .run(name => this.status.set(`Deleted ${name}.`)).build());
}

export default {component: ContextMenuPreview};
