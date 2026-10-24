import {Routes} from '@angular/router';

export const designRoutes: Routes = [
    {path: '', loadComponent: () => import('./workbench').then((m) => m.DesignWorkbench)},
];
