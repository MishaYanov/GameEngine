import {Component} from '@angular/core';
import {Drawer} from '../../../../shared/components/drawer/drawer';
import {Config} from '../../../../shared/models/config.model';
import {DrawerSettings} from '../../../../shared/components/drawer/drawer-settings';
import {EditorIcon} from '../../../../shared/icons/editor-icon';
import {RouterLink} from '@angular/router';

@Component({
	selector: 'editor-nav-manager',
	imports: [Drawer, EditorIcon, RouterLink],
	templateUrl: './editor-nav-manager.html',
	styleUrl: './editor-nav-manager.scss',
})
export class EditorNavManager {
	readonly drawerConfig: Config<DrawerSettings> = {
		minWidth: 'var(--layout-drawer-min-width)',
		maxWidth: 'var(--layout-drawer-max-width)',
		width: 'var(--layout-sidebar-width)',
		expanded: true,
	};
}
