import {Component, provideBrowserGlobalErrorListeners} from '@angular/core';
import {bootstrapApplication} from '@angular/platform-browser';
import {provideRouter, RouterOutlet} from '@angular/router';
import {provideEditorTheme} from '../../../src/app/core/editor/settings/editor-theme.service';
import {designRoutes} from './design.routes';

@Component({selector: 'app-root', imports: [RouterOutlet], template: '<router-outlet />'})
class DesignApp {
}

bootstrapApplication(DesignApp, {
    providers: [provideBrowserGlobalErrorListeners(), provideEditorTheme(), provideRouter(designRoutes)],
}).catch((error: unknown) => console.error(error));
