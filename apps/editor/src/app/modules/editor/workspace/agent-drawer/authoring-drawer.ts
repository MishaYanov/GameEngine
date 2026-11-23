import type {DrawerSettings} from '../../../../shared/components/drawer/drawer-settings';
import type {Config} from '../../../../shared/models/config.model';

export type RightDrawerMode = 'inspector' | 'agent';

export const AUTHORING_DRAWER_CONFIG: Config<DrawerSettings> = {
    width: '20rem',
    minWidth: '12rem',
    maxWidth: '45%',
};
