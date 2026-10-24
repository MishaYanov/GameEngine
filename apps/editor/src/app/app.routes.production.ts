import {Routes} from '@angular/router';
import {applicationRoutes} from './app.routes.shared';

// Development entry points and the design workbench are excluded from production.
export const productionRoutes: Routes = [
    {path: '', pathMatch: 'full', redirectTo: 'setup'},
    ...applicationRoutes,
    {path: '**', redirectTo: 'setup'},
];
