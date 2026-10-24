import {Routes} from '@angular/router';

export const applicationRoutes: Routes = [
    {
        path: 'setup',
        loadComponent: () =>
            import('./modules/project-setup/project-setup').then((m) => m.ProjectSetup),
    },
    {path: 'editor', loadComponent: () => import('./modules/editor/editor').then((m) => m.Editor)},
];
