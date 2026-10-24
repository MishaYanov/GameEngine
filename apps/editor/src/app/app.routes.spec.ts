import {TestBed} from '@angular/core/testing';
import {provideRouter, Router} from '@angular/router';
import {RouterTestingHarness} from '@angular/router/testing';
import {productionRoutes as routes} from './app.routes.production';
import {Editor} from './modules/editor/editor';

describe('Production routes', () => {
    it.each(['/demo', '/design'])('does not expose %s', async (path) => {
        TestBed.configureTestingModule({providers: [provideRouter(routes)]});
        const harness = await RouterTestingHarness.create(path);
        expect(TestBed.inject(Router).url).toBe('/setup');
        expect(harness.routeNativeElement?.querySelector('h1')?.textContent).toContain(
            'Create your project',
        );
        expect(harness.routeNativeElement?.querySelector('editor')).toBeNull();
        expect(harness.routeNativeElement?.querySelector('editor-design-workbench')).toBeNull();
    });
    it('opens the editor directly in production', async () => {
        TestBed.configureTestingModule({providers: [provideRouter(routes)]});
        const harness = await RouterTestingHarness.create();
        expect(await harness.navigateByUrl('/editor', Editor)).toBeInstanceOf(Editor);
    });
});
