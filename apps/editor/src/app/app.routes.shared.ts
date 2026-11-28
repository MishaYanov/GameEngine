import {inject} from '@angular/core';
import {Router, type Routes} from '@angular/router';
import type {Editor} from './modules/editor/editor';

export const applicationRoutes: Routes = [
    {
        path: 'setup',
        loadComponent: () =>
            import('./modules/project-setup/project-setup').then((m) => m.ProjectSetup),
    },
    {
        path: 'editor',
        loadComponent: () => import('./modules/editor/editor').then((m) => m.Editor),
        canDeactivate: [(component: Editor) => component.reviewRouteDeparture(inject(Router).currentNavigation()?.trigger === 'popstate')],
    },
];
