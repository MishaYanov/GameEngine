import {Component, computed, inject, input, model} from '@angular/core';
import {BuilderConfig} from '../../builder/models/builder-config.model';
import {Config} from '../../models/config.model';
import {EditorComponentService} from '../../services/editor-component.service';
import {DEFAULT_DRAWER_SETTINGS, DrawerSettings, drawerSettings} from './drawer-settings';

export type DrawerType = 'left' | 'right' | 'bottom' | 'top';

@Component({
    selector: 'app-drawer',
    imports: [],
    providers: [EditorComponentService],
    templateUrl: './drawer.html',
    styleUrl: './drawer.scss',
})
export class Drawer {
    readonly type = input<DrawerType>('left');
    readonly preferenceKey = input<string>();
    readonly config = model<Config<DrawerSettings>>({...DEFAULT_DRAWER_SETTINGS});
    readonly builderOptions = input<BuilderConfig>();
    readonly isExpanded = computed(() => this.config().expanded ?? DEFAULT_DRAWER_SETTINGS.expanded);
    private readonly behavior = inject(EditorComponentService);
    readonly configReady = this.behavior.initialized;
    readonly configError = this.behavior.error;

    constructor() {
        this.behavior.connectConfig<DrawerSettings>(
            () => this.preferenceKey() ?? `drawer:${this.type()}`,
            this.config,
            DEFAULT_DRAWER_SETTINGS,
            drawerSettings,
        );
    }
}
