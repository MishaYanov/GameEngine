import {Routes} from '@angular/router';
import {environment} from '../environments/environment';
import {applicationRoutes} from './app.routes.shared';

// Startup routing is shared by the browser and Tauri. Explicit deep links are retained.
export function developmentRoutes(startInEditor: boolean): Routes {
    return [
        {path: '', pathMatch: 'full', redirectTo: startInEditor ? 'editor' : 'setup'},
        ...applicationRoutes,
        {path: 'demo', pathMatch: 'full', redirectTo: 'editor'},
        {
            path: 'design',
            loadChildren: () =>
                import('../../projects/design/src/design.routes').then((m) => m.designRoutes),
        },
        {path: '**', redirectTo: 'setup'},
    ];
}

export const routes = developmentRoutes(environment.startInEditor);
