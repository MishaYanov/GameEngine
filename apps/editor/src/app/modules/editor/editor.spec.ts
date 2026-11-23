import {ComponentFixture, TestBed} from '@angular/core/testing';
import {By} from '@angular/platform-browser';
import {provideRouter} from '@angular/router';
import {invoke, isTauri} from '@tauri-apps/api/core';
import {listen} from '@tauri-apps/api/event';
import axe from 'axe-core';
import {vi} from 'vitest';

import {Editor} from './editor';
import {EditorHeader} from './components/editor-header/editor-header';
import {EditorWorkspace} from './workspace/editor-workspace';

vi.mock('@tauri-apps/api/core', () => ({invoke: vi.fn(), isTauri: vi.fn(() => false)}));
vi.mock('@tauri-apps/api/event', () => ({listen: vi.fn(async () => () => undefined)}));

describe('Editor', () => {
    let component: Editor;
    let fixture: ComponentFixture<Editor>;

    beforeEach(async () => {
        vi.mocked(isTauri).mockReturnValue(false);
        await TestBed.configureTestingModule({
            imports: [Editor],
            providers: [provideRouter([])],
        }).compileComponents();

        fixture = TestBed.createComponent(Editor);
        component = fixture.componentInstance;
        await fixture.whenStable();
    });

    it('should create', () => {
        expect(component).toBeTruthy();
    });

    it('has no structural accessibility violations', async () => {
        // Color contrast needs a rendering browser and is checked in the visual audit.
        const results = await axe.run(fixture.nativeElement, {
            rules: {'color-contrast': {enabled: false}},
        });
        expect(
            results.violations.map((violation) => ({
                id: violation.id,
                nodes: violation.nodes.map((node) => node.target),
            })),
        ).toEqual([]);
    });
});

it('waits for the retained workspace check before offering to open a project', async () => {
    vi.mocked(isTauri).mockReturnValue(true);
    let reportEmpty!: (cause: unknown) => void;
    const restore = new Promise<never>((_, reject) => {
        reportEmpty = reject;
    });
    vi.mocked(invoke).mockReset().mockImplementation(async operation => {
        if (operation === 'editor_snapshot') return restore;
        if (operation === 'editor_capabilities') return {
            protocol_version: 2, scene_schema_version: 3, embedded_viewport_supported: true,
        };
        return null;
    });
    await TestBed.configureTestingModule({imports: [Editor], providers: [provideRouter([])]}).compileComponents();
    const fixture = TestBed.createComponent(Editor);
    fixture.detectChanges();
    const root: HTMLElement = fixture.nativeElement;
    const button = root.querySelector<HTMLButtonElement>('.empty-open')!;
    expect(button.disabled).toBe(true);
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith('editor_snapshot', undefined));
    reportEmpty('Open a scene before using the editor');
    await vi.waitFor(() => {
        fixture.detectChanges();
        expect(button.disabled).toBe(false);
    });
    expect(root.querySelector('.open-error')).toBeNull();
    button.click();
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith('choose_project_manifest', undefined));
});

describe('desktop close review', () => {
    const closeEvent = 'editor://window-close-requested';

    beforeEach(async () => {
        vi.mocked(isTauri).mockReturnValue(true);
        vi.mocked(listen).mockClear();
        vi.mocked(invoke).mockReset().mockImplementation(async command => {
            if (command === 'editor_snapshot') throw new Error('Open a scene before using the editor');
            if (command === 'editor_capabilities') return {
                protocol_version: 2, scene_schema_version: 3, embedded_viewport_supported: true,
            };
            // Cancel the native confirmation after the Angular draft review.
            if (command === 'editor_exit') return false;
            return undefined;
        });
        await TestBed.configureTestingModule({imports: [Editor], providers: [provideRouter([])]}).compileComponents();
    });

    afterEach(() => {
        vi.restoreAllMocks();
        vi.mocked(listen).mockClear();
    });

    it('uses the same cancellable draft review for title-bar and File → Exit, once at a time', async () => {
        let decide!: (approved: boolean) => void;
        const review = vi.spyOn(EditorWorkspace.prototype, 'reviewProjectNavigation')
            .mockImplementationOnce(() => new Promise<boolean>(resolve => decide = resolve))
            .mockResolvedValue(true);
        const fixture = TestBed.createComponent(Editor);
        await fixture.whenStable();

        const registration = vi.mocked(invoke).mock.calls.find(([name, payload]) =>
            name === 'editor_set_window_close_review' && payload && 'enabled' in payload && payload['enabled'] === true);
        const registrationPayload = registration?.[1];
        const owner = registrationPayload && 'owner' in registrationPayload ? registrationPayload['owner'] : undefined;
        expect(typeof owner).toBe('string');
        const eventListener = vi.mocked(listen).mock.calls.find(([event]) => event === closeEvent)!;
        expect(eventListener[2]).toEqual({target: {kind: 'WebviewWindow', label: 'main'}});
        const sendClose = (payload: unknown) => eventListener[1]({event: closeEvent, id: 1, payload});
        const header = fixture.debugElement.query(By.directive(EditorHeader)).componentInstance as EditorHeader;

        sendClose('another-editor');
        expect(review).not.toHaveBeenCalled();
        sendClose(owner);
        sendClose(owner);
        header.exitRequested.emit();
        expect(review).toHaveBeenCalledTimes(1);
        decide(false);
        await vi.waitFor(() => {
            fixture.detectChanges();
            expect(header.canExit()).toBe(true);
        });
        expect(invoke).not.toHaveBeenCalledWith('editor_exit', expect.anything());

        sendClose(owner);
        await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith('editor_exit', {expectedRevision: null}));
        await vi.waitFor(() => {
            fixture.detectChanges();
            expect(header.canExit()).toBe(true);
        });
        header.exitRequested.emit();
        await vi.waitFor(() => expect(vi.mocked(invoke).mock.calls.filter(([command]) => command === 'editor_exit')).toHaveLength(2));
        expect(review).toHaveBeenCalledTimes(3);
        fixture.destroy();
        sendClose(owner);
        expect(review).toHaveBeenCalledTimes(3);
        await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith('editor_set_window_close_review', {owner, enabled: false}));
    });

    it('keeps local drafts untouched when the Open Project picker is canceled', async () => {
        const review = vi.spyOn(EditorWorkspace.prototype, 'reviewProjectNavigation').mockResolvedValue(false);
        const useNative = vi.mocked(invoke).getMockImplementation()!;
        let manifest: string | null = null;
        vi.mocked(invoke).mockImplementation((command, payload, options) =>
            command === 'choose_project_manifest' ? Promise.resolve(manifest) : useNative(command, payload, options));
        const fixture = TestBed.createComponent(Editor);
        await fixture.whenStable();
        const header = fixture.debugElement.query(By.directive(EditorHeader)).componentInstance as EditorHeader;

        header.openProjectRequested.emit();
        await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith('choose_project_manifest', undefined));
        await fixture.whenStable();
        expect(review).not.toHaveBeenCalled();

        manifest = 'D:/Projects/Example/project.json';
        header.openProjectRequested.emit();
        await vi.waitFor(() => expect(review).toHaveBeenCalledOnce());
        expect(invoke).not.toHaveBeenCalledWith('editor_open_project', expect.anything());
    });

    it('keeps File → Exit available if registering the title-bar listener fails', async () => {
        const useNative = vi.mocked(invoke).getMockImplementation()!;
        vi.mocked(invoke).mockImplementation((command, payload, options) =>
            command === 'editor_set_window_close_review'
                ? Promise.reject(new Error('The event bridge is unavailable'))
                : useNative(command, payload, options));
        const fixture = TestBed.createComponent(Editor);
        await fixture.whenStable();
        const header = fixture.debugElement.query(By.directive(EditorHeader)).componentInstance as EditorHeader;
        const root: HTMLElement = fixture.nativeElement;
        await vi.waitFor(() => {
            fixture.detectChanges();
            expect(root.querySelector('.open-error')?.textContent ?? '').toContain('File → Exit');
        });
        expect(header.canExit()).toBe(true);
        header.exitRequested.emit();
        await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith('editor_exit', {expectedRevision: null}));
    });

    it.each(['cancel', 'error'] as const)('locks workspace input during native exit and restores it after %s', async outcome => {
        let approveDraftReview!: (approved: boolean) => void;
        const review = vi.spyOn(EditorWorkspace.prototype, 'reviewProjectNavigation')
            .mockImplementation(() => new Promise<boolean>(resolve => approveDraftReview = resolve));
        let finishExit!: () => void;
        const useNative = vi.mocked(invoke).getMockImplementation()!;
        const fixture = TestBed.createComponent(Editor);
        await fixture.whenStable();
        const root: HTMLElement = fixture.nativeElement;
        const workspace = root.querySelector<HTMLElement>('editor-workspace')!;
        const header = fixture.debugElement.query(By.directive(EditorHeader)).componentInstance as EditorHeader;
        vi.mocked(invoke).mockImplementation((command, payload, options) => {
            if (command !== 'editor_exit') return useNative(command, payload, options);
            expect(workspace.hasAttribute('inert')).toBe(true);
            return new Promise((resolve, reject) => {
                finishExit = outcome === 'cancel' ? () => resolve(false) : () => reject(new Error('Exit retry needed.'));
            });
        });

        header.exitRequested.emit();
        expect(review).toHaveBeenCalledTimes(1);
        expect(workspace.hasAttribute('inert')).toBe(false);
        approveDraftReview(true);
        await vi.waitFor(() => {
            fixture.detectChanges();
            expect(finishExit).toBeTypeOf('function');
            expect(workspace.hasAttribute('inert')).toBe(true);
            expect(root.querySelector('.open-status')?.textContent).toContain('Closing the editor');
            expect(root.querySelector('.open-status')?.closest('[inert]')).toBeNull();
        });

        finishExit();
        await vi.waitFor(() => {
            fixture.detectChanges();
            expect(workspace.hasAttribute('inert')).toBe(false);
            expect(header.canExit()).toBe(true);
            if (outcome === 'error') expect(root.querySelector('.open-error')?.textContent).toContain('Exit retry needed.');
        });
    });
});
