import {ApplicationConfig, provideBrowserGlobalErrorListeners} from '@angular/core';
import {provideRouter} from '@angular/router';

import {routes} from './app.routes';
import {provideEditorTheme} from './core/editor/settings/editor-theme.service';

export const appConfig: ApplicationConfig = {
    providers: [provideBrowserGlobalErrorListeners(), provideEditorTheme(), provideRouter(routes)],
};
