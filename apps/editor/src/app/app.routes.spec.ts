import {TestBed} from '@angular/core/testing';
import {provideRouter, Router} from '@angular/router';
import {RouterTestingHarness} from '@angular/router/testing';
import {invoke, isTauri} from '@tauri-apps/api/core';
import {vi} from 'vitest';
import {productionRoutes as routes} from './app.routes.production';
import {Editor} from './modules/editor/editor';
import {EditorWorkspace} from './modules/editor/workspace/editor-workspace';

vi.mock('@tauri-apps/api/core', () => ({invoke: vi.fn(), isTauri: vi.fn(() => false)}));
vi.mock('@tauri-apps/api/event', () => ({listen: vi.fn(async () => () => undefined)}));

describe('Production routes', () => {
    beforeEach(() => vi.mocked(isTauri).mockReturnValue(false));
    afterEach(() => vi.restoreAllMocks());
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

    it('keeps the editor and its drafts mounted when the New Project route is cancelled', async () => {
        vi.mocked(isTauri).mockReturnValue(true);
        vi.mocked(invoke).mockReset().mockImplementation(async command => {
            if (command === 'editor_snapshot') throw new Error('Open a scene before using the editor');
            if (command === 'editor_capabilities') return {
                protocol_version: 2, scene_schema_version: 3, embedded_viewport_supported: true,
            };
            return undefined;
        });
        let decide!: (approved: boolean) => void;
        const review = vi.spyOn(EditorWorkspace.prototype, 'reviewProjectNavigation')
            .mockImplementationOnce(() => new Promise<boolean>(resolve => decide = resolve))
            .mockResolvedValueOnce(true);

        TestBed.configureTestingModule({providers: [provideRouter(routes)]});
        const router = TestBed.inject(Router);
        const harness = await RouterTestingHarness.create('/editor');
        const editor = harness.routeDebugElement?.componentInstance;
        const toSetup = '/setup?from=editor';
        const pending = router.navigateByUrl(toSetup);
        await vi.waitFor(() => expect(review).toHaveBeenCalledTimes(1));
        expect(router.url).toBe('/editor');
        decide(false);
        expect(await pending).toBe(false);
        expect(harness.routeDebugElement?.componentInstance).toBe(editor);

        await harness.navigateByUrl(toSetup);
        expect(router.url).toBe(toSetup);
        expect(review).toHaveBeenCalledTimes(2);
        expect(harness.routeNativeElement?.querySelector('h1')?.textContent).toContain('Create your project');
        expect(invoke).not.toHaveBeenCalledWith('editor_exit', expect.anything());
    });
});
