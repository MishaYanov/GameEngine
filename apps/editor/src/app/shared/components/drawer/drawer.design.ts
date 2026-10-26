import {Component} from '@angular/core';
import {Drawer} from './drawer';
import {EditorPane} from '../pane/pane';
import type {Config} from '../../models/config.model';
import type {DrawerSettings} from './drawer-settings';

@Component({
    imports: [Drawer, EditorPane],
    template: `<app-drawer
			preferenceKey="design:drawer"
			[config]="config"
			resizeLabel="Resize preview drawer"
		>
			<editor-pane title="Explorer" icon="folder"><p>Project files</p></editor-pane>
		</app-drawer>
		<editor-pane title="Scene" icon="cube"
			><p>Drag the divider or use its arrow keys.</p></editor-pane
		>`,
    styles: `
		:host {
			display: flex;
			height: 100%;
			min-width: 0;
		}
		editor-pane {
			flex: 1;
			margin-left: var(--space-2);
		}
		.drawer-label {
			display: flex;
			gap: var(--space-2);
			align-items: center;
			padding: var(--space-4);
		}
		p {
			padding: var(--space-4);
			margin: 0;
			color: var(--color-text-muted);
		}
	`,
})
class DrawerPreview {
    readonly config: Config<DrawerSettings> = {
        width: 'var(--layout-sidebar-width)',
        minWidth: 'var(--layout-drawer-min-width)',
        maxWidth: 'var(--layout-drawer-max-width)',
    };
}

export default {component: DrawerPreview};
