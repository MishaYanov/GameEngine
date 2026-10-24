import {Routes} from '@angular/router';
import {isTauri} from '@tauri-apps/api/core';
import {environment} from '../environments/environment';

export const routes: Routes = [
    {path: '', pathMatch: 'full', redirectTo: isTauri() ? 'setup' : environment.defaultRoute},
    {path: 'setup', loadComponent: () => import('./project-setup').then((m) => m.ProjectSetup)},
    {
        path: 'demo',
        loadComponent: () => import('./demo/demo-workspace').then((m) => m.DemoWorkspace),
    },
    {
        path: 'design',
        loadChildren: () =>
            import('../../projects/design/src/design.routes').then((m) => m.designRoutes),
    },
    {path: '**', redirectTo: 'setup'},
];
