import {Component} from '@angular/core';
import {Drawer} from '../../../../shared/components/drawer/drawer';
import {Config} from '../../../../shared/models/config.model';
import {DrawerSettings} from '../../../../shared/components/drawer/drawer-settings';

@Component({
    selector: 'editor-nav-manager',
    imports: [Drawer],
    templateUrl: './editor-nav-manager.html',
    styleUrl: './editor-nav-manager.scss',
})
export class EditorNavManager {
    readonly drawerConfig: Config<DrawerSettings> = {
        minWidth: '40px',
        width: '25%',
        expanded: true,
    };
}
