import {Config, isConfigObject} from '../../models/config.model';

export interface DrawerSettings {
    width: string;
    height: string;
    minWidth: string;
    maxWidth: string;
    minHeight: string;
    maxHeight: string;
    expanded: boolean;
}

export const DEFAULT_DRAWER_SETTINGS: Readonly<DrawerSettings> = {
    width: '25%',
    height: '100%',
    minWidth: '0px',
    maxWidth: '100%',
    minHeight: '0px',
    maxHeight: '100%',
    expanded: true,
};

export function drawerSettings(value: unknown): Config<DrawerSettings> {
    if (!isConfigObject(value)) return {};
    const config: Config<DrawerSettings> = {};
    for (const name of [
        'width',
        'height',
        'minWidth',
        'maxWidth',
        'minHeight',
        'maxHeight',
    ] as const) {
        const size = value[name];
        if (typeof size === 'string' && /^(?:0|\d+(?:\.\d+)?(?:px|rem|em|%|vw|vh))$/.test(size))
            config[name] = size;
    }
    if (typeof value['expanded'] === 'boolean') config.expanded = value['expanded'];
    return config;
}
