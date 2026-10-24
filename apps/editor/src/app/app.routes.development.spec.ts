import {TestBed} from '@angular/core/testing';
import {provideRouter, Router} from '@angular/router';
import {RouterTestingHarness} from '@angular/router/testing';
import {vi} from 'vitest';
import {isTauri} from '@tauri-apps/api/core';
import {developmentRoutes} from './app.routes.development';
import {Editor} from './modules/editor/editor';

vi.mock('@tauri-apps/api/core', () => ({isTauri: vi.fn(), invoke: vi.fn()}));

describe('Development startup', () => {
    it.each([false, true])('initializes the editor at startup (Tauri: %s)', async (desktop) => {
        vi.mocked(isTauri).mockReturnValue(desktop);
        TestBed.configureTestingModule({providers: [provideRouter(developmentRoutes(true))]});
        const harness = await RouterTestingHarness.create('/');
        expect(TestBed.inject(Router).url).toBe('/editor');
        expect(harness.routeDebugElement?.componentInstance).toBeInstanceOf(Editor);
    });

    it('allows disabling editor startup for staging', async () => {
        vi.mocked(isTauri).mockReturnValue(false);
        TestBed.configureTestingModule({providers: [provideRouter(developmentRoutes(false))]});
        const harness = await RouterTestingHarness.create('/');
        expect(TestBed.inject(Router).url).toBe('/setup');
        expect(harness.routeNativeElement?.querySelector('h1')?.textContent).toContain(
            'Create your project',
        );
    });

    it('keeps explicit setup links and redirects the old demo URL to the editor', async () => {
        vi.mocked(isTauri).mockReturnValue(false);
        TestBed.configureTestingModule({providers: [provideRouter(developmentRoutes(true))]});
        const harness = await RouterTestingHarness.create('/setup');
        expect(TestBed.inject(Router).url).toBe('/setup');
        await harness.navigateByUrl('/demo', Editor);
        expect(TestBed.inject(Router).url).toBe('/editor');
    });
});
