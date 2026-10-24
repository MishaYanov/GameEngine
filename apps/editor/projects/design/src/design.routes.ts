import {Routes} from '@angular/router';

export const designRoutes: Routes = [
    {path: '', loadComponent: () => import('./workbench').then((m) => m.DesignWorkbench)},
    // These aliases also support the workspace view when independently served at /design/.
    {path: 'design', redirectTo: '', pathMatch: 'full'},
    {
        path: 'setup',
        loadComponent: () => import('../../../src/app/project-setup').then((m) => m.ProjectSetup),
    },
];
