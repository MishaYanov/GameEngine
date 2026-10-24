import {Routes} from '@angular/router';

// No reference to development routes: demo/design chunks are absent from production.
export const productionRoutes: Routes = [
    {
        path: '',
        pathMatch: 'full',
        loadComponent: () => import('./project-setup').then((m) => m.ProjectSetup),
    },
    {path: '**', redirectTo: ''},
];
